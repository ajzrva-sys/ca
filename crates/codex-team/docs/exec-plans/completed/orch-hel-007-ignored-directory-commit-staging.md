# ORCH-HEL-007 Ignored Directory Commit Staging

## Purpose / Big Picture
Fix Rust-owned accepted-task commits when a guard report includes a broad directory path whose parent is ignored by the target repository. The user-visible outcome is that a Senior-accepted Helicon task can be committed when the actual changed files are tracked and allowed, even if a broad ignored directory such as `Tests/` also appears in guard bookkeeping.

## Progress
- [x] Capture the failure mode from Helicon `PAR-003`.
- [x] Normalize accepted-task commit paths to actual dirty files.
- [x] Add regression coverage for ignored parent directories.
- [x] Run focused and full validation.
- [x] Rebuild `target/release/codex-team`.

## Surprises & Discoveries
Senior accepted `PAR-003`, but Rust-owned commit failed at `git add accepted worker task`. Git reported that `Tests` is ignored, while the actual changed file `Tests/SwiftStackerCoreTests/BenchmarkSuiteTests.swift` was staged successfully. This indicates Rust passed a broad directory path into `git add` instead of only the concrete dirty file.

## Decision Log
- Decision: Expand candidate commit paths through `git status --short` and stage concrete dirty files under broad directory candidates.
  Rationale: The guard boundary remains enforced by allowed-file globs, while Git no longer receives ignored parent directories as staging arguments.

## Context and Orientation
- Commit staging for accepted Worker tasks lives in `src/app.rs` inside `commit_accepted_worker_task`.
- Existing auto-accept tests for source/test slices live in `src/app_tests.rs`.
- The target repo failure involved an ignored parent directory, a tracked modified test file, and a Senior-accepted task.

## Plan of Work
Add a helper that expands candidate staging paths to concrete dirty files. If a candidate is a directory, include dirty files below it that satisfy the allowed glob set. If a candidate is already a file path, keep it. Then use the normalized list for `git add` and `git commit`.

## Concrete Steps
1. Add path-normalization helper in `src/app.rs`.
2. Wire it into `commit_accepted_worker_task`.
3. Add a regression where `.gitignore` ignores `Tests/`, an already-tracked test file changes, and the guard report contains broad `Tests`.
4. Run focused tests, formatting, full tests, release build, and diff whitespace validation.

## Validation and Acceptance
- `cargo test accepted_worker_commit_expands_ignored_directory_to_dirty_files`
- `cargo test rust_auto_accepts_small_verified_source_and_test_slice_when_tier_allows`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

Acceptance: the ignored-directory regression commits successfully without weakening allowed-file enforcement.

## Idempotence and Recovery
The helper is deterministic and read-only until the existing commit path runs. If a target repo is already partially staged from a failed attempt, rerunning the accepted-task path should stage the same concrete dirty files and commit normally.

## Artifacts and Notes
- `cargo test accepted_worker_commit_expands_ignored_directory_to_dirty_files`: first reproduced the failure, then passed after staging tracked paths with `git add -u`.
- `cargo test rust_auto_accepts_small_verified_source_and_test_slice_when_tier_allows`: passed.
- `cargo fmt -- --check`: passed after one mechanical `cargo fmt`.
- `cargo test`: passed, 155 tests.
- `cargo build --release`: passed and refreshed `target/release/codex-team`.

## Interfaces and Dependencies
No CLI, schema, or prompt changes. Uses existing Git status and allowed glob validation.

## Outcomes & Retrospective
Accepted-task commits now normalize candidate paths to concrete dirty files and stage tracked modifications with `git add -u --`. This preserves allowed-file enforcement while avoiding Git failures when a tracked file lives below an ignored parent directory such as `Tests/`.
