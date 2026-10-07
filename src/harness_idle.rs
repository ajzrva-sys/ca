//! Cooperative idle execution. Foreground activity is observed only at settled
//! operation boundaries; unfinished requests keep their durable reservations.
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{
    load_harness_cycle, update_harness_cycle, HarnessPauseCause, HarnessStage, LearningVault,
};
use std::future::Future;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
#[cfg(test)]
use std::time::{Duration, Instant};

// Blocking preparation cannot be cancelled by aborting its async JoinHandle.
// Carry the same foreground signal onto that worker and check it between bounded
// filesystem operations. The guard restores nested scopes, including on unwind.
thread_local! {
    static PREPARATION: std::cell::RefCell<Option<Arc<AtomicBool>>> = const {
        std::cell::RefCell::new(None)
    };
}

fn preparation_scope<T>(foreground: Arc<AtomicBool>, operation: impl FnOnce() -> T) -> T {
    struct Restore(Option<Arc<AtomicBool>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            PREPARATION.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(PREPARATION.with(|slot| slot.replace(Some(foreground))));
    operation()
}

pub(crate) fn preparation_boundary() -> Result<()> {
    if PREPARATION.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    }) {
        bail!("automatic experiment preparation yielded to foreground work");
    }
    Ok(())
}

struct ContextGate {
    vault: PathBuf,
    cycle: String,
    foreground: Arc<AtomicBool>,
}
tokio::task_local! { static GATE: ContextGate; }

pub(crate) async fn scope<T>(
    vault: &LearningVault,
    id: &str,
    foreground: Arc<AtomicBool>,
    operation: impl Future<Output = T>,
) -> T {
    GATE.scope(
        ContextGate {
            vault: vault.root().into(),
            cycle: id.into(),
            foreground,
        },
        operation,
    )
    .await
}

/// No effect on explicit runs, other vaults, or other cycles. This must be called
/// before reserving a new operation, never halfway through a trial or commit.
pub(crate) async fn boundary(vault: &LearningVault, id: &str) -> Result<()> {
    let foreground = GATE
        .try_with(|gate| {
            gate.vault == vault.root()
                && gate.cycle == id
                && gate.foreground.load(Ordering::Acquire)
        })
        .unwrap_or(false);
    if !foreground {
        return Ok(());
    }
    let (store, cycle) = (vault.clone(), id.to_owned());
    tokio::task::spawn_blocking(move || pause_for_foreground(&store, &cycle)).await??;
    bail!("automatic experiment yielded to foreground work")
}

pub(crate) fn pause_for_foreground(vault: &LearningVault, id: &str) -> Result<()> {
    update_harness_cycle(vault, id, |state| {
        if state.stage == HarnessStage::Paused {
            return Ok(());
        }
        if !state.active_reservations.is_empty() || !state.reservations.is_empty() {
            bail!("foreground pause requires a settled operation boundary");
        }
        if matches!(
            state.stage,
            HarnessStage::EvaluationFrozen
                | HarnessStage::CandidateReady
                | HarnessStage::Evaluating
                | HarnessStage::ReadyToPromote
        ) {
            state.resume_stage = Some(state.stage.clone());
            state.stage = HarnessStage::Paused;
            state.pause_cause = Some(HarnessPauseCause::Foreground);
            state.reason = "Automatic experiment paused for foreground work".into();
        }
        Ok(())
    })
}

pub(crate) fn resume_foreground_pause(vault: &LearningVault, id: &str) -> Result<bool> {
    let (plan, _) = load_harness_cycle(vault, id)?;
    update_harness_cycle(vault, id, |state| {
        if state.stage != HarnessStage::Paused
            || state.pause_cause != Some(HarnessPauseCause::Foreground)
        {
            return Ok(false);
        }
        if !state.active_reservations.is_empty()
            || !state.reservations.is_empty()
            || state.active_seconds >= plan.settings.maximum_active_seconds
            || state.settled_microusd >= plan.settings.maximum_microusd
        {
            return Ok(false);
        }
        state.stage = state
            .resume_stage
            .take()
            .context("missing automatic resume stage")?;
        state.pause_cause = None;
        state.reason = "Automatic experiment resumed after foreground inactivity".into();
        Ok(true)
    })
}

