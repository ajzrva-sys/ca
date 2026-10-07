//! Durable sidecars for V1/V2 scheduling entry points. Admission stays in ca;
//! original schedule definitions, receipts and confirmation digests stay intact.
use crate::automation_runtime::NativeResult;
use crate::runtime::{ActiveProfile, RuntimePaths};
use ca_automation::runtime::{
    ActionState, ExecutionContext, ExecutionReceipt, ExecutionState, ResourceLease, RunStore,
    EXECUTION_SCHEMA, EXECUTION_STORE,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::path::Path;
use uuid::Uuid;

tokio::task_local! { static RESOURCE_FDS: Vec<i32>; }
tokio::task_local! { static EXECUTOR_FD: i32; }

pub(crate) fn current_resource_fds() -> Option<Vec<i32>> {
    RESOURCE_FDS.try_with(Clone::clone).ok()
}
pub(crate) fn current_executor_fd() -> Option<i32> {
    EXECUTOR_FD.try_with(|fd| *fd).ok()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execution_context(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    run_id: Uuid,
    definition: &impl Serialize,
    policy: &impl Serialize,
    workspace: &Path,
    runtime_secs: u64,
    output_bytes: u64,
) -> Result<ExecutionContext, String> {
    let paths = RuntimePaths::from_home(paths.home.canonicalize().map_err(|e| e.to_string())?);
    let workspace = workspace.canonicalize().map_err(|e| e.to_string())?;
    let context = ExecutionContext {
        schema: EXECUTION_SCHEMA.into(),
        run_id: run_id.to_string(),
        profile: profile.label().into(),
        state_root: crate::automation_runtime::state_root(&paths, profile),
        lease_root: paths.data_dir.join("automation-resources/v2"),
        definition_digest: digest(definition)?,
        resource_binding_digest: digest(&workspace)?,
        policy_digest: digest(policy)?,
        resources: vec![
            format!("workspace:{}", workspace.display()),
            format!("repository:{}", workspace.display()),
        ],
        runtime_seconds: runtime_secs,
        output_bytes,
    };
    context.validate()?;
    Ok(context)
}

fn digest(value: &impl Serialize) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|e| e.to_string())?)
    ))
}

pub(crate) struct LegacyRun {
    pub store: RunStore,
    _executor: ResourceLease,
    resources: Vec<ResourceLease>,
}

impl LegacyRun {
    pub(crate) fn open(context: ExecutionContext) -> Result<Self, String> {
        let store = RunStore::open(&context).map_err(|e| e.to_string())?;
        let executor = store.execution_lease().map_err(|e| e.to_string())?;
        let resources = if store.status().map_err(|e| e.to_string())?.state.terminal() {
            Vec::new()
        } else {
            store.acquire_resources().map_err(|e| e.to_string())?
        };
        Ok(Self {
            store,
            _executor: executor,
            resources,
        })
    }

    /// Cancellation drops observation and triggers the host worker guard; the
    /// inherited descriptors keep claims until that worker's descendants drain.
    pub(crate) async fn scope<F: Future>(&self, future: F) -> F::Output {
        #[cfg(unix)]
        let fds = self.resources.iter().map(ResourceLease::raw_fd).collect();
        #[cfg(not(unix))]
        let fds = Vec::new();
        #[cfg(unix)]
        let executor_fd = self._executor.raw_fd();
        #[cfg(not(unix))]
        let executor_fd = -1;
        RESOURCE_FDS
            .scope(
                fds,
                EXECUTOR_FD.scope(
                    executor_fd,
                    EXECUTION_STORE.scope(self.store.clone(), future),
                ),
            )
            .await
    }

    /// Return the confirmed action result, or durably mark a new attempt before
    /// its effects. Running/unknown effects are never retried by this adapter.
    pub(crate) fn begin_action(&self, id: &str) -> Result<Option<Value>, String> {
        let receipt = self.store.status().map_err(|e| e.to_string())?;
        if let Some(action) = receipt.actions.iter().find(|action| action.id == id) {
            match action.state {
                ActionState::Confirmed => {
                    return Ok(Some(action.result.clone().unwrap_or(Value::Null)));
                }
                ActionState::Running | ActionState::Unknown => {
                    return Err(format!(
                        "schedule action {id} requires effect reconciliation"
                    ));
                }
                ActionState::Prepared => {}
            }
        }
        if receipt.state.terminal() {
            return Err("terminal schedule run cannot start another action".into());
        }
        self.store.start().map_err(|e| e.to_string())?;
        self.store.prepare_action(id).map_err(|e| e.to_string())?;
        if !self.store.begin_action(id).map_err(|e| e.to_string())? {
            return Err("schedule action was concurrently confirmed".into());
        }
        Ok(None)
    }

    pub(crate) fn confirm_action(&self, id: &str, result: Value) -> Result<(), String> {
        self.store
            .confirm_action(id, result)
            .map_err(|e| e.to_string())
    }

    pub(crate) fn cached_result(&self) -> Result<Option<NativeResult>, String> {
        let receipt = self.store.status().map_err(|e| e.to_string())?;
        if receipt.state == ExecutionState::Succeeded {
            crate::automation_runtime::result_from_receipt(receipt).map(Some)
        } else {
            Ok(None)
        }
    }

