# ORCH-WATCH-009: Auto-Gate PM-Created Watch Pauses

## Purpose / Big Picture
When `watch-loop --self-heal` hits a Project Manager-created pause whose own options include creating an explicit safety gate, Rust should choose that gate automatically instead of asking the human to type `g`. This does not approve product work; it only records the gate disposition and routes back to PM to formalize the safety boundary.

## Progress
- [x] Root cause and intended contract recorded.
- [x] Auto-gate heuristic implemented.
- [x] Manual behavior preserved for dirty-adoption and non-gate pauses.
- [x] Focused tests and validation completed.
- [x] Release binary rebuilt.

## Surprises & Discoveries
- PM-created human-disposition pauses already include a gate option from `human_disposition_pause_from_planner`.
- `watch-loop` already knows whether `--self-heal` is enabled, so the auto disposition can be scoped to explicit self-heal runs.

## Decision Log
- Auto-gate only when all are true: `--self-heal`, pause source is `project_manager`, and the pause reason/options mention an explicit safety/approval gate.
- Do not auto-gate `rust_dirty_adoption` pauses or generic dirty-baseline pauses.
- Record the same disposition as a human `gate` choice so existing audit/state semantics remain unchanged.

## Context and Orientation
- `src/app.rs::watch_loop` chooses the human-disposition mode.
- `src/app.rs::run_one_supervised_cycle_with_human_mode` applies the selected mode.
- `src/app.rs::apply_human_disposition_choice` owns disposition semantics.

## Plan of Work
Add a small helper that classifies a pause as auto-gateable. Before each watch-loop target iteration, inspect current target state and choose `HumanDispositionMode::Preset(ApproveGate)` only for self-heal PM gate pauses. Keep the existing interactive prompt and non-interactive stop behavior for everything else. Add tests for the positive and negative paths.

## Concrete Steps
1. Add `watch_loop_target_human_disposition_mode(paths, self_heal)` and `pause_is_auto_gateable`.
2. Use it per target iteration so freshly-created pauses can be handled on the next loop.
3. Add tests for PM gateable pause auto-gating and rust dirty-adoption pause not auto-gating.
4. Run validation and rebuild release binary.

## Validation and Acceptance
- `cargo test watch_loop`
- `cargo test human_disposition`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

Acceptance: `watch-loop --self-heal` auto-applies gate for PM-created gate pauses; non-self-heal, dirty-adoption, and non-gate pauses still prompt or stop.

## Idempotence and Recovery
If auto-gate is applied, the existing disposition state/event path records it. If a later cycle cannot proceed, normal PM/Watcher/guard stop conditions still apply.

## Artifacts and Notes
- Validation:
  - `cargo test watch_loop` passed 9 tests.
  - `cargo test human_disposition` passed 10 tests.
  - `cargo fmt -- --check` initially reported formatting drift, then passed after `cargo fmt`.
  - `cargo test` passed 217 unit tests, plus zero-test main/doc targets.
  - `cargo build --release` passed and refreshed `target/release/codex-team`.
  - `git diff --check` passed.

## Interfaces and Dependencies
No new CLI flags or dependencies.

## Outcomes & Retrospective
`watch-loop --self-heal` now auto-records the explicit gate disposition for PM-created gateable pauses, which removes the useless `type g` interruption while preserving human control for true dirty-adoption pauses. This should let the watcher continue into PM gate formalization without pretending to approve product edits.