/// UI-owned monotonic inactivity clock. Wall-clock changes cannot shorten the
/// five-minute interval. A busy state or any user input starts it again.
#[cfg(test)]
pub(crate) struct IdleClock {
    last_activity: Instant,
    pub foreground: Arc<AtomicBool>,
}
#[cfg(test)]
impl IdleClock {
    pub fn new(now: Instant) -> Self {
        Self {
            last_activity: now,
            foreground: Arc::new(AtomicBool::new(true)),
        }
    }
    pub fn observe(&mut self, now: Instant, idle: bool, user_input: bool) -> bool {
        if !idle || user_input {
            self.last_activity = now;
        }
        let eligible =
            idle && now.saturating_duration_since(self.last_activity) >= Duration::from_secs(300);
        self.foreground.store(!eligible, Ordering::Release);
        eligible
    }
}
#[cfg(test)]
impl Drop for IdleClock {
    fn drop(&mut self) {
        self.foreground.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn hrs253_shutdown_signal_cancels_blocking_preparation_and_restores_scope() {
        let foreground = Arc::new(AtomicBool::new(false));
        let flag = foreground.clone();
        let (ready, waiting) = tokio::sync::oneshot::channel();
        let (resume, proceed) = std::sync::mpsc::channel();
        let worker = tokio::task::spawn_blocking(move || {
            preparation_scope(flag, || {
                preparation_boundary().unwrap();
                let nested = Arc::new(AtomicBool::new(true));
                preparation_scope(nested, || assert!(preparation_boundary().is_err()));
                preparation_boundary().unwrap();
                ready.send(()).unwrap();
                proceed.recv().unwrap();
                let result = preparation_boundary();
                assert!(result.is_err());
                result
            })
        });
        waiting.await.unwrap();
        foreground.store(true, Ordering::Release);
        resume.send(()).unwrap();
        assert!(worker.await.unwrap().is_err());
        preparation_boundary().unwrap();
        let panic_flag = Arc::new(AtomicBool::new(true));
        let _ = std::panic::catch_unwind(|| preparation_scope(panic_flag, || panic!("unwind")));
        preparation_boundary().unwrap();
    }

    #[test]
    fn hrs253_cancelled_preparation_does_not_publish_snapshot_or_cycle() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        crate::harness_snapshot::tests::fixture(&source);
        let vault = LearningVault::open_at(temp.path().join("vault")).unwrap();
        let expected = crate::harness_snapshot::source_digest(&source).unwrap();
        let index = std::fs::read(source.join(".git/index")).unwrap();
        let foreground = Arc::new(AtomicBool::new(true));
        preparation_scope(foreground, || {
            let error = crate::harness_snapshot::capture(&vault, &source, &expected).unwrap_err();
            assert!(format!("{error:#}").contains("yielded to foreground"));
            assert!(
                crate::harness_discovery::discover(&temp.path().join("missing"), Some(&vault))
                    .is_err()
            );
        });
        assert!(codex_team::self_improvement::list_harness_cycles(&vault)
            .unwrap()
            .is_empty());
        assert_eq!(std::fs::read(source.join(".git/index")).unwrap(), index);
        assert_eq!(
            std::fs::read_dir(vault.resolve_relative("harness/inputs").unwrap())
                .unwrap()
                .count(),
            0
        );
        // Explicit work on the same thread has no automatic foreground gate.
        let (_, owner) = crate::harness_snapshot::capture(&vault, &source, &expected).unwrap();
        drop(owner);
    }

    #[tokio::test]
    async fn hrs253_supervisor_shutdown_joins_cooperative_blocking_preparation() {
        let mut supervisor = Supervisor::new();
        supervisor.clock.last_activity = Instant::now() - Duration::from_secs(301);
        let (ready, waiting) = tokio::sync::oneshot::channel();
        let finished = Arc::new(AtomicBool::new(false));
        let done = finished.clone();
        supervisor
            .poll_with(true, true, |foreground| {
                tokio::spawn(async move {
                    tokio::task::spawn_blocking(move || {
                        preparation_scope(foreground, || {
                            preparation_boundary().unwrap();
                            ready.send(()).unwrap();
                            let deadline = Instant::now() + Duration::from_secs(2);
                            while preparation_boundary().is_ok() {
                                assert!(
                                    Instant::now() < deadline,
                                    "shutdown did not signal preparation"
                                );
                                std::thread::sleep(Duration::from_millis(1));
                            }
                            done.store(true, Ordering::Release);
                        })
                    })
                    .await
                    .unwrap();
                    Ok(None)
                })
            })
            .await;
        waiting.await.unwrap();
        supervisor.shutdown().await;
        assert!(!supervisor.busy());
        assert!(finished.load(Ordering::Acquire));
        assert!(supervisor.clock.foreground.load(Ordering::Acquire));
        assert!(supervisor
            .poll_with(true, true, |_| panic!("shutdown must reset inactivity"))
            .await
            .is_none());
    }

