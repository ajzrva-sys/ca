//! Native execution facade: no scheduler, provider selection or session owner.
use crate::runtime::{
    ExecutionContext, ExecutionReceipt, ExecutionState, RunStore, EXECUTION_STORE,
};
use anyhow::{Context, Result};
use ca_automation_protocol::{
    MechanisticActionV1, MechanisticRequestV1, WorkflowExecutionStatusV1, WorkflowRequestV1,
    WorkflowResponseV1,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)] // One owned worker envelope; keep caller DTOs unchanged.
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EngineOperation {
    Workflow {
        config_path: PathBuf,
        request: WorkflowRequestV1,
    },
    Mechanistic {
        repository: PathBuf,
        request: MechanisticRequestV1,
    },
}
pub struct AutomationEngine;
impl AutomationEngine {
    /// These typed actions apply required isolation at every repository command
    /// boundary. Hosts may leave their trusted coordinator outside an outer
    /// sandbox, which cannot be nested on supported macOS kernels.
    /// Query/observation-only requests retain the host sandbox. Hardened typed
    /// queries may accompany evidence in the trusted coordinator.
    pub fn requires_command_isolation(actions: &[MechanisticActionV1]) -> bool {
        actions.iter().any(MechanisticActionV1::is_evidence)
    }
    pub fn workflow_local_roots(config: &std::path::Path) -> Result<Vec<PathBuf>> {
        crate::remote::workflow_local_roots(config)
    }
    pub fn workflow_credential_env(config: &std::path::Path) -> Result<Vec<String>> {
        crate::remote::workflow_credential_env(config)
    }
    pub fn repository_revision(repository: &std::path::Path) -> Result<String> {
        crate::revision::repository_revision(&crate::paths::Paths::new(repository)?)
    }
    pub fn preview(
        context: &ExecutionContext,
        operation: &EngineOperation,
    ) -> Result<serde_json::Value> {
        context.validate().map_err(anyhow::Error::msg)?;
        match operation {
            EngineOperation::Workflow { request, .. } => {
                ca_automation_protocol::validate_workflow_request(request)
                    .map_err(anyhow::Error::msg)?
            }
            EngineOperation::Mechanistic { request, .. } => {
                crate::mechanistic::validate_request(request)?;
                anyhow::ensure!(
                    !Self::requires_command_isolation(&request.actions)
                        || !request.allow_unisolated_evidence,
                    "native verification requires command isolation; unisolated evidence is a legacy advisory option"
                );
            }
        }
        Ok(
            serde_json::json!({"schema":"ca.automation-execution-preview.v2","run_id":context.run_id,"definition_digest":context.definition_digest,"resource_binding_digest":context.resource_binding_digest,"policy_digest":context.policy_digest}),
        )
    }
    pub fn status(context: &ExecutionContext) -> Result<ExecutionReceipt> {
        RunStore::open(context)?.status()
    }
    pub fn cancel(context: &ExecutionContext) -> Result<ExecutionReceipt> {
        let s = RunStore::open(context)?;
        s.cancel()?;
        s.status()
    }
    pub fn reconcile(context: &ExecutionContext) -> Result<ExecutionReceipt> {
        RunStore::open(context)?.reconcile()
    }
    pub async fn start(
        context: &ExecutionContext,
        operation: EngineOperation,
    ) -> Result<ExecutionReceipt> {
        Self::preview(context, &operation)?;
        let store = RunStore::open(context)?;
        let existing = store.status()?;
        if existing.state.terminal() {
            return Ok(existing);
        }
        let workflow_run = matches!(&operation, EngineOperation::Workflow { request, .. }
            if matches!(&request.operation, ca_automation_protocol::WorkflowOperationV1::Run { .. }));
        let workflow_admission = if workflow_run {
            match crate::automation_workflow::admit_workflow_run(&store, true).await? {
                Some(admission) => Some(admission),
                None => return store.status(),
            }
        } else {
            None
        };
        let _owner = if workflow_run {
            None
        } else {
            Some(store.execution_lease()?)
        };
        let _resources = if workflow_run {
            Vec::new()
        } else {
            store.acquire_resources()?
        };
        // Compatibility runs acquire this same machine-wide ownership. A
        // conservative key covers manifests that bind overlapping remote paths.
        let _workflow_mutation = if matches!(&operation, EngineOperation::Workflow { request, .. }
            if matches!(&request.operation, ca_automation_protocol::WorkflowOperationV1::Run { .. }))
            && !context
                .resources
                .iter()
                .any(|resource| resource == "typed-workflow-mutation")
        {
            anyhow::ensure!(
                !crate::runtime::worker_contained(),
                "host did not admit the typed workflow ownership resource"
            );
            Some(crate::runtime::ResourceLease::acquire(
                &context.lease_root,
                "typed-workflow-mutation",
            )?)
        } else {
            None
        };
        let status = store.start()?;
        if status.state.terminal() {
            return Ok(status);
        }
        store.prepare_action("target")?;
        match store.begin_action("target") {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                drop(_owner);
                drop(workflow_admission);
                return store.reconcile();
            }
        }
        let result = EXECUTION_STORE
            .scope(store.clone(), async {
                match operation {
                    EngineOperation::Workflow {
                        config_path,
                        request,
                    } => {
                        let response = crate::automation_workflow::execute(
                            &config_path,
                            &context.state_root,
                            request,
                        )
                        .await?;
                        let state = match &response {
                            WorkflowResponseV1::Execution { receipt } => match receipt.status {
                                WorkflowExecutionStatusV1::Completed => ExecutionState::Succeeded,
                                WorkflowExecutionStatusV1::Cancelled => ExecutionState::Cancelled,
                                WorkflowExecutionStatusV1::Stale
                                | WorkflowExecutionStatusV1::Blocked => {
                                    ExecutionState::NeedsAttention
                                }
                                _ => ExecutionState::Failed,
                            },
                            _ => ExecutionState::Succeeded,
                        };
                        Ok((state, serde_json::to_value(response)?))
                    }
                    EngineOperation::Mechanistic {
                        repository,
                        request,
                    } => {
                        let cancellation = crate::cancellation::CancellationToken::default();
                        let worker_cancel = cancellation.clone();
                        let watcher_store = store.clone();
                        let watcher = tokio::spawn(async move {
                            loop {
                                if watcher_store.cancel_requested().unwrap_or(true) {
                                    worker_cancel.cancel();
                                    break;
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            }
                        });
                        let execution_store = store.clone();
                        let worker = tokio::task::spawn_blocking(move || {
                            EXECUTION_STORE.sync_scope(execution_store, || {
                                let _json = crate::machine_json::MachineJsonModeGuard::enter();
                                let executor =
                                    crate::mechanistic::MechanisticExecutor::new(&repository)?;
                                if request.actions.iter().all(|a| a.is_query()) {
                                    executor.execute_queries(&request)
                                } else if request.actions.iter().all(|a| a.is_evidence()) {
                                    executor
                                        .execute_evidence_with_cancellation(&request, &cancellation)
                                } else {
                                    execute_mixed_mechanistic(&request, &cancellation, |fragment| {
                                        if fragment.actions[0].is_query() {
                                            executor.execute_queries(fragment)
                                        } else {
                                            executor.execute_evidence_with_cancellation(
                                                fragment,
                                                &cancellation,
                                            )
                                        }
                                    })
                                }
                            })
                        })
                        .await;
                        watcher.abort();
                        let receipt = worker.context("native evidence worker stopped")??;
                        let state = if receipt.status
                            == ca_automation_protocol::MechanisticReceiptStatusV1::Completed
                        {
                            ExecutionState::Succeeded
                        } else {
                            ExecutionState::Failed
                        };
                        Ok::<_, anyhow::Error>((state, serde_json::to_value(receipt)?))
                    }
                }
            })
            .await;
        match result {
            Ok((state, value)) => {
                if store.status()?.actions.iter().any(|action| {
                    action.id != "target"
                        && matches!(
                            action.state,
                            crate::runtime::ActionState::Running
                                | crate::runtime::ActionState::Unknown
                        )
                }) {
                    return store.finish(ExecutionState::NeedsAttention, None,
                        Some("workflow action has an unconfirmed effect; adapter read-back is required".into()));
                }
                store.confirm_action(
                    "target",
                    serde_json::json!({"execution_state":state,"result":value}),
                )?;
                store.finish(state, Some(value), None)
            }
            Err(error) => store.finish(
                ExecutionState::NeedsAttention,
                None,
                Some(format!("{error:#}")),
            ),
        }
    }
}

