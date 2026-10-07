# ORCH-WATCH-010: Auto-Gate Newly Created PM Pauses

## Purpose / Big Picture
`watch-loop --self-heal` can auto-gate a PM-created gateable pause when that pause exists before a cycle starts, but a target step may create the same pause during the cycle. The loop should immediately apply the same safe gate disposition instead of stopping and asking the user to type `g`.

## Progress
- [x] Root cause and intended contract recorded.
- [x] Auto-gate helper handles active pauses before and after a target step.
- [x] Focused regression tests added.
- [x] Validation completed and release binary rebuilt.

## Surprises & Discoveries
- The first auto-gate slice selected a preset mode before running the target step. That does not handle pauses produced later by PM inside the same step.

## Decision Log
- Reuse the existing `ApproveGate` disposition path so state/events stay identical to a manual `g` answer.
- Keep auto-gate scoped to `--self-heal`, `source=project_manager`, and explicit safety/approval gate text.

## Context and Orientation
- `src/app.rs::watch_loop` owns the supervised loop.
- `src/app.rs::pause_is_auto_gateable` owns the gateable-pause predicate.
- `src/app.rs::apply_human_disposition_choice` records the decision.

## Plan of Work
Add a helper that loads state, applies `ApproveGate` when the active pause is auto-gateable, saves state, and reports status. Call it before the target step and again immediately after the target step if the step returns a human pause.

## Concrete Steps
1. Add `try_auto_gate_watch_loop_pause`.
2. Call it before target step setup and after target step when `target_outcome.human_pause`.
3. Add tests for applying the helper and not applying it to dirty-adoption pauses.
4. Run validation.

## Validation and Acceptance
- `cargo test watch_loop`
- `cargo test human_disposition`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

Acceptance: a PM-created gateable pause created during a watch-loop target step is auto-gated under `--self-heal` and does not stop the loop solely to ask for `g`.

## Idempotence and Recovery
If the helper cannot load/save state or the pause is not gateable, the existing prompt/stop behavior remains.

## Artifacts and Notes
- Validation:
  - `cargo test watch_loop` passed 11 tests.
  - `cargo test human_disposition` passed 10 tests.
  - `cargo fmt -- --check` initially reported formatting drift, then passed after `cargo fmt`.
  - `cargo test` passed 219 unit tests, plus zero-test main/doc targets.
  - `cargo build --release` passed and refreshed `target/release/codex-team`.
  - `git diff --check` passed.

## Interfaces and Dependencies
No CLI changes or new dependencies.

## Outcomes & Retrospective
The watch loop now applies the auto-gate helper both before target work and after target work when PM creates a new gateable pause. This closes the "asks me to type g" gap while keeping dirty-adoption pauses manual.
