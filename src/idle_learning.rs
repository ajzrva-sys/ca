//! Read-only learning admission checks, never performed by the input reducer.
use std::time::{Duration, Instant};

pub(crate) struct Schedule {
    next: Instant,
    task: Option<tokio::task::JoinHandle<Result<Option<crate::learning::MaintenanceKind>, String>>>,
    config_signature: String,
}

impl Schedule {
    pub(crate) fn new() -> Self {
        Self {
            next: Instant::now(),
            task: None,
            config_signature: String::new(),
        }
    }

    pub(crate) fn poll(
        &mut self,
        store: &crate::learning::LearningStore,
        config: &crate::learning::LearningConfig,
    ) -> Option<Result<Option<crate::learning::MaintenanceKind>, String>> {
        use futures::FutureExt;
        if self.task.as_ref().is_some_and(|task| task.is_finished()) {
            let result = self
                .task
                .take()
                .unwrap()
                .now_or_never()
                .unwrap()
                .unwrap_or_else(|_| Err("learning schedule worker stopped".into()));
            if self.config_signature == format!("{config:?}") {
                return Some(result);
            }
            self.next = Instant::now();
        }
        if self.task.is_none() && Instant::now() >= self.next {
            self.next = Instant::now() + Duration::from_secs(1);
            let store = store.clone();
            let config = config.clone();
            self.config_signature = format!("{config:?}");
            self.task = Some(tokio::task::spawn_blocking(move || {
                for kind in crate::learning::MaintenanceKind::ALL {
                    if store.due(kind, &config, chrono::Utc::now())? {
                        return Ok(Some(kind));
                    }
                }
                Ok(None)
            }));
        }
        None
    }
}

impl Drop for Schedule {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bug257_schedule_reads_off_loop_and_throttles_unchanged_config() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(temporary.path());
        let store = crate::learning::LearningStore::for_profile(
            &paths,
            &crate::runtime::ActiveProfile::Default,
            true,
        )
        .unwrap();
        let config = crate::learning::LearningConfig::default();
        let mut schedule = Schedule::new();
        assert!(schedule.poll(&store, &config).is_none());
        while !schedule.task.as_ref().unwrap().is_finished() {
            tokio::task::yield_now().await;
        }
        assert!(matches!(
            schedule.poll(&store, &config),
            Some(Ok(Some(crate::learning::MaintenanceKind::Dream)))
        ));
        for _ in 0..100 {
            assert!(schedule.poll(&store, &config).is_none());
            assert!(schedule.task.is_none());
        }
        assert!(!paths
            .learning_state_file(&crate::runtime::ActiveProfile::Default)
            .exists());
    }
}
