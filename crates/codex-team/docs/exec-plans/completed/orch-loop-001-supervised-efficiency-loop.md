# Supervised Efficiency Loop

## Purpose / Big Picture
Automate the workflow where the orchestrator advances real repo work, the watcher assesses token/time inefficiencies, Rust applies safe local mitigations, and unresolved efficiency work is handed to the orchestrator repo for a high-intelligence repair cycle. The first slice stays sequential and bounded.

## Progress
- [x] State intended contract and safe boundaries.
- [x] Add non-interactive step mode that stops on human-disposition pauses.
- [x] Add `watch-loop` CLI command for target step + watcher/self-heal iterations.
- [x] Persist loop reports and cross-repo efficiency feedback packets.
- [x] Optionally run one bounded orchestrator-repo cycle when unresolved alerts remain.
- [x] Add focused tests and validation.
- [x] Rebuild release binary.

## Surprises & Discoveries
- Existing watch mode is intentionally advisory plus Rust-owned self-heal only.
- Existing `step_once` prompts on active human-disposition pauses; supervised automation needs a non-interactive stop path.

## Decision Log
- Keep the first implementation single-worktree and sequential.
- Do not let watcher edit product files directly.
- Do not run parallel Workers in live worktrees.
- Use the orchestrator repo itself for high-intelligence efficiency fixes, via normal `codex-team` routing, rather than embedding an ad hoc model-edit path inside watch mode.

## Context and Orientation
- `src/cli.rs` owns subcommands.
- `src/app.rs::watch` builds the current efficiency report and Rust self-heal.
- `src/app.rs::step_once_with_runner` owns one orchestration cycle.
- `.orchestrator/observations/efficiency_watch.*` and `efficiency_self_heal.*` already exist.

## Plan of Work
Add a supervised `watch-loop` command. Each iteration runs at most one non-interactive target `step`, then writes a watcher report and optional Rust self-heal report. If alerts remain and an orchestrator repo was supplied, write an external efficiency feedback packet into that orchestrator repo, set Planner due there, and run at most the configured number of non-interactive orchestrator cycles. Stop when the target watch report is clean, a human-disposition pause is active, max cycles are reached, or no orchestrator repo was supplied for unresolved alerts.

## Concrete Steps
1. Add `HumanDispositionMode` around `step_once_with_runner`.
2. Add reusable helpers for one non-interactive supervised cycle.
3. Add loop report and feedback packet structs/rendering.
4. Add `watch-loop` CLI flags.
5. Add tests for non-interactive pause, feedback packet persistence, and clean-loop stopping.
6. Update docs and validation notes.

## Validation and Acceptance
- `cargo test watch_loop`
- `cargo test efficiency_watch`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

## Idempotence and Recovery
Feedback packets are append-only observations. Re-running the loop can produce another packet and event, but each run is bounded by `--max-cycles` and `--orchestrator-cycles`.

## Artifacts and Notes
This implementation does not run commands in Helicon. The command is for future use by the user.

## Interfaces and Dependencies
- New CLI: `codex-team watch-loop --repo <target> --orchestrator-repo <codex_team_orchestrator> --max-cycles N --self-heal`.
- Target loop reports live under `.orchestrator/observations/efficiency_loop.*`.
- Orchestrator feedback packets live under `.orchestrator/observations/external_efficiency_feedback/`.

## Outcomes & Retrospective
Implemented as a bounded, sequential loop. The command advances the target repo one non-interactive step at a time, runs watcher/self-heal, writes unresolved alerts as external efficiency feedback for the orchestrator repo, marks that repo Planner-due, and runs bounded normal orchestrator cycles there. If the orchestrator repo is missing `.orchestrator/config.toml`, `watch-loop` initializes that side with `--orchestrator-mission` or its default mission. If the orchestrator-repair side is paused or has no runnable cycle, `watch-loop` stops instead of continuing to spend target-repo iterations. It does not prompt through human-disposition pauses or run parallel live-worktree Workers.

Validation so far:
- `cargo test watch_loop`: passed 5 tests.
- `cargo test efficiency_watch`: passed 9 tests.
- `cargo fmt -- --check`: passed.
- `cargo test`: passed 192 tests.
- `cargo build --release`: passed.
