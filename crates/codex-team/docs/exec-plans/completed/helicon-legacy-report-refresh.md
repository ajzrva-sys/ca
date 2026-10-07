# Helicon Legacy Report Refresh

## Purpose / Big Picture

Let operators refresh an existing dirty-adoption pause report with the current report classifier without clearing the pause or advancing PM, CEO, or Worker. This is for repos that were initialized before adoption report classification existed.

## Progress

- [x] Inspect current disposition CLI, report renderer, and tests.
- [x] Add `--refresh-report` to `codex-team disposition`.
- [x] Regenerate dirty adoption reports only for active `rust_dirty_adoption` pauses.
- [x] Add `legacy_unknown` classification for ambiguous legacy root docs when scaffold provenance is missing.
- [x] Add focused tests and docs.
- [x] Run focused and full validation.
- [x] Move this plan to completed with outcomes.

## Surprises & Discoveries

- `current_repo_observation_snapshot` intentionally excludes `.orchestrator/**`; refresh should include those files in the refreshed report while preserving the existing pause threshold behavior.

## Decision Log

- Keep refresh as part of `disposition` because it is a human-pause operation and must not run orchestration roles.
- Require an active `rust_dirty_adoption` pause for refresh so unrelated pauses are not rewritten as adoption reports.
- Treat root docs that `init` can create as `legacy_unknown` only when `.orchestrator/observations/adoption_scaffold.json` is missing.

## Context and Orientation

- CLI dispatch: `src/cli.rs`.
- Disposition state and dirty report rendering: `src/app.rs`.
- Focused tests: `src/app_tests.rs`.
- User docs and validation ledger: `README.md`, `TESTING.md`, `CONTINUITY.md`, `NEXT_STEPS.md`.

## Plan of Work

Add `--refresh-report` to the existing disposition command. The app entry point will validate the active pause, regenerate `.orchestrator/observations/dirty_adoption_report.md` from current Git status with `.orchestrator/**` included, append an event, save state, and print status. If no scaffold provenance exists, legacy root control docs are classified separately as `legacy_unknown`.

## Concrete Steps

1. Patch `src/cli.rs` to parse `--refresh-report`.
2. Patch `src/app.rs` to add a refresh helper and include `.orchestrator/**` in refresh snapshots only.
3. Extend `AdoptionReportClass` with `legacy_unknown`.
4. Add tests for refresh preserving pause, rejecting non-adoption pauses, and legacy classification.
5. Update docs and validation records.
6. Run validation commands.

## Validation and Acceptance

- `cargo test human_disposition`
- `cargo test dirty_adoption_report`
- `cargo test dirty_tree`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

Acceptance is met when refresh rewrites the report without clearing the pause, refuses non-adoption pauses, legacy root docs are classified as `legacy_unknown`, and the release binary is rebuilt.

## Idempotence and Recovery

Refreshing the report is idempotent aside from timestamps and current dirty status. It does not stage, commit, revert, quarantine, or clear any pause. If validation fails, keep the pause unchanged and rerun the focused tests after the smallest code correction.

## Artifacts and Notes

- Do not run commands in `/Users/andrewzimmerman/Downloads/heliconfocus` for this implementation.
- Do not stage or commit unless explicitly requested.

## Interfaces and Dependencies

- Updated CLI: `codex-team disposition --repo <path> --decision keep-paused --refresh-report`.
- No new dependencies.

## Outcomes & Retrospective

- Added `codex-team disposition --repo <path> --decision keep-paused --refresh-report`.
- Refresh is accepted only for active `rust_dirty_adoption` pauses and only with `keep-paused`.
- Refresh rewrites `.orchestrator/observations/dirty_adoption_report.md`, includes `.orchestrator/**` in the refreshed report, records an event, saves state, and leaves the pause active.
- Dirty adoption reports now classify ambiguous legacy root control docs as `legacy_unknown` when no adoption scaffold record exists.
- Validation passed: `cargo test human_disposition`, `cargo test dirty_adoption_report`, `cargo test dirty_tree`, `cargo fmt -- --check`, `cargo test`, `cargo build --release`, `target/release/codex-team disposition --help`, and `git diff --check`.