    #[test]
    fn hrs253_idle_clock_requires_uninterrupted_inactivity_and_shutdown_stops_admission() {
        let start = Instant::now();
        let mut clock = IdleClock::new(start);
        assert!(!clock.observe(start + Duration::from_secs(299), true, false));
        assert!(clock.observe(start + Duration::from_secs(300), true, false));
        assert!(!clock.observe(start + Duration::from_secs(301), true, true));
        assert!(!clock.observe(start + Duration::from_secs(600), true, false));
        assert!(clock.observe(start + Duration::from_secs(601), true, false));
        assert!(!clock.observe(start + Duration::from_secs(602), false, false));
        let flag = clock.foreground.clone();
        drop(clock);
        assert!(flag.load(Ordering::Acquire));
    }
    #[test]
    fn hrs253_idle_resume_requires_owned_profile_repository_and_binary() {
        let temp = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(temp.path().join("vault")).unwrap();
        let root = temp.path().canonicalize().unwrap();
        let (plan, _, _) = crate::harness_generation::tests::inputs();
        codex_team::self_improvement::start_harness_cycle(&vault, &plan, 0).unwrap();
        vault
            .write_mutable(
                OWNER,
                &Owner {
                    profile: "default".into(),
                    repository: root.clone(),
                    cycle_id: plan.id.clone(),
                    plan_digest: plan.digest.clone(),
                },
            )
            .unwrap();
        pause_for_foreground(&vault, &plan.id).unwrap();
        let foreground = AtomicBool::new(false);
        let before = load_harness_cycle(&vault, &plan.id).unwrap().1;
        assert!(
            validate_native_owner_baseline(&vault, "default", &root, "older-executable")
                .unwrap_err()
                .to_string()
                .contains("re-freeze and review")
        );
        validate_native_owner_baseline(&vault, "default", &root, &plan.baseline_binary_digest)
            .unwrap();
        validate_native_owner_baseline(&vault, "other", &root, "older-executable").unwrap();
        assert_eq!(
            codex_team::self_improvement::canonical_digest(&before).unwrap(),
            codex_team::self_improvement::canonical_digest(
                &load_harness_cycle(&vault, &plan.id).unwrap().1
            )
            .unwrap(),
        );
        assert!(resume_owned(
            &vault,
            "other",
            &root,
            &plan.baseline_binary_digest,
            &foreground
        )
        .unwrap()
        .is_none());
        assert!(resume_owned(
            &vault,
            "default",
            vault.root(),
            &plan.baseline_binary_digest,
            &foreground
        )
        .unwrap()
        .is_none());
        assert!(
            resume_owned(&vault, "default", &root, "another-binary", &foreground)
                .unwrap()
                .is_none()
        );
        foreground.store(true, Ordering::Release);
        assert!(resume_owned(
            &vault,
            "default",
            &root,
            &plan.baseline_binary_digest,
            &foreground
        )
        .unwrap()
        .is_none());
        foreground.store(false, Ordering::Release);
        assert_eq!(
            resume_owned(
                &vault,
                "default",
                &root,
                &plan.baseline_binary_digest,
                &foreground
            )
            .unwrap(),
            Some(plan.id.clone())
        );
        pause_for_foreground(&vault, &plan.id).unwrap();
        codex_team::self_improvement::pause_harness_cycle(&vault, &plan.id).unwrap();
        assert!(resume_owned(
            &vault,
            "default",
            &root,
            &plan.baseline_binary_digest,
            &foreground
        )
        .unwrap()
        .is_none());
    }

    #[tokio::test]
    async fn hrs253_supervisor_dispatches_once_and_foreground_signals_the_running_worker() {
        let mut supervisor = Supervisor::new();
        supervisor.clock.last_activity = Instant::now() - Duration::from_secs(301);
        let (send, receive) = tokio::sync::oneshot::channel();
        let observed = supervisor.clock.foreground.clone();
        assert!(supervisor
            .poll_with(true, true, |flag| {
                assert!(!flag.load(Ordering::Acquire));
                tokio::spawn(async move {
                    receive.await.unwrap();
                    Ok(Some("finished".into()))
                })
            })
            .await
            .is_none());
        assert!(supervisor.busy());
        assert!(supervisor
            .poll_with(true, true, |_| panic!("duplicate idle dispatch"))
            .await
            .is_none());
        supervisor.user_activity();
        assert!(observed.load(Ordering::Acquire));
        send.send(()).unwrap();
        while !supervisor.task.as_ref().unwrap().is_finished() {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            supervisor
                .poll_with(true, true, |_| panic!("foreground must prevent dispatch"))
                .await
                .as_deref(),
            Some("finished")
        );
        assert!(!supervisor.busy());
        supervisor.shutdown().await;
    }

