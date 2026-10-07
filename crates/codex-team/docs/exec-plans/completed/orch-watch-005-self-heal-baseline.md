# ORCH-WATCH-005: Self-Heal Baseline-Aware Watch Alerts

## Purpose / Big Picture
After `watch --self-heal` applies a mitigation, subsequent watcher runs should stop surfacing the already-handled historical alerts. Otherwise the operator still has to interpret stale noise and paste logs elsewhere.

## Progress
- [x] Identify stale watcher noise after self-heal.
- [x] Hide prompt-bloat and PM/Worker double-context alerts before the latest self-heal timestamp.
- [x] Hide stale verification-hint alerts that are already persisted in suppressions.
- [x] Avoid repeated-failure alerts for resolved historical tasks with no active retry/repair lane.
- [x] Add regression tests.
- [x] Run full validation and rebuild release binary.
- [x] Move this plan to completed with evidence.

## Surprises & Discoveries
- The PAR-006 run was already below the prompt-bloat threshold, but watcher still reported older PAR-003 through PAR-005 prompt alerts.
- Persisted stale hint suppressions were honored by verification planning, but the watch report still listed them as alerts.
- `State.senior_dev_repair_order` stores a task id string, not a structured object.

## Decision Log
- Use the latest `efficiency_self_heal.json.generated_at` as a baseline for prompt-cost alerts.
- Keep unparseable timestamps visible instead of accidentally hiding evidence.
- Keep repeated-task alerts only for active retry/repair/queued tasks.

## Context and Orientation
- Watch report assembly: `build_efficiency_watch_report` in `src/app.rs`.
- Self-heal report: `.orchestrator/observations/efficiency_self_heal.json`.
- Stale hint suppression: `.orchestrator/observations/suppressed_verification_hints.json`.
- Tests live in `src/app_tests.rs`.

## Plan of Work
Thread a self-heal timestamp baseline into prompt alert generation, filter already-suppressed stale hints, and make repeated-failure alerts active-work-only. Add focused tests and rerun the validation suite.

## Concrete Steps
1. Add timestamp parsing and baseline helpers.
2. Filter prompt-cost records by baseline.
3. Filter stale hint alerts by persisted suppression fingerprints.
4. Restrict repeated failure alerts to active task lanes.
5. Validate and rebuild.

## Validation and Acceptance
- `cargo test efficiency_watch`
- `cargo test self_heal`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

Acceptance:
- Old prompt-bloat alerts disappear after a self-heal baseline.
- Already suppressed stale hints no longer appear as watch alerts.
- Resolved historical failed tasks do not keep warning forever.
- New post-baseline inefficiencies still alert.

## Idempotence and Recovery
This is read-only against target telemetry except normal watch report output. Delete `efficiency_self_heal.json` to inspect pre-baseline history again.

## Artifacts and Notes
Focused validation:
- `cargo test efficiency_watch`: passed, 5 tests.
- `cargo test self_heal`: passed, 3 tests.
- `cargo fmt -- --check`: passed.
- `cargo test`: passed, 166 tests.
- `cargo build --release`: passed.
- Live `target/release/codex-team watch --repo /Users/andrewzimmerman/Downloads/heliconfocus --self-heal`: passed with `alerts: none` and no self-heal actions required.

## Interfaces and Dependencies
No CLI changes.

## Outcomes & Retrospective
Watcher alerts are now baseline-aware. Once `watch --self-heal` has handled prompt bloat or stale verification hints, later watcher runs no longer keep reporting the same historical issues. Resolved task failures without an active retry/repair/queued lane also stop surfacing as current inefficiencies. The release binary at `target/release/codex-team` was rebuilt and a live Helicon watch returned no alerts.
