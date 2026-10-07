//! Close a dead pre-decision experiment without pretending unknown usage was
//! free or successful. All execution owners must be excluded before mutation.
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{
    load_harness_cycle, update_harness_cycle, HarnessControllerLock, LearningVault,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) fn abandon(vault: &LearningVault, id: &str) -> Result<()> {
    let controller = vault.acquire_harness_controller_lock()?;
    if !abandon_owned(vault, id, false, &controller)?
        && load_harness_cycle(vault, id)?.1.abandonment.is_none()
    {
        bail!("only an interrupted pre-decision experiment can be abandoned");
    }
    Ok(())
}

pub(crate) fn abandon_owned(
    vault: &LearningVault,
    id: &str,
    automatic: bool,
    controller: &HarnessControllerLock,
) -> Result<bool> {
    if !controller.matches(vault) {
        bail!("recovery controller belongs to another vault");
    }
    let _worker = vault.acquire_worker_lock()?;
    let (plan, state) = load_harness_cycle(vault, id)?;
    if !state.can_abandon(automatic) {
        return Ok(false);
    }
    let frozen = plan.vm.as_ref().context(
        "automatic quiescence proof requires a frozen dedicated VM; legacy host work is retained",
    )?;
    let _vm = crate::harness_vm::ownership::VmOwnership::acquire(&frozen.profile.home)?;
    // This inspects inventory as well as frozen controller/template identity.
    // A dead supervisor with an orphan guest cannot authorize recovery.
    crate::harness_vm::verify(frozen, Instant::now() + Duration::from_secs(30))?;
    if crate::harness_promotion::report(vault, id)?.is_some()
        || crate::harness_guidance::report(vault, id)?.is_some()
        || crate::harness_activation::report(vault, id)?.is_some()
    {
        bail!("activation evidence requires its existing promotion/rollback recovery");
    }
    crate::harness_idle::preparation_boundary()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    update_harness_cycle(vault, id, |cycle| {
        cycle.abandon_interrupted(&plan, now, automatic)
    })?;
    Ok(true)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use codex_team::self_improvement::{
        start_harness_cycle, HarnessPlan, HarnessStage, HarnessVmProfile,
    };
    use std::os::unix::fs::PermissionsExt;

    pub(crate) fn fixture(root: &std::path::Path) -> (LearningVault, HarnessPlan) {
        let root = root.canonicalize().unwrap();
        let home = root.join("vm");
        std::fs::create_dir_all(home.join("vms/base")).unwrap();
        for name in ["config.json", "disk.img", "nvram.bin"] {
            std::fs::write(home.join("vms/base").join(name), b"fixture").unwrap();
        }
        let tart = root.join("tart");
        std::fs::write(&tart, b"#!/bin/sh\ncat \"$TART_HOME/inventory.json\"\n").unwrap();
        std::fs::set_permissions(&tart, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(
            home.join("inventory.json"),
            br#"[{"Name":"base","Source":"local","Running":false,"State":"stopped"}]"#,
        )
        .unwrap();
        let vm = crate::harness_vm::freeze(
            &HarnessVmProfile {
                tart,
                home,
                template: "base".into(),
                guest_agent_sha256: "a".repeat(64),
            },
            30,
        )
        .unwrap();
        let (mut plan, _, _) = crate::harness_generation::tests::inputs();
        plan.vm = Some(vm);
        plan.seal().unwrap();
        let vault = LearningVault::open_at(root.join("vault")).unwrap();
        start_harness_cycle(&vault, &plan, 1).unwrap();
        update_harness_cycle(&vault, &plan.id, |state| {
            state.reserve(&plan, "unknown-post", 100)?;
            state.reserve_active_time(&plan, "unknown-work", 30)
        })
        .unwrap();
        (vault, plan)
    }

    #[test]
    fn hrs253_recovery_retains_unknown_usage_and_seals_closed_cycle() {
        let temp = tempfile::tempdir().unwrap();
        let (vault, plan) = fixture(temp.path());
        let before = load_harness_cycle(&vault, &plan.id).unwrap().1;
        abandon(&vault, &plan.id).unwrap();
        let after = load_harness_cycle(&vault, &plan.id).unwrap().1;
        assert_eq!(after.stage, HarnessStage::Rejected);
        assert_eq!(after.reservations, before.reservations);
        assert_eq!(after.active_reservations, before.active_reservations);
        assert_eq!(after.settled_microusd, before.settled_microusd);
        assert_eq!(after.active_seconds, before.active_seconds);
        assert!(!crate::harness_control::blocks_new_cycle(&after));
        assert!(after.abandonment.is_some());
        assert!(
            update_harness_cycle(&vault, &plan.id, |state| state.settle("unknown-post", 0))
                .is_err()
        );
        assert!(codex_team::self_improvement::resume_harness_cycle(&vault, &plan.id).is_err());
        abandon(&vault, &plan.id).unwrap(); // Idempotent, no replacement receipt.
        assert_eq!(
            serde_json::to_value(&after).unwrap(),
            serde_json::to_value(load_harness_cycle(&vault, &plan.id).unwrap().1).unwrap()
        );
        let mut tampered = after;
        tampered.reservations.clear();
        vault
            .write_mutable(&format!("harness/cycles/{}.json", plan.id), &tampered)
            .unwrap();
        assert!(load_harness_cycle(&vault, &plan.id).is_err());
    }

    #[test]
    fn hrs253_recovery_excludes_live_controllers_workers_and_vms() {
        let temp = tempfile::tempdir().unwrap();
        let (vault, plan) = fixture(temp.path());
        let before = serde_json::to_value(load_harness_cycle(&vault, &plan.id).unwrap().1).unwrap();
        let controller = vault.acquire_harness_controller_lock().unwrap();
        assert!(abandon(&vault, &plan.id).is_err());
        let worker = vault.acquire_worker_lock().unwrap();
        assert!(abandon_owned(&vault, &plan.id, true, &controller).is_err());
        drop(worker);
        let home = &plan.vm.as_ref().unwrap().profile.home;
        let vm = crate::harness_vm::ownership::VmOwnership::acquire(home).unwrap();
        assert!(abandon_owned(&vault, &plan.id, true, &controller).is_err());
        drop(vm);
        std::fs::write(
            home.join("inventory.json"),
            br#"[
            {"Name":"base","Source":"local","Running":false,"State":"stopped"},
            {"Name":"ca-harness-run-orphan","Source":"local","Running":true,"State":"running"}
        ]"#,
        )
        .unwrap();
        assert!(abandon_owned(&vault, &plan.id, true, &controller).is_err());
        assert_eq!(
            before,
            serde_json::to_value(load_harness_cycle(&vault, &plan.id).unwrap().1).unwrap()
        );
    }

    #[test]
    fn hrs253_recovery_preserves_user_pauses_and_decision_states() {
        let temp = tempfile::tempdir().unwrap();
        let (vault, plan) = fixture(temp.path());
        codex_team::self_improvement::pause_harness_cycle(&vault, &plan.id).unwrap();
        let controller = vault.acquire_harness_controller_lock().unwrap();
        assert!(!abandon_owned(&vault, &plan.id, true, &controller).unwrap());
        for stage in [
            HarnessStage::ReadyToPromote,
            HarnessStage::SourceAdopting,
            HarnessStage::SourceAdopted,
            HarnessStage::Promoted,
            HarnessStage::WaitingForCleanSource,
            HarnessStage::RolledBack,
        ] {
            let mut cycle = load_harness_cycle(&vault, &plan.id).unwrap().1;
            cycle.stage = stage;
            assert!(!cycle.can_abandon(false));
        }
        let mut cycle = load_harness_cycle(&vault, &plan.id).unwrap().1;
        cycle.stage = HarnessStage::RecoveryRequired;
        cycle.decision = Some(codex_team::self_improvement::HarnessDecision {
            verdict: codex_team::self_improvement::HarnessVerdict::Promote,
            reasons: vec![],
            fixed_cases: vec![],
            regressed_cases: vec![],
            trials_digest: "a".repeat(64),
        });
        assert!(!cycle.can_abandon(false));
        assert!(abandon_owned(&vault, &plan.id, false, &controller).unwrap());
    }
}