    #[test]
    fn hrs253_idle_admission_enforces_cooldown_and_unsettled_terminal_work() {
        let records = crate::harness_freeze::tests::outcomes();
        let group = crate::harness_discovery::select(
            &records,
            &Default::default(),
            records[0].harness.build.as_deref().unwrap(),
        )
        .unwrap()
        .remove(0);
        assert!(admission(&[], &group, 86400));
        let (plan, _, _) = crate::harness_generation::tests::inputs();
        let mut cycle = codex_team::self_improvement::HarnessCycle::new(&plan, 10).unwrap();
        assert!(!admission(&[cycle.clone()], &group, 90000));
        cycle.stage = HarnessStage::Rejected;
        assert!(!admission(&[cycle.clone()], &group, 86409));
        assert!(admission(&[cycle.clone()], &group, 86410));
        cycle.stage = HarnessStage::BudgetExhausted;
        assert!(!admission(&[cycle.clone()], &group, 86409));
        assert!(admission(&[cycle.clone()], &group, 86410));
        cycle.active_reservations.insert("unfinished".into(), 1);
        assert!(!admission(&[cycle.clone()], &group, 90000));
        cycle.active_reservations.clear();
        cycle.reservations.insert("unknown".into(), 1);
        assert!(!admission(&[cycle], &group, 90000));
    }

    #[tokio::test]
    async fn hrs253_disabled_idle_worker_is_inert_and_background_driver_yields_before_input_reads()
    {
        let temp = tempfile::tempdir().unwrap();
        let config = crate::config::CaConfig::default();
        assert!(!toml::to_string(&config)
            .unwrap()
            .contains("harness_improvement"));
        assert!(run_once_at(
            config.clone(),
            crate::runtime::RuntimePaths::from_home(temp.path()),
            crate::runtime::ActiveProfile::Default,
            Arc::new(AtomicBool::new(false)),
            temp.path().join("automation/harness"),
        )
        .await
        .unwrap()
        .is_none());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
        let vault = LearningVault::open_at(temp.path().join("vault")).unwrap();
        let (plan, _, _) = crate::harness_generation::tests::inputs();
        codex_team::self_improvement::start_harness_cycle(&vault, &plan, 0).unwrap();
        let result = scope(
            &vault,
            &plan.id,
            Arc::new(AtomicBool::new(true)),
            crate::harness_control::run_background(
                vault.clone(),
                plan.id.clone(),
                "/unavailable".into(),
                "/unavailable".into(),
                config,
                None,
            ),
        )
        .await;
        assert!(result.is_err());
        let (_, state) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert_eq!(state.pause_cause, Some(HarnessPauseCause::Foreground));
        assert!(state.reservations.is_empty());
        assert!(state.active_reservations.is_empty());
    }

