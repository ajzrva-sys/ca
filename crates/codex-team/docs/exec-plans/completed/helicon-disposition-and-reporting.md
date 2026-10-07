# Helicon Disposition And Adoption Reporting

## Purpose / Big Picture

Add a non-interactive way to answer a human-disposition pause and make dirty adoption reports distinguish user-owned baseline dirt from orchestrator-created adoption scaffolding. This lets API/Codex-driven runs stop safely, record an explicit human decision, and continue only on a later command.

## Progress

- [x] Inspect existing CLI, state, dirty adoption, and init behavior.
- [x] Add a `disposition` CLI subcommand.
- [x] Reuse existing Rust disposition state transitions without calling roles.
- [x] Classify dirty adoption report entries by ownership.
- [x] Add focused regression tests.
- [x] Update docs and validation notes.
- [x] Run focused and full validation.
- [x] Move this plan to completed with outcomes.

## Surprises & Discoveries

- The interactive prompt already maps `preserve`, `recheck`, `gate`, and stop choices to typed state transitions.
- `init_repo` creates missing root docs and `.orchestrator/**` before the adoption pause, so reports need explicit scaffold classification.

## Decision Log

- Add a thin CLI wrapper rather than a parallel state machine, because `apply_human_disposition_choice` already owns the state transition contract.
- Keep `preserve` and `keep-paused` terminal for the current invocation; `recheck` and `gate` only set routing flags for the next explicit `step`.
- Record root files actually created by `init` in `.orchestrator/observations/adoption_scaffold.json`; classify only those recorded files and `.orchestrator/**` as `orchestrator_init` rather than guessing about preexisting user docs.

## Context and Orientation

- CLI dispatch: `src/cli.rs`.
- Conductor state and dirty adoption report: `src/app.rs`.
- Focused tests: `src/app_tests.rs`.
- User-facing command documentation: `README.md`, `TESTING.md`, `CONTINUITY.md`, `NEXT_STEPS.md`.

## Plan of Work

Add `codex-team disposition --repo <path> --decision preserve|recheck|gate|keep-paused`. The app entry point loads state, checks for an active pause except for `keep-paused`, applies the existing transition helper, saves state, and prints status. Dirty adoption reports will add a classification summary and grouped capped examples so operators can see which files are user-owned versus orchestrator scaffolding.

## Concrete Steps

1. Patch `src/cli.rs` with a `Disposition` subcommand and `DispositionDecisionArg` value enum.
2. Patch `src/app.rs` with a public `disposition` wrapper and a non-interactive choice mapper.
3. Patch dirty adoption rendering with `AdoptionReportClass` and grouped report sections.
4. Add tests for CLI-style decisions and report classification.
5. Update docs with command usage, validation results, and the new `ORCH-HEL-003` task status.
6. Run validation commands from `TESTING.md`.

## Validation and Acceptance

- `cargo test human_disposition`
- `cargo test dirty_tree`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

Acceptance is met when non-interactive disposition records durable state/events, report classification is visible and tested, and the release binary is rebuilt.

## Idempotence and Recovery

`keep-paused` leaves state unchanged. Re-running `preserve`, `recheck`, or `gate` after the pause is cleared should fail with a clear message rather than inventing a decision. If validation fails, keep edits local and rerun the focused test after correcting the smallest owning subsystem.

## Artifacts and Notes

- No Helicon product files should be changed by this task.
- Do not commit or stage unless explicitly requested.

## Interfaces and Dependencies

- New CLI: `codex-team disposition --repo <path> --decision <decision>`.
- No new dependencies.
- Existing `.orchestrator/state.json` and `.orchestrator/events.jsonl` schemas are reused.

## Outcomes & Retrospective

- Added `codex-team disposition --repo <path> --decision preserve|recheck|gate|keep-paused`.
- The command reuses existing Rust disposition transitions, saves state/events, prints status, and does not call PM, CEO, or Worker.
- Dirty adoption reports now include `Counts By Adoption Class` and grouped capped examples for `user_baseline`, `orchestrator_init`, and `generated_or_ignored`.
- `init` records root docs it created in `.orchestrator/observations/adoption_scaffold.json` for report classification.
- Validation passed: `cargo test human_disposition`, `cargo test dirty_adoption_report_classifies`, `cargo test dirty_tree`, `cargo fmt -- --check`, `cargo test`, `cargo build --release`, `target/release/codex-team disposition --help`, and `git diff --check`.
