//! One model-free observation with durable, quiet heartbeat state.
use crate::automation::AutomationUsageCountersV1;
use crate::automation_runtime::NativeResult;
use ca_automation::runtime::{ExecutionContext, ResourceLease};
use ca_watch::{WatchEventV1, WatchId, WatchRequestV1, WatchService, WatchState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MonitorBaseline {
    fingerprint: String,
    state: WatchState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MonitorOccurrenceReceipt {
    request_sha256: String,
    result: NativeResult,
    baseline: MonitorBaseline,
}

pub async fn run(request: WatchRequestV1, ctx: &ExecutionContext) -> Result<NativeResult, String> {
    request.validate().map_err(|error| error.to_string())?;
    ctx.validate()?;
    let started = std::time::Instant::now();
    let _owner = ResourceLease::acquire(
        &ctx.lease_root,
        &format!("monitor-baseline:{}:{}", ctx.profile, ctx.definition_digest),
    )
    .map_err(|error| error.to_string())?;
    let baseline_path = ctx
        .state_root
        .join("monitor-baselines")
        .join(format!("{}.json", ctx.definition_digest));
    let receipt_path = ctx
        .state_root
        .join("runs")
        .join(&ctx.run_id)
        .join("monitor-result.json");
    let request_sha256 = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&request).map_err(|error| error.to_string())?)
    );
    if receipt_path.exists() {
        let receipt: MonitorOccurrenceReceipt =
            ca_automation::runtime::read(&receipt_path, ctx.output_bytes.saturating_add(4096))
                .map_err(|error| error.to_string())?;
        if receipt.request_sha256 != request_sha256 {
            return Err("Monitor occurrence is bound to another request".into());
        }
        if !baseline_path.exists() {
            ca_automation::runtime::atomic_write(&baseline_path, &receipt.baseline)
                .map_err(|error| error.to_string())?;
        }
        return Ok(receipt.result);
    }
    let previous = if baseline_path.exists() {
        Some(
            ca_automation::runtime::read::<MonitorBaseline>(&baseline_path, 4096)
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    let service = WatchService::new();
    let event = observe(&service, request.clone(), ctx.runtime_seconds).await?;
    let baseline = stable_baseline(&request, &event)?;
    let status = report_status(previous.as_ref(), &baseline);
    let content = serde_json::to_string(&serde_json::json!({
        "schema":"ca.heartbeat-report.v1", "status":status,
        "summary":format!("{}: {}", request.label, event.summary),
        "state":format!("{}:{}", event.state, baseline.fingerprint),
    }))
    .map_err(|error| error.to_string())?;
    if content.len() as u64 > ctx.output_bytes {
        return Err("Monitor result exceeds its admitted output budget".into());
    }
    // The run-specific receipt precedes the shared baseline. Re-entry reads the
    // same report instead of changing a previously undelivered event to quiet.
    let result = NativeResult {
        usage: AutomationUsageCountersV1 {
            tool_calls: 1,
            runtime_secs: started.elapsed().as_secs().saturating_add(1),
            output_bytes: content.len() as u64,
            ..Default::default()
        },
        content,
    };
    ca_automation::runtime::atomic_write(
        &receipt_path,
        &MonitorOccurrenceReceipt {
            request_sha256,
            result: result.clone(),
            baseline: baseline.clone(),
        },
    )
    .map_err(|error| error.to_string())?;
    ca_automation::runtime::atomic_write(&baseline_path, &baseline)
        .map_err(|error| error.to_string())?;
    Ok(result)
}

/// Subscribe before starting, then stop only the observer after its first
/// classified sample. No cancellation path signals the observed PID.
async fn observe(
    service: &WatchService,
    request: WatchRequestV1,
    runtime_seconds: u64,
) -> Result<WatchEventV1, String> {
    let mut events = service.subscribe();
    let id = service
        .start(request)
        .await
        .map_err(|error| error.to_string())?;
    let mut guard = ObserverGuard {
        service: service.clone(),
        id: Some(id),
    };
    let wait = async {
        loop {
            match events.recv().await {
                Ok(event) if event.watch_id == id && event.state != WatchState::Starting => {
                    return Ok(event)
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => return Err("Monitor observation stream closed".to_string()),
            }
        }
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(runtime_seconds.saturating_sub(1).clamp(1, 30)),
        wait,
    )
    .await;
    service.stop(id).await;
    guard.id = None;
    match result {
        Ok(result) => result,
        Err(_) => Err(
            "Monitor probe did not complete within its admitted runtime; target process unaffected"
                .into(),
        ),
    }
}

struct ObserverGuard {
    service: WatchService,
    id: Option<WatchId>,
}
impl Drop for ObserverGuard {
    fn drop(&mut self) {
        let Some(id) = self.id.take() else {
            return;
        };
        let service = self.service.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                service.stop(id).await;
            });
        }
    }
}

fn stable_baseline(
    request: &WatchRequestV1,
    event: &WatchEventV1,
) -> Result<MonitorBaseline, String> {
    let bytes = serde_json::to_vec(&(request, event.state, &event.summary))
        .map_err(|error| error.to_string())?;
    Ok(MonitorBaseline {
        fingerprint: format!("{:x}", Sha256::digest(bytes)),
        state: event.state,
    })
}

