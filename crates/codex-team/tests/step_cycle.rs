//! Integration tests for the step / status / doctor public API surface.

mod common;

use common::{cleanup, init_orchestrator, patch_config_no_codex, patch_state_quiesce, temp_repo};
use std::fs;

/// After `init`, calling `step` with an empty NEXT_STEPS.md should succeed
/// (no work items to process).
#[test]
fn init_then_step_empty_queue_returns_no_work() {
    let repo = temp_repo("step-empty-queue");
    init_orchestrator(&repo, "integration test: empty queue step");

    // Overwrite NEXT_STEPS.md with an empty task list.
    let next_steps = repo.join(".orchestrator/NEXT_STEPS.md");
    fs::write(&next_steps, "# NEXT_STEPS\n\nNo approved tasks yet.\n").expect("write NEXT_STEPS");

    // Disable codex requirements and quiesce routing flags so step() doesn't
    // try to spawn a real AI process.
    patch_config_no_codex(&repo);
    patch_state_quiesce(&repo);

    let result = codex_team::app::step(repo.clone(), true);
    cleanup(&repo);

    result.expect("step on empty queue should succeed");
}

/// After `init`, calling `status` should succeed and print/emit state.
#[test]
fn status_after_init() {
    let repo = temp_repo("status-after-init");
    init_orchestrator(&repo, "integration test: status check");

    let result = codex_team::app::status(repo.clone(), true, false);
    cleanup(&repo);

    result.expect("status after init should succeed");
}

/// After `init`, calling `doctor_command` should succeed.
/// Uses non-JSON mode since JSON mode bails on health check failures
/// (ollama/codex not available in test environments).
#[test]
fn doctor_on_init_repo() {
    let repo = temp_repo("doctor-init");
    init_orchestrator(&repo, "integration test: doctor check");

    // Patch config so the codex binary health-check passes (no real codex on
    // PATH in CI / integration test envs).
    patch_config_no_codex(&repo);

    let result = codex_team::app::doctor_command(repo.clone(), false);
    cleanup(&repo);

    result.expect("doctor on initialized repo should succeed");
}
