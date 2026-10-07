use super::{MechanisticActionV1, MechanisticReceiptStatusV1};
use crate::paths::Paths;
use anyhow::{bail, Result};
use ca_watch::{HostSpec, ProcessSpec, WatchId, WatchRequestV1, WatchService, WatchState};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservationSnapshotV1 {
    pub(crate) watch_id: String,
    pub(crate) status: MechanisticReceiptStatusV1,
    pub(crate) state: String,
    pub(crate) summary: String,
    pub(crate) sequence: u64,
    pub(crate) terminal: bool,
}

#[derive(Clone)]
pub(crate) struct LocalObservation {
    service: WatchService,
    watch_id: WatchId,
}

impl std::fmt::Debug for LocalObservation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalObservation")
            .field("watch_id", &self.watch_id.to_string())
            .finish_non_exhaustive()
    }
}

impl LocalObservation {
    pub(crate) async fn start(repo: &Path, action: &MechanisticActionV1) -> Result<Self> {
        let paths = Paths::new(repo)?;
        let request = match action {
            MechanisticActionV1::ObserveProcess {
                label,
                pid,
                interval_seconds,
            } => WatchRequestV1 {
                label: label.clone(),
                host: HostSpec::Local,
                pid: *pid,
                process: ProcessSpec::Generic,
                interval_secs: *interval_seconds,
            },
            MechanisticActionV1::ObserveCopy {
                label,
                pid,
                log_path,
                completion_marker,
                interval_seconds,
            } => WatchRequestV1 {
                label: label.clone(),
                host: HostSpec::Local,
                pid: *pid,
                process: ProcessSpec::Copy {
                    log_path: log_path
                        .as_deref()
                        .map(|value| contained_observation_path(&paths.root, value))
                        .transpose()?
                        .map(path_text),
                    completion_marker: path_text(contained_observation_path(
                        &paths.root,
                        completion_marker,
                    )?),
                },
                interval_secs: *interval_seconds,
            },
            _ => bail!("local observation requires one observation action"),
        };
        let service = WatchService::new();
        let watch_id = service.start(request).await?;
        Ok(Self { service, watch_id })
    }

    pub(crate) async fn snapshot(&self) -> Result<ObservationSnapshotV1> {
        let snapshot = self
            .service
            .snapshot_watch(self.watch_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("local observation record is missing"))?;
        let summary = crate::self_improvement::sanitize_trace_text(&snapshot.event.summary)
            .trim_end()
            .chars()
            .take(4_096)
            .collect();
        Ok(ObservationSnapshotV1 {
            watch_id: self.watch_id.to_string(),
            status: match snapshot.event.state {
                WatchState::Completed | WatchState::Exited => MechanisticReceiptStatusV1::Completed,
                WatchState::Failed => MechanisticReceiptStatusV1::Failed,
                WatchState::Stopped => MechanisticReceiptStatusV1::Cancelled,
                _ => MechanisticReceiptStatusV1::Completed,
            },
            state: snapshot.event.state.to_string(),
            summary,
            sequence: snapshot.event.sequence,
            terminal: snapshot.event.state.is_terminal(),
        })
    }

    pub(crate) async fn cancel(&self) -> bool {
        self.service.stop(self.watch_id).await
    }
}

fn contained_observation_path(repo: &Path, value: &str) -> Result<PathBuf> {
    if value.is_empty() || value.len() > 4096 || value.contains('\\') || value.starts_with('/') {
        bail!("observation paths must be repository-relative");
    }
    let relative = Path::new(value);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("observation path traversal is not allowed");
    }
    let canonical_repo = fs::canonicalize(repo)?;
    let candidate = canonical_repo.join(relative);
    let mut existing = candidate.as_path();
    while !existing.exists() {
        existing = existing
            .parent()
            .ok_or_else(|| anyhow::anyhow!("observation path has no contained parent"))?;
    }
    let canonical_existing = fs::canonicalize(existing)?;
    if !canonical_existing.starts_with(&canonical_repo) {
        bail!("observation path escapes the pinned repository");
    }
    let mut cursor = canonical_repo.clone();
    for component in relative.components() {
        cursor.push(component.as_os_str());
        if let Ok(metadata) = fs::symlink_metadata(&cursor) {
            if metadata.file_type().is_symlink() {
                bail!("observation paths may not traverse symlinks");
            }
        }
    }
    Ok(candidate)
}

fn path_text(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn cancelling_observation_does_not_signal_the_observed_process() -> Result<()> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!("codex-team-observe-{nonce}"));
        fs::create_dir_all(&root)?;
        let action = MechanisticActionV1::ObserveProcess {
            label: "self".to_string(),
            pid: std::process::id(),
            interval_seconds: 2,
        };
        let observation = LocalObservation::start(&root, &action).await?;
        assert!(observation.cancel().await);
        assert_eq!(observation.snapshot().await?.state, "stopped");
        assert!(Path::new("/proc/self").exists() || std::process::id() > 0);
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
