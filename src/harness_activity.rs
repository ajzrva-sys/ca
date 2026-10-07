//! One durable active-time reservation spans a whole host-owned operation,
//! including model calls, tools, compilation and verification. Cancellation
//! leaves the reservation in place; a new process cannot reset its allowance.
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{update_harness_cycle, HarnessPlan, LearningVault};
use std::future::Future;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::time::Instant;

pub struct HarnessActivity {
    vault: LearningVault,
    plan: HarnessPlan,
    id: String,
    maximum_seconds: u64,
    started: Instant,
    deadline: Instant,
    // 0 = reserved, 1 = running, 2 = closed (including cancellation).
    state: AtomicU8,
}

impl HarnessActivity {
    pub async fn begin(
        vault: LearningVault,
        plan: HarnessPlan,
        maximum_seconds: u64,
    ) -> Result<Arc<Self>> {
        Self::begin_kind(vault, plan, maximum_seconds, false).await
    }

    pub async fn begin_transition(
        vault: LearningVault,
        plan: HarnessPlan,
        maximum_seconds: u64,
    ) -> Result<Arc<Self>> {
        Self::begin_kind(vault, plan, maximum_seconds, true).await
    }

    async fn begin_kind(
        vault: LearningVault,
        plan: HarnessPlan,
        maximum_seconds: u64,
        transition: bool,
    ) -> Result<Arc<Self>> {
        let id = uuid::Uuid::new_v4().to_string();
        let (store, frozen, work_id) = (vault.clone(), plan.clone(), id.clone());
        tokio::task::spawn_blocking(move || {
            update_harness_cycle(&store, &frozen.id, |state| {
                if transition {
                    state.reserve_transition_time(&frozen, &work_id, maximum_seconds)
                } else {
                    state.reserve_active_time(&frozen, &work_id, maximum_seconds)
                }
            })
        })
        .await??;
        let started = Instant::now();
        let deadline = started
            .checked_add(Duration::from_secs(maximum_seconds))
            .context("activity deadline overflow")?;
        Ok(Arc::new(Self {
            vault,
            plan,
            id,
            maximum_seconds,
            started,
            deadline,
            state: AtomicU8::new(0),
        }))
    }

    pub fn matches(&self, vault: &LearningVault, plan: &HarnessPlan) -> bool {
        self.vault.root() == vault.root()
            && self.plan.id == plan.id
            && self.plan.digest == plan.digest
    }

    pub fn reservation(&self) -> (&str, u64) {
        (&self.id, self.maximum_seconds)
    }

    pub fn remaining(&self) -> Result<Duration> {
        if self.state.load(Ordering::SeqCst) != 1 {
            bail!("activity is not running");
        }
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|v| !v.is_zero())
            .context("activity deadline exhausted")
    }

    /// Poll only host-controlled asynchronous work here. Synchronous subprocess
    /// work must have its own kill-and-wait owner; dropping a blocking task does
    /// not stop its process and is not an acceptable implementation of this API.
    pub async fn run<T>(&self, operation: impl Future<Output = Result<T>>) -> Result<T> {
        self.state
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| anyhow::anyhow!("activity cannot be run twice"))?;
        struct Close<'a>(&'a AtomicU8);
        impl Drop for Close<'_> {
            fn drop(&mut self) {
                self.0.store(2, Ordering::SeqCst);
            }
        }
        let guard = Close(&self.state);
        let result = tokio::time::timeout_at(self.deadline, operation).await;
        // Stop admission before writing the settlement. A retained backend
        // cannot issue new calls against a completed activity.
        drop(guard);
        let elapsed = if result.is_err() {
            self.maximum_seconds
        } else {
            let duration = self.started.elapsed();
            duration
                .as_secs()
                .saturating_add(u64::from(duration.subsec_nanos() != 0))
                .min(self.maximum_seconds)
        };
        let (vault, plan, id) = (self.vault.clone(), self.plan.clone(), self.id.clone());
        tokio::task::spawn_blocking(move || {
            update_harness_cycle(&vault, &plan.id, |state| {
                state.settle_active_time(&plan, &id, elapsed)
            })
        })
        .await??;
        result.context("harness activity deadline exhausted")?
    }
}
