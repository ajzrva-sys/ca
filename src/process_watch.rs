use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use ca_watch::{WatchEventV1, WatchId, WatchRequestV1, WatchService, WatchSnapshotV1};
use tokio::sync::{broadcast, Mutex};

use crate::jobs::JobOwner;

pub const WATCH_PROCESS_TOOL_NAME: &str = "watch_process";
pub const STOP_WATCHER_TOOL_NAME: &str = "stop_watcher";

#[derive(Clone)]
pub struct ProcessWatchManager {
    inner: Arc<Inner>,
}

struct Inner {
    service: WatchService,
    owners: Mutex<HashMap<WatchId, JobOwner>>,
    last_sequences: Mutex<HashMap<WatchId, u64>>,
    events: broadcast::Sender<WatchEventV1>,
}

static PROCESS_WATCH_MANAGER: LazyLock<ProcessWatchManager> =
    LazyLock::new(ProcessWatchManager::new);

impl ProcessWatchManager {
    pub fn new() -> Self {
        Self::with_service(WatchService::new())
    }

    pub fn with_service(service: WatchService) -> Self {
        let mut service_events = service.subscribe();
        let (events, _) = broadcast::channel(256);
        let inner = Arc::new(Inner {
            service,
            owners: Mutex::new(HashMap::new()),
            last_sequences: Mutex::new(HashMap::new()),
            events,
        });
        let forward = Arc::downgrade(&inner);
        tokio::spawn(async move {
            loop {
                match service_events.recv().await {
                    Ok(event) => {
                        let Some(inner) = forward.upgrade() else {
                            break;
                        };
                        inner.publish_if_owned(event).await;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        Self { inner }
    }

    pub fn process() -> Self {
        PROCESS_WATCH_MANAGER.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WatchEventV1> {
        self.inner.events.subscribe()
    }

    pub async fn start_owned(
        &self,
        request: WatchRequestV1,
        owner: JobOwner,
    ) -> Result<WatchId, String> {
        let id = self
            .inner
            .service
            .start(request)
            .await
            .map_err(|error| error.to_string())?;
        self.inner.owners.lock().await.insert(id, owner);
        if let Some(snapshot) = self.inner.service.snapshot_watch(id).await {
            self.inner.publish_if_owned(snapshot.event).await;
        }
        Ok(id)
    }

    pub async fn stop_owned(&self, id: WatchId, owner: JobOwner) -> Result<(), String> {
        if !self.is_owned(id, owner).await {
            return Err(format!("Unknown watcher: {id}"));
        }
        if self.inner.service.stop(id).await {
            Ok(())
        } else {
            Err(format!("Watcher {id} is already stopped"))
        }
    }

    pub async fn is_owned(&self, id: WatchId, owner: JobOwner) -> bool {
        self.inner.owners.lock().await.get(&id) == Some(&owner)
    }

    pub async fn list_owned(&self, owner: JobOwner) -> Vec<WatchSnapshotV1> {
        let owners = self.inner.owners.lock().await.clone();
        self.inner
            .service
            .snapshot()
            .await
            .into_iter()
            .filter(|snapshot| owners.get(&snapshot.watch_id) == Some(&owner))
            .collect()
    }

    pub async fn shutdown_owner(&self, owner: JobOwner) {
        let ids: Vec<WatchId> = self
            .inner
            .owners
            .lock()
            .await
            .iter()
            .filter_map(|(id, candidate)| (*candidate == owner).then_some(*id))
            .collect();
        for id in ids {
            if self
                .inner
                .service
                .snapshot_watch(id)
                .await
                .is_some_and(|snapshot| !snapshot.event.state.is_terminal())
            {
                let _ = self.inner.service.stop(id).await;
            }
        }
    }

    pub async fn shutdown(&self) {
        self.inner.service.shutdown().await;
    }
}

impl Inner {
    async fn publish_if_owned(&self, event: WatchEventV1) {
        if !self.owners.lock().await.contains_key(&event.watch_id) {
            return;
        }
        let mut last_sequences = self.last_sequences.lock().await;
        let last = last_sequences.entry(event.watch_id).or_default();
        if event.sequence <= *last {
            return;
        }
        *last = event.sequence;
        drop(last_sequences);
        let _ = self.events.send(event);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use ca_watch::{
        HostSpec, ProbeError, ProbeObservation, ProbeTransport, ProcessSpec, WatchRequestV1,
    };

    use super::*;

    struct Alive;

    #[async_trait]
    impl ProbeTransport for Alive {
        async fn probe(&self, _request: &WatchRequestV1) -> Result<ProbeObservation, ProbeError> {
            Ok(ProbeObservation {
                marker_present: false,
                identity: Some("stable".to_string()),
                log_line: None,
            })
        }
    }

    fn request() -> WatchRequestV1 {
        WatchRequestV1 {
            label: "owned".to_string(),
            host: HostSpec::Local,
            pid: 42,
            process: ProcessSpec::Generic,
            interval_secs: 2,
        }
    }

    #[tokio::test]
    async fn watcher_ids_are_stable_and_owner_scoped() {
        let manager =
            ProcessWatchManager::with_service(WatchService::with_transport(Arc::new(Alive)));
        let owner = JobOwner::new(None);
        let other = JobOwner::new(None);
        let mut events = manager.subscribe();
        let id = manager.start_owned(request(), owner).await.expect("start");
        let announced = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
            .await
            .expect("owner event timeout")
            .expect("owner event");
        assert_eq!(announced.watch_id, id);
        let snapshots = manager.list_owned(owner).await;
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].watch_id, id);
        assert!(manager.list_owned(other).await.is_empty());
        assert!(manager.stop_owned(id, other).await.is_err());
        manager.stop_owned(id, owner).await.expect("owner stops");
    }
}
