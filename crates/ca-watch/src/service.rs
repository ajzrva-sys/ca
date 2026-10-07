use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{broadcast, watch, Mutex, RwLock};

use crate::adapter::{self, AdapterState};
use crate::transport::{ProbeTransport, SystemProbeTransport};
use crate::types::{
    bounded_summary, ValidationError, WatchEventV1, WatchId, WatchRequestV1, WatchSnapshotV1,
    WatchState,
};

#[derive(Clone)]
pub struct WatchService {
    inner: Arc<ServiceInner>,
}

struct ServiceInner {
    transport: Arc<dyn ProbeTransport>,
    records: Mutex<HashMap<WatchId, Arc<WatchRecord>>>,
    events: broadcast::Sender<WatchEventV1>,
}

struct WatchRecord {
    request: WatchRequestV1,
    started_at: chrono::DateTime<Utc>,
    sequence: AtomicU64,
    failures: AtomicU32,
    finished: AtomicBool,
    event: RwLock<WatchEventV1>,
    cancel: watch::Sender<bool>,
}

impl Default for WatchService {
    fn default() -> Self {
        Self::new()
    }
}

impl WatchService {
    pub fn new() -> Self {
        Self::with_transport(Arc::new(SystemProbeTransport))
    }

    pub fn with_transport(transport: Arc<dyn ProbeTransport>) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(ServiceInner {
                transport,
                records: Mutex::new(HashMap::new()),
                events,
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WatchEventV1> {
        self.inner.events.subscribe()
    }

    pub async fn start(&self, request: WatchRequestV1) -> Result<WatchId, ValidationError> {
        request.validate()?;
        let id = WatchId::new();
        let started_at = Utc::now();
        let initial = WatchEventV1 {
            version: 1,
            watch_id: id,
            sequence: 1,
            observed_at: started_at,
            state: WatchState::Starting,
            summary: bounded_summary(&format!(
                "Watching {} PID {} on {}",
                request.process.kind_name(),
                request.pid,
                request.host.display_name()
            )),
        };
        let (cancel, cancel_rx) = watch::channel(false);
        let record = Arc::new(WatchRecord {
            request,
            started_at,
            sequence: AtomicU64::new(1),
            failures: AtomicU32::new(0),
            finished: AtomicBool::new(false),
            event: RwLock::new(initial.clone()),
            cancel,
        });
        self.inner.records.lock().await.insert(id, record.clone());
        let _ = self.inner.events.send(initial);
        let service = self.clone();
        tokio::spawn(async move {
            service.run_watch(record, cancel_rx).await;
        });
        Ok(id)
    }

    pub async fn stop(&self, watch_id: WatchId) -> bool {
        let record = self.inner.records.lock().await.get(&watch_id).cloned();
        let Some(record) = record else {
            return false;
        };
        if record.finished.swap(true, Ordering::AcqRel) {
            return false;
        }
        let _ = record.cancel.send(true);
        self.emit(
            &record,
            WatchState::Stopped,
            "Stopped watching; target process unaffected",
        )
        .await;
        true
    }

    pub async fn snapshot(&self) -> Vec<WatchSnapshotV1> {
        let records: Vec<Arc<WatchRecord>> =
            self.inner.records.lock().await.values().cloned().collect();
        let mut snapshots = Vec::with_capacity(records.len());
        for record in records {
            snapshots.push(snapshot_record(&record).await);
        }
        snapshots.sort_by_key(|snapshot| (snapshot.started_at, snapshot.watch_id));
        snapshots
    }

    pub async fn snapshot_watch(&self, watch_id: WatchId) -> Option<WatchSnapshotV1> {
        let record = self.inner.records.lock().await.get(&watch_id).cloned()?;
        Some(snapshot_record(&record).await)
    }

    pub async fn shutdown(&self) {
        let ids: Vec<WatchId> = self.inner.records.lock().await.keys().copied().collect();
        for id in ids {
            let _ = self.stop(id).await;
        }
    }

    async fn run_watch(&self, record: Arc<WatchRecord>, mut cancel_rx: watch::Receiver<bool>) {
        let mut adapter_state = AdapterState::default();
        loop {
            if record.finished.load(Ordering::Acquire) || *cancel_rx.borrow() {
                return;
            }

            let probe = self.inner.transport.probe(&record.request).await;
            if record.finished.load(Ordering::Acquire) || *cancel_rx.borrow() {
                return;
            }
            let delay = match probe {
                Ok(observation) => {
                    record.failures.store(0, Ordering::Release);
                    let adapter_event =
                        adapter::classify(&record.request, observation, &mut adapter_state);
                    let terminal = adapter_event.state.is_terminal();
                    if terminal && record.finished.swap(true, Ordering::AcqRel) {
                        return;
                    }
                    self.emit(&record, adapter_event.state, &adapter_event.summary)
                        .await;
                    if terminal {
                        return;
                    }
                    Duration::from_secs(record.request.interval_secs)
                }
                Err(_) => {
                    let failures = record.failures.fetch_add(1, Ordering::AcqRel) + 1;
                    let (state, summary) = if failures < 3 {
                        (
                            WatchState::Degraded,
                            format!("Probe unavailable; retrying ({failures}/3)"),
                        )
                    } else {
                        (
                            WatchState::Unreachable,
                            "Target remains unreachable; observation will continue".to_string(),
                        )
                    };
                    self.emit(&record, state, &summary).await;
                    retry_delay(record.request.interval_secs, failures)
                }
            };

            tokio::select! {
                changed = cancel_rx.changed() => {
                    if changed.is_err() || *cancel_rx.borrow() {
                        return;
                    }
                }
                () = tokio::time::sleep(delay) => {}
            }
        }
    }

    async fn emit(&self, record: &WatchRecord, state: WatchState, summary: &str) {
        let event = WatchEventV1 {
            version: 1,
            watch_id: record.event.read().await.watch_id,
            sequence: record.sequence.fetch_add(1, Ordering::AcqRel) + 1,
            observed_at: Utc::now(),
            state,
            summary: bounded_summary(summary),
        };
        *record.event.write().await = event.clone();
        let _ = self.inner.events.send(event);
    }
}

async fn snapshot_record(record: &WatchRecord) -> WatchSnapshotV1 {
    let event = record.event.read().await.clone();
    WatchSnapshotV1 {
        version: 1,
        watch_id: event.watch_id,
        request: record.request.clone(),
        started_at: record.started_at,
        event,
        consecutive_transport_failures: record.failures.load(Ordering::Acquire),
    }
}

fn retry_delay(interval_secs: u64, failures: u32) -> Duration {
    let exponent = failures.saturating_sub(1).min(5);
    let multiplier = 1_u64 << exponent;
    Duration::from_secs(interval_secs.saturating_mul(multiplier).min(60))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use super::*;
    use crate::{HostSpec, ProbeError, ProbeObservation, ProcessSpec};

    #[derive(Default)]
    struct FakeTransport {
        results: Mutex<VecDeque<Result<ProbeObservation, ProbeError>>>,
    }

    struct BlockingTransport {
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl ProbeTransport for BlockingTransport {
        async fn probe(&self, _request: &WatchRequestV1) -> Result<ProbeObservation, ProbeError> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(observation(Some("stable"), false, None))
        }
    }

    impl FakeTransport {
        fn new(results: Vec<Result<ProbeObservation, ProbeError>>) -> Self {
            Self {
                results: Mutex::new(results.into()),
            }
        }
    }

    #[async_trait]
    impl ProbeTransport for FakeTransport {
        async fn probe(&self, _request: &WatchRequestV1) -> Result<ProbeObservation, ProbeError> {
            self.results
                .lock()
                .await
                .pop_front()
                .unwrap_or(Err(ProbeError))
        }
    }

    fn request(process: ProcessSpec) -> WatchRequestV1 {
        WatchRequestV1 {
            label: "test".to_string(),
            host: HostSpec::Local,
            pid: 42,
            process,
            interval_secs: 2,
        }
    }

    fn observation(identity: Option<&str>, marker: bool, log: Option<&str>) -> ProbeObservation {
        ProbeObservation {
            marker_present: marker,
            identity: identity.map(str::to_string),
            log_line: log.map(str::to_string),
        }
    }

    async fn next(receiver: &mut broadcast::Receiver<WatchEventV1>) -> WatchEventV1 {
        tokio::time::timeout(Duration::from_secs(1), receiver.recv())
            .await
            .expect("event timeout")
            .expect("event channel")
    }

    #[tokio::test(start_paused = true)]
    async fn generic_orders_running_then_exit() {
        let transport = Arc::new(FakeTransport::new(vec![
            Ok(observation(Some("identity"), false, None)),
            Ok(observation(None, false, None)),
        ]));
        let service = WatchService::with_transport(transport);
        let mut events = service.subscribe();
        let id = service
            .start(request(ProcessSpec::Generic))
            .await
            .expect("start");
        assert_eq!(next(&mut events).await.state, WatchState::Starting);
        assert_eq!(next(&mut events).await.state, WatchState::Running);
        tokio::time::advance(Duration::from_secs(2)).await;
        let exited = next(&mut events).await;
        assert_eq!(exited.watch_id, id);
        assert_eq!(exited.state, WatchState::Exited);
        assert_eq!(exited.sequence, 3);
    }

    #[tokio::test(start_paused = true)]
    async fn copy_marker_wins_and_missing_marker_fails() {
        let process = ProcessSpec::Copy {
            log_path: Some("/tmp/copy.log".to_string()),
            completion_marker: "/tmp/done".to_string(),
        };
        let service = WatchService::with_transport(Arc::new(FakeTransport::new(vec![Ok(
            observation(None, true, None),
        )])));
        let mut events = service.subscribe();
        service
            .start(request(process.clone()))
            .await
            .expect("start");
        let _ = next(&mut events).await;
        assert_eq!(next(&mut events).await.state, WatchState::Completed);

        let service = WatchService::with_transport(Arc::new(FakeTransport::new(vec![Ok(
            observation(None, false, None),
        )])));
        let mut events = service.subscribe();
        service.start(request(process)).await.expect("start");
        let _ = next(&mut events).await;
        assert_eq!(next(&mut events).await.state, WatchState::Failed);
    }

    #[tokio::test(start_paused = true)]
    async fn identity_drift_fails() {
        let service = WatchService::with_transport(Arc::new(FakeTransport::new(vec![
            Ok(observation(Some("first"), false, None)),
            Ok(observation(Some("second"), false, None)),
        ])));
        let mut events = service.subscribe();
        service
            .start(request(ProcessSpec::Generic))
            .await
            .expect("start");
        let _ = next(&mut events).await;
        assert_eq!(next(&mut events).await.state, WatchState::Running);
        tokio::time::advance(Duration::from_secs(2)).await;
        assert_eq!(next(&mut events).await.state, WatchState::Failed);
    }

    #[tokio::test(start_paused = true)]
    async fn transport_degrades_becomes_unreachable_and_recovers() {
        let service = WatchService::with_transport(Arc::new(FakeTransport::new(vec![
            Err(ProbeError),
            Err(ProbeError),
            Err(ProbeError),
            Ok(observation(Some("same"), false, None)),
        ])));
        let mut events = service.subscribe();
        service
            .start(request(ProcessSpec::Generic))
            .await
            .expect("start");
        assert_eq!(next(&mut events).await.state, WatchState::Starting);
        assert_eq!(next(&mut events).await.state, WatchState::Degraded);
        tokio::time::advance(Duration::from_secs(2)).await;
        assert_eq!(next(&mut events).await.state, WatchState::Degraded);
        tokio::time::advance(Duration::from_secs(4)).await;
        assert_eq!(next(&mut events).await.state, WatchState::Unreachable);
        tokio::time::advance(Duration::from_secs(8)).await;
        assert_eq!(next(&mut events).await.state, WatchState::Running);
    }

    #[tokio::test(start_paused = true)]
    async fn stop_emits_terminal_event_without_a_probe_side_effect() {
        let service = WatchService::with_transport(Arc::new(FakeTransport::new(vec![Ok(
            observation(Some("same"), false, None),
        )])));
        let mut events = service.subscribe();
        let id = service
            .start(request(ProcessSpec::Generic))
            .await
            .expect("start");
        let _ = next(&mut events).await;
        let _ = next(&mut events).await;
        assert!(service.stop(id).await);
        let stopped = next(&mut events).await;
        assert_eq!(stopped.state, WatchState::Stopped);
        assert!(stopped.summary.contains("unaffected"));
        assert!(!service.stop(id).await);
    }

    #[tokio::test]
    async fn stopping_during_a_probe_never_emits_after_the_terminal_event() {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let service = WatchService::with_transport(Arc::new(BlockingTransport {
            started: started.clone(),
            release: release.clone(),
        }));
        let mut events = service.subscribe();
        let id = service
            .start(request(ProcessSpec::Generic))
            .await
            .expect("start");
        assert_eq!(next(&mut events).await.state, WatchState::Starting);
        started.notified().await;
        assert!(service.stop(id).await);
        assert_eq!(next(&mut events).await.state, WatchState::Stopped);
        release.notify_waiters();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), events.recv())
                .await
                .is_err()
        );
    }

    #[test]
    fn retry_backoff_is_capped() {
        assert_eq!(retry_delay(2, 1), Duration::from_secs(2));
        assert_eq!(retry_delay(2, 4), Duration::from_secs(16));
        assert_eq!(retry_delay(300, 20), Duration::from_secs(60));
    }

    #[tokio::test]
    async fn real_local_probe_observes_the_current_process_without_signaling_it() {
        let service = WatchService::new();
        let mut events = service.subscribe();
        let id = service
            .start(WatchRequestV1 {
                label: "test process".to_string(),
                host: HostSpec::Local,
                pid: std::process::id(),
                process: ProcessSpec::Generic,
                interval_secs: 2,
            })
            .await
            .expect("start");
        assert_eq!(next(&mut events).await.state, WatchState::Starting);
        assert_eq!(next(&mut events).await.state, WatchState::Running);
        assert!(service.stop(id).await);
        assert_eq!(next(&mut events).await.state, WatchState::Stopped);
    }

    #[tokio::test]
    async fn real_local_copy_checks_the_marker_before_the_pid() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let marker = directory.path().join("complete");
        std::fs::write(&marker, b"done").expect("write marker");
        let service = WatchService::new();
        let mut events = service.subscribe();
        service
            .start(WatchRequestV1 {
                label: "copy".to_string(),
                host: HostSpec::Local,
                pid: u32::MAX,
                process: ProcessSpec::Copy {
                    log_path: None,
                    completion_marker: marker.to_string_lossy().into_owned(),
                },
                interval_secs: 2,
            })
            .await
            .expect("start");
        let _ = next(&mut events).await;
        assert_eq!(next(&mut events).await.state, WatchState::Completed);
    }
}
