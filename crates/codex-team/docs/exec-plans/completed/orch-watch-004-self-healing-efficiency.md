# ORCH-WATCH-004: Self-Healing Efficiency Watch

## Purpose / Big Picture
The watcher should not require the operator to paste telemetry into a separate Codex thread for routine efficiency fixes. Add an opt-in self-heal mode that applies safe Rust-owned mitigations directly in the target repo's `.orchestrator` data.

## Progress
- [x] Identify safe self-heal actions.
- [x] Add `codex-team watch --self-heal`.
- [x] Persist stale verification-hint suppressions.
- [x] Auto-switch Project Manager config back to compact mode when prompt bloat is detected.
- [x] Write a self-heal report.
- [x] Add regression tests.
- [x] Run validation and rebuild release binary.
- [x] Move this plan to completed with evidence.

## Surprises & Discoveries
- The existing stale-hint suppression reads failed evidence dynamically, but a durable suppression file will make the mitigation visible and independent from old evidence retention.
- Prompt bloat can only be auto-healed safely at target-repo config level when the repo is not already using compact planner context. Deeper code-level prompt fixes still require orchestrator source changes.

## Decision Log
- Self-heal is opt-in via `watch --self-heal` for this slice.
- Self-heal may edit only `.orchestrator/config.toml` and `.orchestrator/observations/*`.
- Self-heal must not approve work, enqueue tasks, run roles, commit, stage, revert, or edit product/control docs.

## Context and Orientation
- CLI watch subcommand: `src/cli.rs`.
- Watch app entry point: `pub fn watch` in `src/app.rs`.
- Efficiency report: `build_efficiency_watch_report`.
- Stale hint suppression: `stale_observation_hint_command_fingerprints`.
- Config default: `planner_context = "compact"`.

## Plan of Work
Extend watch with `--self-heal`, build a self-heal report from current watcher alerts, and apply only mechanical mitigations. Materialized stale verification hints will be read by verification planning. Prompt bloat will switch `planner_context` from `full` to `compact` when needed.

## Concrete Steps
1. Add `--self-heal` CLI flag and pass it to `app::watch`.
2. Add suppression-file structs and readers/writers.
3. Include suppression-file fingerprints in verification-plan hint skipping.
4. Add self-heal action/report rendering.
5. Add tests for config compaction and stale-hint suppression persistence.
6. Validate and rebuild.

## Validation and Acceptance
- `cargo test self_heal`
- `cargo test efficiency_watch`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

Acceptance:
- `codex-team watch --self-heal` writes `.orchestrator/observations/efficiency_self_heal.md` and `.json`.
- Prompt bloat with `planner_context = "full"` rewrites config to compact.
- Failed observation hints are persisted to a suppression file and skipped later.
- Self-heal does not touch product files, queues, commits, staging, or role execution.

## Idempotence and Recovery
The self-heal pass is idempotent. Re-running it keeps compact mode compact and merges existing stale-hint suppressions. To recover, edit `.orchestrator/config.toml` back to `planner_context = "full"` or delete `.orchestrator/observations/suppressed_verification_hints.json`.

## Artifacts and Notes
This is the first automatic maintenance lane. It is intentionally small so future self-heal actions can be added behind tests.

Focused validation:
- `cargo test self_heal`: passed, 2 tests.
- `cargo test persisted_suppressed_observation_hint`: passed, 1 test.
- `cargo test efficiency_watch`: passed, 3 tests.
- `cargo fmt -- --check`: passed.
- `cargo test`: passed, 164 tests.
- `cargo build --release`: passed.

Live Helicon smoke:
- `target/release/codex-team watch --repo /Users/andrewzimmerman/Downloads/heliconfocus --self-heal`: passed.
- Result: compact planner context was already active; self-heal wrote two stale verification-hint suppressions for `BenchmarkSuiteTests` and `TiledRendererTests`.

## Interfaces and Dependencies
- Adds a CLI flag: `watch --self-heal`.
- Adds target observation files:
  - `.orchestrator/observations/suppressed_verification_hints.json`
  - `.orchestrator/observations/efficiency_self_heal.md`
  - `.orchestrator/observations/efficiency_self_heal.json`

## Outcomes & Retrospective
`codex-team watch --self-heal` now applies the first safe automatic maintenance actions. It restores compact planner mode when prompt-bloat alerts occur under full planner context, persists stale verification-hint suppressions, writes a self-heal report, and keeps the mitigation lane limited to Rust-owned `.orchestrator` config/observation data. The release binary at `target/release/codex-team` was rebuilt.
