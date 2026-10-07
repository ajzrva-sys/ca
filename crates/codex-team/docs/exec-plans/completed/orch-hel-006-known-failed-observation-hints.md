# ORCH-HEL-006 Known-Failed Observation Hints

## Purpose / Big Picture
Prevent observation-memory verification hints from reintroducing commands that the target repository's `TESTING.md` already records as known failing. This keeps Rust-owned acceptance verification focused on the task contract instead of turning stale or diagnostic commands into blocking gates.

## Progress
- [x] Identify the failing path from Helicon `PAR-003` verification output.
- [x] Add a `TESTING.md` known-failure filter for observation hints.
- [x] Add regression coverage.
- [x] Run focused and full validation.
- [x] Rebuild `target/release/codex-team`.
- [x] Move this plan to completed with evidence.

## Surprises & Discoveries
Helicon `PAR-003` passed the task's explicit verification commands, then Rust appended `swift test --filter BenchmarkSuiteTests` from observation hints. Helicon's `TESTING.md` already documents that command as failed, so using it as an acceptance gate is negative evidence inversion.

## Decision Log
- Decision: Filter observation hints using target-repo `TESTING.md` failure rows instead of suppressing all hints for implementation tasks.
  Rationale: source/test tasks still benefit from local verification memory; only known-failed commands should be excluded.

## Context and Orientation
- Verification plan assembly lives in `src/app.rs` near `build_verification_plan_with_observation_hints`.
- Observation hint tests live in `src/app_tests.rs` near `verification_plan_uses_local_hints_before_global_hints`.
- Helicon evidence: `PAR-003` task verification passed explicit build/diff commands, then failed on hinted `swift test --filter BenchmarkSuiteTests`.

## Plan of Work
Read `TESTING.md` if present, extract markdown table command rows whose status cell records failure or blocking, parse those commands through the existing deterministic command parser, and skip semantically matching observation hints. Add a regression that keeps valid hints while skipping a known-failed Swift test filter.

## Concrete Steps
1. Add helper functions in `src/app.rs`.
2. Wire the helper into `build_verification_plan_with_observation_hints`.
3. Add a focused unit test in `src/app_tests.rs`.
4. Run `cargo test` filters, formatting, full tests, release build, and diff whitespace validation.

## Validation and Acceptance
- `cargo test verification_plan_skips_testing_known_failed_observation_hint`
- `cargo test verification_plan_uses_local_hints_before_global_hints`
- `cargo test verification`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

Acceptance: known-failed commands from `TESTING.md` are not added from observation hints, valid observation hints still work, and the rebuilt release binary contains the fix.

## Idempotence and Recovery
The filter is read-only against target repositories and only affects candidate verification hints. If a command later becomes valid, updating `TESTING.md` to remove the failed status allows the hint again.

## Artifacts and Notes
- `cargo test verification_plan_skips_testing_known_failed_observation_hint`: passed.
- `cargo test verification_plan_uses_local_hints_before_global_hints`: passed.
- `cargo test verification`: passed, 17 tests.
- `cargo fmt -- --check`: passed after one mechanical `cargo fmt`.
- `cargo test`: passed, 154 tests.
- `cargo build --release`: passed and refreshed `target/release/codex-team`.

## Interfaces and Dependencies
No CLI or schema changes. Uses existing `VerificationCommand` parsing and semantic fingerprints.

## Outcomes & Retrospective
`build_verification_plan_with_observation_hints` now reads target-repo `TESTING.md`, extracts markdown table rows whose status cell records a failure/blocker, and skips semantically matching observation hints. Valid hints still apply. This prevents Helicon `PAR-003` from being blocked by the already documented `swift test --filter BenchmarkSuiteTests` failure while preserving observation-memory coverage for other source tasks.