fn report_status(previous: Option<&MonitorBaseline>, current: &MonitorBaseline) -> &'static str {
    if previous.is_some_and(|previous| previous.fingerprint == current.fingerprint)
        || (previous.is_none() && current.state == WatchState::Running)
    {
        return "unchanged";
    }
    match current.state {
        WatchState::Completed | WatchState::Exited => "completed",
        WatchState::Failed => "failed",
        WatchState::Degraded
        | WatchState::Unreachable
        | WatchState::Starting
        | WatchState::Stopped => "needs_attention",
        WatchState::Running => "changed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ca_watch::{HostSpec, ProbeError, ProbeObservation, ProbeTransport, ProcessSpec};
    use std::sync::Arc;

    struct FixtureProbe {
        wait_forever: bool,
    }
    #[async_trait::async_trait]
    impl ProbeTransport for FixtureProbe {
        async fn probe(&self, _: &WatchRequestV1) -> Result<ProbeObservation, ProbeError> {
            if self.wait_forever {
                std::future::pending::<()>().await;
            }
            Ok(ProbeObservation {
                marker_present: false,
                identity: Some("fixture-process-identity".into()),
                log_line: None,
            })
        }
    }

    fn request(pid: u32) -> WatchRequestV1 {
        WatchRequestV1 {
            label: "fixture".into(),
            host: HostSpec::Local,
            pid,
            process: ProcessSpec::Generic,
            interval_secs: 2,
        }
    }

    #[tokio::test]
    async fn observing_and_cancelling_never_stop_the_observed_process() {
        let mut process = tokio::process::Command::new("/bin/sleep")
            .arg("10")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let request = request(process.id().unwrap());
        let service = WatchService::with_transport(Arc::new(FixtureProbe {
            wait_forever: false,
        }));
        let event = observe(&service, request.clone(), 5).await.unwrap();
        assert_eq!(event.state, WatchState::Running);
        assert_eq!(service.snapshot().await[0].event.state, WatchState::Stopped);
        assert!(process.try_wait().unwrap().is_none());
        let service = WatchService::with_transport(Arc::new(FixtureProbe { wait_forever: true }));
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(30),
            observe(&service, request, 5),
        )
        .await;
        assert!(result.is_err());
        tokio::task::yield_now().await;
        assert_eq!(service.snapshot().await[0].event.state, WatchState::Stopped);
        assert!(process.try_wait().unwrap().is_none());
        service.shutdown().await;
        process.kill().await.unwrap();
        process.wait().await.unwrap();
    }

    #[tokio::test]
    async fn changing_observer_ids_and_timestamps_does_not_change_monitor_state() {
        let service = WatchService::with_transport(Arc::new(FixtureProbe {
            wait_forever: false,
        }));
        let request = request(123);
        let first = observe(&service, request.clone(), 5).await.unwrap();
        let second = observe(&service, request.clone(), 5).await.unwrap();
        assert_ne!(first.watch_id, second.watch_id);
        let first = stable_baseline(&request, &first).unwrap();
        let second = stable_baseline(&request, &second).unwrap();
        assert_eq!(first.fingerprint, second.fingerprint);
        assert_eq!(report_status(None, &first), "unchanged");
        assert_eq!(report_status(Some(&first), &second), "unchanged");
        let completed = MonitorBaseline {
            fingerprint: "complete".into(),
            state: WatchState::Completed,
        };
        assert_eq!(report_status(Some(&first), &completed), "completed");
        assert_eq!(report_status(Some(&completed), &completed), "unchanged");
    }

    #[tokio::test]
    async fn repeated_occurrence_returns_its_original_report_without_a_new_probe() {
        let temporary = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temporary.path()).unwrap();
        let ctx = ExecutionContext {
            schema: ca_automation::runtime::EXECUTION_SCHEMA.into(),
            run_id: "monitor-fixture".into(),
            profile: "default".into(),
            state_root: root.join("state"),
            lease_root: root.join("leases"),
            definition_digest: "a".repeat(64),
            resource_binding_digest: "b".repeat(64),
            policy_digest: "c".repeat(64),
            resources: Vec::new(),
            runtime_seconds: 5,
            output_bytes: 4096,
        };
        let request = request(123);
        let content = "retained original completion report".to_string();
        let receipt = MonitorOccurrenceReceipt {
            request_sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&request).unwrap())
            ),
            result: NativeResult {
                usage: AutomationUsageCountersV1 {
                    tool_calls: 1,
                    output_bytes: content.len() as u64,
                    ..Default::default()
                },
                content: content.clone(),
            },
            baseline: MonitorBaseline {
                fingerprint: "c".repeat(64),
                state: WatchState::Completed,
            },
        };
        ca_automation::runtime::atomic_write(
            &ctx.state_root
                .join("runs")
                .join(&ctx.run_id)
                .join("monitor-result.json"),
            &receipt,
        )
        .unwrap();
        assert_eq!(run(request.clone(), &ctx).await.unwrap().content, content);
        assert_eq!(
            run(request, &ctx).await.unwrap().usage.output_bytes,
            content.len() as u64
        );
        assert!(run(super::tests::request(124), &ctx)
            .await
            .unwrap_err()
            .contains("another request"));
    }
}