    #[tokio::test]
    async fn hrs253_foreground_pause_is_scoped_settled_and_never_overrides_user_pause() {
        let temp = tempfile::tempdir().unwrap();
        let vault = LearningVault::open_at(temp.path().join("vault")).unwrap();
        let (plan, _, _) = crate::harness_generation::tests::inputs();
        codex_team::self_improvement::start_harness_cycle(&vault, &plan, 0).unwrap();
        boundary(&vault, &plan.id).await.unwrap();
        let flag = Arc::new(AtomicBool::new(true));
        assert!(
            scope(&vault, &plan.id, flag.clone(), boundary(&vault, &plan.id))
                .await
                .is_err()
        );
        let (_, paused) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert_eq!(paused.pause_cause, Some(HarnessPauseCause::Foreground));
        assert!(resume_foreground_pause(&vault, &plan.id).unwrap());
        update_harness_cycle(&vault, &plan.id, |state| {
            state.reserve_active_time(&plan, "running", 1)
        })
        .unwrap();
        assert!(
            scope(&vault, &plan.id, flag.clone(), boundary(&vault, &plan.id))
                .await
                .is_err()
        );
        let (_, running) = load_harness_cycle(&vault, &plan.id).unwrap();
        assert_eq!(running.stage, HarnessStage::EvaluationFrozen);
        assert!(running.active_reservations.contains_key("running"));
        update_harness_cycle(&vault, &plan.id, |state| {
            state.settle_active_time(&plan, "running", 1)
        })
        .unwrap();
        assert!(
            scope(&vault, &plan.id, flag.clone(), boundary(&vault, &plan.id))
                .await
                .is_err()
        );
        codex_team::self_improvement::pause_harness_cycle(&vault, &plan.id).unwrap();
        assert!(!resume_foreground_pause(&vault, &plan.id).unwrap());
        assert_eq!(
            load_harness_cycle(&vault, &plan.id).unwrap().1.pause_cause,
            Some(HarnessPauseCause::User)
        );
        let other = LearningVault::open_at(temp.path().join("other")).unwrap();
        scope(&vault, &plan.id, flag, boundary(&other, &plan.id))
            .await
            .unwrap();
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub repository: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vm: Option<codex_team::self_improvement::HarnessVmProfile>,
}
impl AutomationConfig {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    profile: String,
    repository: PathBuf,
    cycle_id: String,
    plan_digest: String,
}
const OWNER: &str = "harness/idle-owner.json";

pub(crate) fn resumable_provider(
    vault: &LearningVault,
    profile: &str,
    repository: &std::path::Path,
) -> Result<Option<(String, String)>> {
    let Some(owner) = vault.read_mutable::<Owner>(OWNER)? else {
        return Ok(None);
    };
    let (plan, cycle) = load_harness_cycle(vault, &owner.cycle_id)?;
    if owner.plan_digest != plan.digest {
        bail!("automatic experiment ownership changed");
    }
    if owner.profile != profile
        || owner.repository != repository.canonicalize()?
        || cycle.stage != HarnessStage::Paused
        || cycle.pause_cause != Some(HarnessPauseCause::Foreground)
        || cycle.can_abandon(true)
    {
        return Ok(None);
    }
    let inputs = crate::harness_freeze::load_inputs(vault, &owner.cycle_id).or_else(|_| {
        crate::harness_generation::load_candidate(vault, &owner.cycle_id).map(|(inputs, _)| inputs)
    })?;
    Ok(Some((
        inputs.provider.source.provider,
        inputs.provider.model,
    )))
}

pub(crate) fn abandonable_owner(
    vault: &LearningVault,
    profile: &str,
    repository: &std::path::Path,
) -> Result<Option<String>> {
    let Some(owner) = vault.read_mutable::<Owner>(OWNER)? else {
        return Ok(None);
    };
    let (plan, cycle) = load_harness_cycle(vault, &owner.cycle_id)?;
    if owner.plan_digest != plan.digest {
        bail!("automatic experiment ownership changed");
    }
    Ok((owner.profile == profile
        && owner.repository == repository.canonicalize()?
        && crate::harness_control::blocks_new_cycle(&cycle)
        && cycle.can_abandon(true))
    .then_some(owner.cycle_id))
}

#[cfg(test)]
#[test]
fn hrs253_recovery_idle_closes_only_its_owned_interrupted_cycle() {
    let temp = tempfile::tempdir().unwrap();
    let repository = temp.path().canonicalize().unwrap();
    let (vault, plan) = crate::harness_recovery::tests::fixture(temp.path());
    vault
        .write_mutable(
            OWNER,
            &Owner {
                profile: "default".into(),
                repository: repository.clone(),
                cycle_id: plan.id.clone(),
                plan_digest: plan.digest.clone(),
            },
        )
        .unwrap();
    let controller = vault.acquire_harness_controller_lock().unwrap();
    let foreground = AtomicBool::new(false);
    assert!(!recover_owned(&vault, "another", &repository, &foreground, &controller).unwrap());
    foreground.store(true, Ordering::Release);
    assert!(!recover_owned(&vault, "default", &repository, &foreground, &controller).unwrap());
    foreground.store(false, Ordering::Release);
    codex_team::self_improvement::pause_harness_cycle(&vault, &plan.id).unwrap();
    assert!(!recover_owned(&vault, "default", &repository, &foreground, &controller).unwrap());
    codex_team::self_improvement::resume_harness_cycle(&vault, &plan.id).unwrap();
    assert!(recover_owned(&vault, "default", &repository, &foreground, &controller).unwrap());
    assert!(!recover_owned(&vault, "default", &repository, &foreground, &controller).unwrap());
    let closed = load_harness_cycle(&vault, &plan.id).unwrap().1;
    assert_eq!(closed.reservations["unknown-post"], 100);
    assert_eq!(closed.active_reservations["unknown-work"], 30);
    assert!(!crate::harness_control::blocks_new_cycle(&closed));
    // Closing cannot erase the normal 24-hour admission cooldown.
    let records = crate::harness_freeze::tests::outcomes();
    let group = crate::harness_discovery::select(
        &records,
        &Default::default(),
        records[0].harness.build.as_deref().unwrap(),
    )
    .unwrap()
    .remove(0);
    assert!(!admission(
        std::slice::from_ref(&closed),
        &group,
        closed.created_at + 86399
    ));
    assert!(admission(
        std::slice::from_ref(&closed),
        &group,
        closed.created_at + 86400
    ));
}

fn recover_owned(
    vault: &LearningVault,
    profile: &str,
    repository: &std::path::Path,
    foreground: &AtomicBool,
    controller: &codex_team::self_improvement::HarnessControllerLock,
) -> Result<bool> {
    let Some(owner) = vault.read_mutable::<Owner>(OWNER)? else {
        return Ok(false);
    };
    let (plan, cycle) = load_harness_cycle(vault, &owner.cycle_id)?;
    if owner.plan_digest != plan.digest {
        bail!("automatic experiment ownership changed");
    }
    if foreground.load(Ordering::Acquire)
        || owner.profile != profile
        || repository.canonicalize()? != owner.repository
        || !crate::harness_control::blocks_new_cycle(&cycle)
        || !cycle.can_abandon(true)
    {
        return Ok(false);
    }
    crate::harness_recovery::abandon_owned(vault, &owner.cycle_id, true, controller)
}

fn admission(
    cycles: &[codex_team::self_improvement::HarnessCycle],
    group: &crate::harness_discovery::FailureGroup,
    now: u64,
) -> bool {
    let active = cycles.iter().any(crate::harness_control::blocks_new_cycle);
    let settings = codex_team::self_improvement::HarnessSettings {
        enabled: true,
        ..Default::default()
    };
    let tasks = group
        .evidence_ids
        .iter()
        .filter_map(|id| id.rsplit_once(':').map(|(task, _)| task.to_owned()))
        .collect::<Vec<_>>();
    group.independent_tasks >= 2
        && codex_team::self_improvement::harness_idle_eligible(
            &settings,
            300,
            now,
            cycles.iter().map(|cycle| cycle.created_at).max(),
            &tasks,
            active,
        )
}

fn resume_owned(
    vault: &LearningVault,
    profile: &str,
    repository: &std::path::Path,
    binary_digest: &str,
    foreground: &AtomicBool,
) -> Result<Option<String>> {
    let Some(owner) = vault.read_mutable::<Owner>(OWNER)? else {
        return Ok(None);
    };
    let (plan, cycle) = load_harness_cycle(vault, &owner.cycle_id)?;
    if owner.plan_digest != plan.digest {
        bail!("automatic experiment ownership changed");
    }
    if owner.profile != profile
        || cycle.stage != HarnessStage::Paused
        || cycle.pause_cause != Some(HarnessPauseCause::Foreground)
        || foreground.load(Ordering::Acquire)
        || binary_digest != plan.baseline_binary_digest
    {
        return Ok(None);
    }
    if repository.canonicalize()? != owner.repository {
        return Ok(None);
    }
    if !resume_foreground_pause(vault, &owner.cycle_id)? {
        return Ok(None);
    }
    Ok(Some(owner.cycle_id))
}

fn validate_native_owner_baseline(
    vault: &LearningVault,
    profile: &str,
    repository: &std::path::Path,
    binary_digest: &str,
) -> Result<()> {
    let Some(owner) = vault.read_mutable::<Owner>(OWNER)? else {
        return Ok(());
    };
    let (plan, cycle) = load_harness_cycle(vault, &owner.cycle_id)?;
    if owner.plan_digest != plan.digest {
        bail!("automatic experiment ownership changed");
    }
    if owner.profile == profile
        && owner.repository == repository.canonicalize()?
        && cycle.stage == HarnessStage::Paused
        && cycle.pause_cause == Some(HarnessPauseCause::Foreground)
        && binary_digest != plan.baseline_binary_digest
    {
        bail!("frozen harness baseline cannot attest native worker containment; re-freeze and review the experiment with the current executable before evaluation; existing candidate and evidence are preserved");
    }
    Ok(())
}

pub(crate) async fn run_once_at(
    config: crate::config::CaConfig,
    paths: crate::runtime::RuntimePaths,
    profile: crate::runtime::ActiveProfile,
    foreground: Arc<AtomicBool>,
    vault_root: PathBuf,
) -> Result<Option<String>> {
    if !config.harness_improvement.enabled {
        return Ok(None);
    }
    let worker_config = config.clone();
    let control = foreground.clone();
    let native_occurrence = ca_automation::runtime::current_run_store().is_some();
    let prepared = tokio::task::spawn_blocking(move || {
        preparation_scope(control.clone(), || -> Result<_> {
            if control.load(Ordering::Acquire) {
                return Ok(None);
            }
            let repository = worker_config
                .harness_improvement
                .repository
                .clone()
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
            if !repository.is_absolute() {
                bail!("automatic experiment repository must be absolute");
            }
            let binary = std::env::current_exe()?;
            let existing = LearningVault::existing_at(vault_root.clone())?;
            let controller = existing
                .as_ref()
                .map(LearningVault::acquire_harness_controller_lock)
                .transpose()?;
            if let Some(vault) = &existing {
                recover_owned(
                    vault,
                    profile.label(),
                    &repository,
                    &control,
                    controller.as_ref().expect("existing vault controller"),
                )?;
                let digest = crate::harness_engineering::artifact_digest(&binary)?;
                if native_occurrence {
                    validate_native_owner_baseline(vault, profile.label(), &repository, &digest)?;
                }
                if let Some(id) =
                    resume_owned(vault, profile.label(), &repository, &digest, &control)?
                {
                    return Ok(Some((
                        vault.clone(),
                        id,
                        repository,
                        binary,
                        controller.unwrap(),
                    )));
                }
            }
            let provider =
                crate::harness_control::configured_provider(&worker_config, &paths, &profile)?;
            let report = crate::harness_discovery::discover(&paths.data_dir, existing.as_ref())?;
            let Some(group) = crate::harness_discovery::select_for_provider(&report, &provider)
            else {
                return Ok(None);
            };
            let cycles = existing
                .as_ref()
                .map(codex_team::self_improvement::list_harness_cycles)
                .transpose()?
                .unwrap_or_default();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            if !admission(&cycles, group, now) || control.load(Ordering::Acquire) {
                return Ok(None);
            }
            let vault = match existing {
                Some(vault) => vault,
                None => LearningVault::open_at(vault_root)?,
            };
            let controller = match controller {
                Some(lock) => lock,
                None => vault.acquire_harness_controller_lock()?,
            };
            let instructions = crate::harness_guidance::baseline_instructions(
                &vault,
                &provider.source,
                &provider.model,
                &provider.options,
            )?;
            let plan = crate::harness_discovery::create_automatic(
                &vault,
                &repository,
                &binary,
                &paths.data_dir,
                provider,
                instructions,
                true,
                worker_config.harness_improvement.vm.as_ref(),
            )?;
            {
                let _lock = vault.acquire_lock()?;
                vault.write_mutable(
                    OWNER,
                    &Owner {
                        profile: profile.label().into(),
                        repository: repository.canonicalize()?,
                        cycle_id: plan.id.clone(),
                        plan_digest: plan.digest,
                    },
                )?;
            }
            if control.load(Ordering::Acquire) {
                pause_for_foreground(&vault, &plan.id)?;
            }
            Ok(Some((vault, plan.id, repository, binary, controller)))
        })
    })
    .await??;
    let Some((vault, id, repository, binary, controller)) = prepared else {
        return Ok(None);
    };
    let result = scope(
        &vault,
        &id,
        foreground,
        crate::harness_control::run_background(
            vault.clone(),
            id.clone(),
            repository,
            binary,
            config,
            Some(controller),
        ),
    )
    .await;
    let (_, state) = load_harness_cycle(&vault, &id)?;
    if state.stage == HarnessStage::Paused {
        return Ok(Some(format!("Experiment {id} paused")));
    }
    result?;
    Ok(Some(format!("Experiment {id}: {:?}", state.stage)))
}

#[cfg(test)]
#[allow(dead_code)] // Legacy idle-observer fixtures; production admission is ca-owned.
pub(crate) struct Supervisor {
    clock: IdleClock,
    task: Option<tokio::task::JoinHandle<Result<Option<String>>>>,
    next_scan: Instant,
    blocked: Option<crate::harness_control::PricingAdmissionBlocked>,
    pricing_probe: Option<tokio::task::JoinHandle<bool>>,
    next_pricing_probe: Instant,
}
#[cfg(test)]
#[allow(dead_code)]
impl Supervisor {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            clock: IdleClock::new(now),
            task: None,
            next_scan: now,
            blocked: None,
            pricing_probe: None,
            next_pricing_probe: now,
        }
    }
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }
    pub fn user_activity(&mut self) {
        self.clock.observe(Instant::now(), false, true);
    }
    pub async fn poll(
        &mut self,
        idle: bool,
        config: &crate::config::CaConfig,
        paths: &crate::runtime::RuntimePaths,
        profile: &crate::runtime::ActiveProfile,
    ) -> Option<String> {
        use futures::FutureExt;
        if let Some(blocked) = &self.blocked {
            let route_changed = blocked.provider != config.defaults.provider
                || blocked.model != config.defaults.model
                || blocked.profile != profile.label();
            let prices_changed = self
                .pricing_probe
                .as_ref()
                .is_some_and(|task| task.is_finished())
                && self
                    .pricing_probe
                    .take()
                    .unwrap()
                    .now_or_never()
                    .unwrap()
                    .unwrap_or(false);
            if route_changed || prices_changed {
                if let Some(task) = self.pricing_probe.take() {
                    task.abort();
                }
                self.blocked = None;
                self.next_scan = Instant::now();
            } else if self.pricing_probe.is_none() && Instant::now() >= self.next_pricing_probe {
                self.next_pricing_probe = Instant::now() + Duration::from_secs(1);
                let store = crate::pricing::PricingStore::for_paths(paths, profile);
                let revision = blocked.revision.clone();
                self.pricing_probe = Some(tokio::task::spawn_blocking(move || {
                    store
                        .admission_revision()
                        .is_ok_and(|current| current != revision)
                }));
            }
        }
        // Admission now belongs to the common automation queue. This observer
        // only settles already-owned activity; it never starts an experiment.
        self.poll_with(idle, false, |_foreground| tokio::spawn(async { Ok(None) }))
            .await
    }

    async fn poll_with(
        &mut self,
        idle: bool,
        enabled: bool,
        spawn: impl FnOnce(Arc<AtomicBool>) -> tokio::task::JoinHandle<Result<Option<String>>>,
    ) -> Option<String> {
        let now = Instant::now();
        let eligible = self.clock.observe(now, idle && enabled, false);
        let mut message = None;
        if self.task.as_ref().is_some_and(|task| task.is_finished()) {
            let result = self.task.take().unwrap().await;
            message = match result {
                Ok(Ok(message)) => message,
                Ok(Err(error))
                    if error
                        .downcast_ref::<crate::harness_control::PricingAdmissionBlocked>()
                        .is_some() =>
                {
                    let blocked = error
                        .downcast_ref::<crate::harness_control::PricingAdmissionBlocked>()
                        .unwrap()
                        .clone();
                    let message =
                        crate::terminal_text::sanitize_terminal_text(&blocked.to_string());
                    self.blocked = Some(blocked);
                    Some(message)
                }
                Ok(Err(error)) => Some(format!(
                    "Automatic experiment stopped: {}",
                    crate::learning::sanitized_learning_text(&format!("{error:#}"))
                        .unwrap_or_else(|| "details unavailable".into())
                        .chars()
                        .take(200)
                        .collect::<String>()
                )),
                Err(_) => Some(
                    "Automatic experiment worker stopped; inspect its retained receipts".into(),
                ),
            };
            self.next_scan = now + Duration::from_secs(60);
        }
        if eligible && self.task.is_none() && self.blocked.is_none() && now >= self.next_scan {
            self.next_scan = now + Duration::from_secs(60);
            self.task = Some(spawn(self.clock.foreground.clone()));
        }
        message
    }
    pub async fn shutdown(&mut self) {
        self.user_activity();
        if let Some(mut task) = self.task.take() {
            if tokio::time::timeout(Duration::from_secs(5), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
    }
}
#[cfg(test)]
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.clock.foreground.store(true, Ordering::Release);
        if let Some(task) = &self.task {
            task.abort();
        }
        if let Some(task) = &self.pricing_probe {
            task.abort();
        }
    }
}