/// V2 composes the existing V1 endpoints in request order. V1 callers retain
/// their query-only/evidence-only contracts and original digest inputs.
fn execute_mixed_mechanistic(
    request: &MechanisticRequestV1,
    cancellation: &crate::cancellation::CancellationToken,
    mut execute: impl FnMut(
        &MechanisticRequestV1,
    ) -> Result<ca_automation_protocol::MechanisticReceiptV1>,
) -> Result<ca_automation_protocol::MechanisticReceiptV1> {
    use ca_automation_protocol::{MechanisticReceiptStatusV1, MechanisticReceiptV1};
    crate::mechanistic::validate_request(request)?;
    anyhow::ensure!(
        request
            .actions
            .iter()
            .all(|action| action.is_query() || action.is_evidence()),
        "native mixed verification accepts typed queries and evidence only"
    );
    let store =
        crate::runtime::current_run_store().context("mixed execution has no admitted run")?;
    let limit = ca_automation_protocol::MAX_MECHANISTIC_OUTPUT_BYTES
        .min(store.context().output_bytes as usize);
    let action_budget = limit.saturating_sub(1024) / request.actions.len();
    anyhow::ensure!(
        action_budget >= 512,
        "mixed receipt cannot fit the admitted output bound"
    );
    let full_digest = crate::mechanistic::request_digest(request)?;
    let started = std::time::Instant::now();
    let mut combined: Option<MechanisticReceiptV1> = None;
    for (index, action) in request.actions.iter().enumerate() {
        anyhow::ensure!(
            !cancellation.is_cancelled() && !store.cancel_requested()?,
            "native batch cancelled"
        );
        let mut fragment_request = request.clone();
        fragment_request.actions = vec![action.clone()];
        let fragment_digest = crate::mechanistic::request_digest(&fragment_request)?;
        let id = format!("mechanistic_{index}");
        store.prepare_action(&id)?;
        let mut fragment: MechanisticReceiptV1 = if store.begin_action(&id)? {
            let mut fragment = execute(&fragment_request)?;
            anyhow::ensure!(
                fragment.schema == ca_automation_protocol::MECHANISTIC_RECEIPT_SCHEMA
                    && fragment.request_digest == fragment_digest
                    && fragment.repository_revision == request.expected_revision
                    && fragment.action_results.len() == 1
                    && fragment.action_results[0].action_index == 0,
                "typed action did not produce its bound receipt"
            );
            bound_mixed_action(&mut fragment.action_results[0], action_budget)?;
            store.confirm_action(&id, serde_json::json!({
                "schema":"ca.mechanistic-action-checkpoint.v2", "action_index":index,
                "request_digest":full_digest,"fragment_digest":fragment_digest,"receipt":fragment,
            }))?;
            fragment
        } else {
            let saved = store
                .status()?
                .actions
                .into_iter()
                .find(|saved| saved.id == id)
                .and_then(|saved| saved.result)
                .context("confirmed action has no receipt")?;
            anyhow::ensure!(
                saved["schema"] == "ca.mechanistic-action-checkpoint.v2"
                    && saved["action_index"] == index
                    && saved["request_digest"] == full_digest
                    && saved["fragment_digest"] == fragment_digest,
                "confirmed mixed action binding changed"
            );
            serde_json::from_value(saved["receipt"].clone())?
        };
        anyhow::ensure!(
            fragment.schema == ca_automation_protocol::MECHANISTIC_RECEIPT_SCHEMA
                && fragment.request_digest == fragment_digest
                && fragment.repository_revision == request.expected_revision
                && fragment.action_results.len() == 1
                && fragment.action_results[0].action_index == 0,
            "mixed action receipt identity changed"
        );
        fragment.action_results[0].action_index = index as u8;
        let complete = fragment.status == MechanisticReceiptStatusV1::Completed;
        if let Some(combined) = combined.as_mut() {
            combined.finished_at = fragment.finished_at;
            combined.status = fragment.status;
            combined.readiness = fragment.readiness;
            combined.action_results.extend(fragment.action_results);
        } else {
            fragment.request_digest = full_digest.clone();
            fragment.broker = None;
            fragment.capsule = None;
            combined = Some(fragment);
        }
        if !complete {
            break;
        }
    }
    let mut combined = combined.context("mixed batch produced no receipt")?;
    combined.elapsed_millis = started.elapsed().as_millis() as u64;
    combined.output_bytes = combined
        .action_results
        .iter()
        .map(|action| action.summary.len() + action.entries.iter().map(String::len).sum::<usize>())
        .sum::<usize>() as u64;
    anyhow::ensure!(
        serde_json::to_vec(&combined)?.len() <= limit,
        "mixed receipt exceeded the admitted output bound"
    );
    Ok(combined)
}

