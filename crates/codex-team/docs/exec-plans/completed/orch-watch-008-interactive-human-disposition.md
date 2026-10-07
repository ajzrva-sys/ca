# ORCH-WATCH-008: Interactive Human Disposition In Watch Loop

## Purpose / Big Picture
`watch-loop` should not fail closed without asking when it is running in an interactive terminal and the target repo needs a human disposition. It should reuse the existing human-disposition prompt, record the typed decision, and continue only when the selected disposition permits continuation. Non-interactive/API use must remain safe and stop without blocking.

## Progress
- [x] Root cause and intended contract recorded.
- [x] Watch-loop prompt mode added.
- [x] Non-interactive stop behavior preserved.
- [x] Focused tests and full validation completed.
- [x] Release binary rebuilt.

## Surprises & Discoveries
- `step` already has `HumanDispositionMode::Prompt`; `watch-loop` bypassed it by checking `state.human_disposition_required` before calling `step_once_with_runner_mode`.

## Decision Log
- Use terminal detection to choose prompt mode for `watch-loop`: interactive stdin prompts; non-interactive stdin stops.
- Keep disposition semantics unchanged: `preserve` stops, `recheck` and `gate` clear the pause and make PM due, `keep-paused` leaves the pause active.

## Context and Orientation
- Main loop: `src/app.rs::watch_loop`.
- Supervised target step helper: `src/app.rs::run_one_supervised_cycle`.
- Existing prompt and semantics: `prompt_human_disposition_choice`, `apply_human_disposition_choice`.
- CLI surface: `src/cli.rs::Commands::WatchLoop`.

## Plan of Work
Parameterize supervised cycles by human-disposition mode. `watch-loop` will choose `Prompt` only when stdin is an interactive terminal. The existing `Stop` mode remains available for tests and non-interactive execution. Add tests around the helper behavior so the watcher does not accidentally block API workflows.

## Concrete Steps
1. Add `stdin` terminal detection for watch-loop.
2. Split `run_one_supervised_cycle` into a default wrapper and a mode-aware helper.
3. Preserve the current early pause return only in `Stop` mode.
4. Add focused tests for prompt/recheck continuation without launching roles and existing stop behavior.
5. Run validation.

## Validation and Acceptance
- `cargo test watch_loop`
- `cargo test human_disposition`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

Acceptance: an interactive watch-loop can prompt for a human disposition instead of stopping immediately; non-interactive watch-loop still reports `target_human_disposition_pause`; no disposition semantics or product-file behavior changes.

## Idempotence and Recovery
If interrupted before answering, no disposition is recorded and the existing pause remains active. If answered, the existing disposition persistence path records the decision in state/events.

## Artifacts and Notes
- Validation:
  - `cargo test watch_loop` passed 7 tests.
  - `cargo test human_disposition` passed 10 tests.
  - `cargo fmt -- --check` initially reported formatting drift, then passed after `cargo fmt`.
  - `cargo test` passed 215 unit tests, plus zero-test main/doc targets.
  - `cargo build --release` passed and refreshed `target/release/codex-team`.
  - `git diff --check` passed.

## Interfaces and Dependencies
- Uses `std::io::IsTerminal`; no new dependency.

## Outcomes & Retrospective
`watch-loop` now prompts in interactive terminals when the target repo has an active human-disposition pause, using the same Rust-owned disposition semantics as `step`. Non-interactive loops still stop safely. This should make the supervised loop feel less brittle without allowing unattended automation to invent human policy choices.