#[cfg(test)]
mod pricing_admission_tests {
    use super::*;

    #[tokio::test]
    async fn bug257_missing_prices_emit_once_and_route_change_rearms_admission() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(temporary.path());
        let profile = crate::runtime::ActiveProfile::Default;
        let mut config = crate::config::CaConfig::default_config();
        config.defaults.provider = "unknown-provider".into();
        config.defaults.model = "unknown-model".into();
        let mut supervisor = Supervisor::new();
        supervisor.clock.last_activity = Instant::now() - Duration::from_secs(301);
        let revision = crate::pricing::PricingStore::for_paths(&paths, &profile)
            .admission_revision()
            .unwrap();
        let blocked = crate::harness_control::PricingAdmissionBlocked {
            provider: config.defaults.provider.clone(),
            model: config.defaults.model.clone(),
            profile: profile.label().into(),
            revision,
        };
        supervisor.task = Some(tokio::spawn(async move { Err(blocked.into()) }));
        while !supervisor.task.as_ref().unwrap().is_finished() {
            tokio::task::yield_now().await;
        }
        let notice = supervisor
            .poll_with(true, true, |_| {
                panic!("blocked admission must not dispatch")
            })
            .await
            .unwrap();
        assert!(notice.contains("unknown-provider/unknown-model"));
        assert!(notice.contains("Ordinary chat remains available"));
        for _ in 0..100 {
            supervisor.next_scan = Instant::now();
            assert!(supervisor
                .poll_with(true, true, |_| panic!(
                    "unchanged prices must remain latched"
                ))
                .await
                .is_none());
        }
        config.defaults.model = "another-model".into();
        config.harness_improvement.enabled = false; // Re-arm only; never launch a real experiment.
        supervisor.poll(false, &config, &paths, &profile).await;
        assert!(supervisor.blocked.is_none());
        assert!(!supervisor.busy());
    }
}