fn bound_mixed_action(
    action: &mut ca_automation_protocol::MechanisticActionResultV1,
    limit: usize,
) -> Result<()> {
    while serde_json::to_vec(action)?.len() > limit {
        action.truncated = true;
        if !action.entries.is_empty() {
            action.entries.truncate(action.entries.len() / 2);
        } else if action.summary.len() > 128 {
            action.summary = action
                .summary
                .chars()
                .take(action.summary.chars().count() / 2)
                .collect();
        } else if action.decision.is_some() {
            action.decision = None;
        } else {
            anyhow::bail!("mixed action metadata exceeds the admitted bound");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mixed_context(root: &std::path::Path, id: &str) -> ExecutionContext {
        ExecutionContext {
            schema: crate::runtime::EXECUTION_SCHEMA.into(),
            run_id: id.into(),
            profile: "default".into(),
            state_root: root.join("state"),
            lease_root: root.join("leases"),
            definition_digest: "a".repeat(64),
            resource_binding_digest: "b".repeat(64),
            policy_digest: "c".repeat(64),
            resources: Vec::new(),
            runtime_seconds: 30,
            output_bytes: 8192,
        }
    }
    fn mixed_request(revision: String) -> MechanisticRequestV1 {
        serde_json::from_value(serde_json::json!({
            "schema":ca_automation_protocol::MECHANISTIC_REQUEST_SCHEMA,"expected_revision":revision,
            "actions":[{"kind":"git_status_summary","max_entries":20},
                {"kind":"evidence","recipe":{"kind":"builtin","check":"snapshot_hygiene"}}]
        })).unwrap()
    }
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn native_mixed_batch_executes_actual_query_and_evidence_in_order() -> Result<()> {
        if std::env::var_os("CA_NATIVE_MIXED_REAL_FIXTURE").is_none() {
            // Other parallel library fixtures enable a process-wide debug
            // backend. This exact subprocess always exercises real isolation.
            let output = std::process::Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "native::tests::native_mixed_batch_executes_actual_query_and_evidence_in_order",
                    "--nocapture",
                ])
                .env("CA_NATIVE_MIXED_REAL_FIXTURE", "1")
                .env_remove("CODEX_TEAM_TEST_ISOLATION_READY")
                .output()?;
            anyhow::ensure!(
                output.status.success(),
                "actual mixed fixture failed: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return Ok(());
        }
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().canonicalize()?;
        let repository = root.join("repository");
        std::fs::create_dir(&repository)?;
        std::fs::create_dir(repository.join("src"))?;
        std::fs::write(
            repository.join("Cargo.toml"),
            "[package]\nname='native_mixed_fixture'\nversion='0.1.0'\nedition='2021'\n",
        )?;
        std::fs::write(
            repository.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"native_mixed_fixture\"\nversion = \"0.1.0\"\n",
        )?;
        std::fs::write(repository.join("src/lib.rs"), "#[test]\nfn physical_fixture() { assert!(std::fs::write(\"mixed-physical-marker\", b\"passed\").is_ok()); }\n")?;
        for args in [
            vec!["init", "--quiet"],
            vec!["add", "Cargo.toml", "Cargo.lock", "src/lib.rs"],
        ] {
            anyhow::ensure!(
                crate::git_metadata::command_no_filters(&repository)?
                    .args(args)
                    .status()?
                    .success(),
                "initialize mixed native fixture"
            );
        }
        let mut request = mixed_request(AutomationEngine::repository_revision(&repository)?);
        request.actions[1] = serde_json::from_value(serde_json::json!({
            "kind":"evidence","recipe":{"kind":"cargo_test","filter":null},"timeout_seconds":45
        }))?;
        let original = serde_json::to_vec(&request)?;
        let executor = crate::mechanistic::MechanisticExecutor::new(&repository)?;
        assert!(
            executor.execute_evidence(&request).is_err(),
            "legacy evidence contract widened"
        );
        let context = mixed_context(&root, "actual-mixed");
        let receipt = AutomationEngine::start(
            &context,
            EngineOperation::Mechanistic {
                repository: repository.clone(),
                request: request.clone(),
            },
        )
        .await?;
        assert_eq!(
            receipt.state,
            ExecutionState::Succeeded,
            "{:?}",
            receipt.reason
        );
        let result: ca_automation_protocol::MechanisticReceiptV1 =
            serde_json::from_value(receipt.result.clone().unwrap())?;
        assert_eq!(
            result.request_digest,
            crate::mechanistic::request_digest(&request)?
        );
        assert_eq!(result.action_results.len(), 2);
        assert_eq!(
            (
                result.action_results[0].action_index,
                result.action_results[0].category.as_str()
            ),
            (0, "query")
        );
        assert_eq!(
            (
                result.action_results[1].action_index,
                result.action_results[1].category.as_str()
            ),
            (1, "evidence")
        );
        assert!(
            !repository.join("mixed-physical-marker").exists(),
            "physical command ran in the governed tree"
        );
        for id in ["mechanistic_0", "mechanistic_1"] {
            assert_eq!(
                receipt
                    .actions
                    .iter()
                    .find(|action| action.id == id)
                    .unwrap()
                    .state,
                crate::runtime::ActionState::Confirmed
            );
        }
        assert_eq!(serde_json::to_vec(&request)?, original);
        assert_eq!(
            AutomationEngine::start(
                &context,
                EngineOperation::Mechanistic {
                    repository,
                    request
                }
            )
            .await?,
            receipt
        );
        Ok(())
    }
    #[tokio::test]
    async fn mixed_checkpoint_bounds_results_and_never_repeats_an_unconfirmed_action() -> Result<()>
    {
        let temporary = tempfile::tempdir()?;
        let context = mixed_context(&temporary.path().canonicalize()?, "mixed-interruption");
        let store = RunStore::open(&context)?;
        store.start()?;
        let request = mixed_request("d".repeat(64));
        let mut calls = Vec::new();
        EXECUTION_STORE.scope(store.clone(), async {
            let result = execute_mixed_mechanistic(&request, &crate::cancellation::CancellationToken::default(), |fragment| {
                calls.push(fragment.actions[0].category());
                if fragment.actions[0].is_evidence() { anyhow::bail!("effect completed but process interrupted before confirmation"); }
                Ok(ca_automation_protocol::MechanisticReceiptV1 {
                    schema:ca_automation_protocol::MECHANISTIC_RECEIPT_SCHEMA.into(),request_digest:crate::mechanistic::request_digest(fragment)?,
                    repository_revision:fragment.expected_revision.clone(),status:ca_automation_protocol::MechanisticReceiptStatusV1::Completed,
                    readiness:ca_automation_protocol::DelegationReadinessV1 { queries_available:true,evidence_available:true,observation_available:true,evidence_backend:Some("fixture".into()),reasons:Vec::new() },
                    action_results:vec![ca_automation_protocol::MechanisticActionResultV1 {
                        action_index:0,category:"query".into(),status:ca_automation_protocol::MechanisticReceiptStatusV1::Completed,
                        summary:"bounded query fixture".into(),entries:vec!["x".repeat(2048);100],truncated:false,duration_millis:1,decision:None,
                    }],output_bytes:0,started_at:"start".into(),finished_at:"finish".into(),elapsed_millis:1,fallback_reason:None,broker:None,capsule:None,
                })
            });
            assert!(result.is_err());
            assert_eq!(calls, ["query","evidence"]);
            let first = store.status()?.actions.into_iter().find(|action| action.id == "mechanistic_0").unwrap();
            assert_eq!(first.state, crate::runtime::ActionState::Confirmed);
            assert!(first.result.as_ref().unwrap()["receipt"]["action_results"][0]["truncated"].as_bool().unwrap());
            assert!(serde_json::to_vec(&first.result)?.len() < context.output_bytes as usize);
            assert!(execute_mixed_mechanistic(&request, &crate::cancellation::CancellationToken::default(), |_| {
                anyhow::bail!("a confirmed or ambiguous action was repeated")
            }).unwrap_err().to_string().contains("unconfirmed effect"));
            assert_eq!(store.reconcile()?.state, ExecutionState::NeedsAttention);
            Ok::<_, anyhow::Error>(())
        }).await?;
        Ok(())
    }

    #[test]
    fn command_isolation_selection_covers_any_evidence_in_a_typed_batch() {
        let evidence: MechanisticActionV1 = serde_json::from_value(serde_json::json!({
            "kind":"evidence", "recipe":{"kind":"cargo_check"}
        }))
        .unwrap();
        let changed: MechanisticActionV1 = serde_json::from_value(serde_json::json!({
            "kind":"verify_changed_files", "paths":["src/lib.rs"]
        }))
        .unwrap();
        let query = MechanisticActionV1::GitStatusSummary { max_entries: 20 };
        let observation = MechanisticActionV1::ObserveProcess {
            label: "existing process".into(),
            pid: 1,
            interval_seconds: 1,
        };
        assert!(AutomationEngine::requires_command_isolation(&[
            evidence.clone(),
            changed
        ]));
        assert!(AutomationEngine::requires_command_isolation(&[
            evidence.clone(),
            query.clone()
        ]));
        assert!(AutomationEngine::requires_command_isolation(&[
            evidence.clone(),
            observation.clone()
        ]));
        for actions in [
            vec![],
            vec![query.clone()],
            vec![observation.clone()],
            vec![query, observation],
        ] {
            assert!(!AutomationEngine::requires_command_isolation(&actions));
        }
    }

    #[test]
    fn native_preview_refuses_legacy_advisory_evidence_without_changing_request() {
        let context = ExecutionContext {
            schema: crate::runtime::EXECUTION_SCHEMA.into(),
            run_id: "evidence-preview".into(),
            profile: "default".into(),
            state_root: "/native-preview/state".into(),
            lease_root: "/native-preview/leases".into(),
            definition_digest: "a".repeat(64),
            resource_binding_digest: "b".repeat(64),
            policy_digest: "c".repeat(64),
            resources: Vec::new(),
            runtime_seconds: 10,
            output_bytes: 8192,
        };
        let request: MechanisticRequestV1 = serde_json::from_value(serde_json::json!({
            "schema":ca_automation_protocol::MECHANISTIC_REQUEST_SCHEMA,
            "expected_revision":"a".repeat(64),
            "allow_unisolated_evidence":true,
            "actions":[{"kind":"evidence","recipe":{"kind":"cargo_check"}}]
        }))
        .unwrap();
        let operation = EngineOperation::Mechanistic {
            repository: "/native-preview/repository".into(),
            request,
        };
        let original = serde_json::to_vec(&operation).unwrap();
        assert!(AutomationEngine::preview(&context, &operation)
            .unwrap_err()
            .to_string()
            .contains("requires command isolation"));
        assert_eq!(serde_json::to_vec(&operation).unwrap(), original);
        let EngineOperation::Mechanistic { request, .. } = &operation else {
            unreachable!()
        };
        let mut accepted = request.clone();
        accepted.allow_unisolated_evidence = false;
        assert!(AutomationEngine::preview(
            &context,
            &EngineOperation::Mechanistic {
                repository: "/native-preview/repository".into(),
                request: accepted
            }
        )
        .is_ok());
        let mut mixed = request.clone();
        mixed
            .actions
            .insert(0, MechanisticActionV1::GitStatusSummary { max_entries: 20 });
        let mixed_operation = EngineOperation::Mechanistic {
            repository: "/native-preview/repository".into(),
            request: mixed.clone(),
        };
        let mixed_original = serde_json::to_vec(&mixed_operation).unwrap();
        assert!(AutomationEngine::preview(&context, &mixed_operation)
            .unwrap_err()
            .to_string()
            .contains("requires command isolation"));
        assert_eq!(
            serde_json::to_vec(&mixed_operation).unwrap(),
            mixed_original
        );
        mixed.allow_unisolated_evidence = false;
        assert!(AutomationEngine::preview(
            &context,
            &EngineOperation::Mechanistic {
                repository: "/native-preview/repository".into(),
                request: mixed,
            }
        )
        .is_ok());
    }
}