    pub(crate) fn finish_success(&self, result: &NativeResult) -> Result<ExecutionReceipt, String> {
        let receipt = self.store.status().map_err(|e| e.to_string())?;
        if receipt.actions.iter().any(|action| {
            action.id != "target"
                && matches!(action.state, ActionState::Running | ActionState::Unknown)
        }) {
            return self.finish_failure(
                ExecutionState::NeedsAttention,
                "schedule has an unresolved action effect",
            );
        }
        let value = serde_json::to_value(result).map_err(|e| e.to_string())?;
        match receipt.actions.iter().find(|action| action.id == "target") {
            Some(action) if action.state == ActionState::Unknown => {
                return self.finish_failure(
                    ExecutionState::NeedsAttention,
                    "schedule target outcome requires reconciliation",
                );
            }
            Some(action) if action.state == ActionState::Running => {}
            Some(action) if action.state == ActionState::Confirmed => {
                let confirmed = action.result.as_ref().or(receipt.result.as_ref());
                if !confirmed
                    .is_some_and(|saved| *saved == value || saved.get("result") == Some(&value))
                {
                    return Err("confirmed schedule target result cannot be replaced".into());
                }
                return self
                    .store
                    .finish(ExecutionState::Succeeded, Some(value), None)
                    .map_err(|e| e.to_string());
            }
            _ => {
                self.store.start().map_err(|e| e.to_string())?;
                self.store
                    .prepare_action("target")
                    .map_err(|e| e.to_string())?;
                self.store
                    .begin_action("target")
                    .map_err(|e| e.to_string())?;
            }
        }
        self.confirm_action(
            "target",
            serde_json::json!({"execution_state":"succeeded", "result":value}),
        )?;
        self.store
            .finish(ExecutionState::Succeeded, Some(value), None)
            .map_err(|e| e.to_string())
    }

    pub(crate) fn finish_failure(
        &self,
        state: ExecutionState,
        reason: &str,
    ) -> Result<ExecutionReceipt, String> {
        if !matches!(
            state,
            ExecutionState::Failed | ExecutionState::Cancelled | ExecutionState::NeedsAttention
        ) {
            return Err("schedule failure outcome is invalid".into());
        }
        self.store
            .finish(state, None, Some(reason.into()))
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(root: &tempfile::TempDir, profile: ActiveProfile, id: Uuid) -> ExecutionContext {
        execution_context(
            &RuntimePaths::from_home(root.path()),
            &profile,
            id,
            &serde_json::json!({"version":1,"enabled":false,"target":"prompt"}),
            &serde_json::json!({"allow_network":false,"grants":[]}),
            root.path(),
            30,
            8192,
        )
        .unwrap()
    }

    #[test]
    fn sidecar_preserves_identity_and_confirmed_actions_without_repeating_effects() {
        let root = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let context = context(&root, ActiveProfile::Default, id);
        let run = LegacyRun::open(context.clone()).unwrap();
        assert!(run.begin_action("target").unwrap().is_none());
        assert!(run.begin_action("prompt").unwrap().is_none());
        let confirmed = serde_json::json!({"content":"finished"});
        run.confirm_action("prompt", confirmed.clone()).unwrap();
        assert_eq!(run.begin_action("prompt").unwrap(), Some(confirmed));
        let result = NativeResult {
            content: "finished".into(),
            usage: Default::default(),
        };
        run.finish_success(&result).unwrap();
        run.finish_success(&result).unwrap();
        assert!(run
            .finish_success(&NativeResult {
                content: "changed".into(),
                usage: Default::default(),
            })
            .is_err());
        drop(run);
        let resumed = LegacyRun::open(context).unwrap();
        assert_eq!(
            resumed.cached_result().unwrap().unwrap().content,
            "finished"
        );
        let mut changed = resumed.store.context().clone();
        changed.policy_digest = "d".repeat(64);
        assert!(RunStore::open(&changed).is_err());
    }

    #[test]
    fn profile_sidecars_share_workspace_ownership_and_ambiguous_effects_are_held() {
        let root = tempfile::tempdir().unwrap();
        let first =
            LegacyRun::open(context(&root, ActiveProfile::Default, Uuid::new_v4())).unwrap();
        let other = context(&root, ActiveProfile::Named("other".into()), Uuid::new_v4());
        assert!(LegacyRun::open(other.clone()).is_err());
        first.begin_action("effect").unwrap();
        assert!(first.begin_action("effect").is_err());
        let receipt = first
            .finish_success(&NativeResult {
                content: "unproven".into(),
                usage: Default::default(),
            })
            .unwrap();
        assert_eq!(receipt.state, ExecutionState::NeedsAttention);
        drop(first);
        assert!(LegacyRun::open(other).is_ok());
    }

    #[tokio::test]
    async fn scoped_sidecar_passes_run_and_owned_descriptors_to_worker_dispatch() {
        let root = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        let run = LegacyRun::open(context(&root, ActiveProfile::Default, id)).unwrap();
        run.scope(async {
            assert_eq!(
                ca_automation::runtime::current_run_store()
                    .unwrap()
                    .context()
                    .run_id,
                id.to_string()
            );
            #[cfg(unix)]
            {
                assert_eq!(current_resource_fds().unwrap().len(), 2);
                assert!(current_executor_fd().unwrap() >= 3);
            }
        })
        .await;
        assert!(current_resource_fds().is_none());
    }
}
