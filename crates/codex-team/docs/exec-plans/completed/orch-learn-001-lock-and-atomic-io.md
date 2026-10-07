# ORCH-LEARN-001: Repo Lock And Atomic IO Hardening

## Purpose / Big Picture
Prevent overlapping high-risk orchestrator runs from racing `.orchestrator` state, and move app text/JSONL writes behind reusable Rust filesystem helpers.

## Progress
- [x] Inspect ORCH-main lock/IO patterns and current `codex-team` ownership seams.
- [x] Add `.orchestrator/codex-team.lock` path derivation.
- [x] Add Rust-owned lock acquisition, stale recovery, corrupt-lock refusal, and token-safe release.
- [x] Add atomic text writes and bounded JSONL append helpers.
- [x] Wire `step`, `run`, and `watch-loop` through the repo lock.
- [x] Run focused and full validation, with release rebuild explicitly deferred by the user.
- [x] Move this plan to `docs/exec-plans/completed/`.

## Surprises & Discoveries
- Source evidence contradicted the planning assumption that `status` is read-only: `status` refreshes state and calls `save_state`. This pass leaves it unlocked because the approved scope is high-risk mutating runs only, and tracks the broader policy as `ORCH-LEARN-002`.

## Decision Log
- Use `.orchestrator/codex-team.lock` as the lock path so the lock is repo-local and product-specific.
- Acquire locks before config/state loading for `step`, `run`, and `watch-loop`; lower-level cycle helpers assume the caller owns locking.
- Use direct `kill -0 <pid>` without a shell for live PID checks, avoiding a new dependency.
- Refuse corrupt locks instead of deleting them; a human should confirm no active process before manual removal.
- Keep existing `app.rs` helper names and route them through `src/fs_util.rs` to avoid high-churn callsite edits.

## Context and Orientation
- `src/paths.rs` owns `.orchestrator` path derivation.
- `src/lock.rs` owns repo lock metadata and guard behavior.
- `src/fs_util.rs` owns shared read/write/JSONL helpers.
- `src/app.rs` owns command-level orchestration and keeps thin wrappers for existing callsites.
- `src/app_tests.rs` covers app-level lock refusal and `watch-loop` dedup behavior.

## Plan of Work
Add the lock path and module, then wire only `step`, `run`, and `watch-loop` at their public app entrypoints. Implement atomic file replacement and JSONL append truncation in `fs_util`, then keep the existing app helper names as wrappers. Add focused tests beside the new modules plus app integration tests for command-boundary behavior.

## Concrete Steps
1. Add `lock` to the library module list and add `Paths::lock`.
2. Implement `RepoLockTarget`, `RepoLockGuard`, `acquire_repo_lock`, and `acquire_repo_locks`.
3. Implement `read_text`, `write_text_atomic`, and `append_jsonl_bounded` in `src/fs_util.rs`.
4. Route `app.rs` `read_text`, `write_text`, and `append_jsonl` through `src/fs_util.rs`.
5. Acquire the repo lock in `step` and `run_cycles`; acquire sorted/deduplicated repo locks in `watch_loop`.
6. Add module and app tests for lock and IO behavior.
7. Update `NEXT_STEPS.md`, `CONTINUITY.md`, and `TESTING.md` with results.

## Validation and Acceptance
- Passed: `cargo test lock` - 15 tests.
- Passed: `cargo test fs_util` - 5 tests.
- Passed: `cargo test watch_loop` - 12 tests.
- Passed: `cargo test human_disposition` - 10 tests.
- Passed after `cargo fmt`: `cargo fmt -- --check`.
- Passed: `cargo test` - 234 unit tests plus zero-test main/doc targets.
- Skipped by explicit user instruction: `cargo build --release`; `target/release/codex-team` was not rebuilt because the existing runtime was still running.
- Passed: `git diff --check`.

## Idempotence and Recovery
Lock acquisition is idempotent for dead owners and conservative for live/corrupt owners. If a process crashes, rerunning `step`, `run`, or `watch-loop` should recover a dead-PID lock automatically. If the lock file is corrupt, remove `.orchestrator/codex-team.lock` manually only after confirming no `codex-team` process is active.

Atomic writes use same-directory temp files and clean temp files on write failures. If validation fails later, adjust only the ORCH-LEARN-001 files listed in `NEXT_STEPS.md`, then rerun the focused failing command before full validation.

## Artifacts and Notes
- ORCH-main references were used as design evidence only; MCP, memory, worktree, and TUI product shape were intentionally not copied.
- Process-group lifecycle hardening remains a later task, not part of this pass.
- A broader lock/read-only policy pass is tracked as `ORCH-LEARN-002`.

## Interfaces and Dependencies
- New runtime artifact: `.orchestrator/codex-team.lock`.
- No new Cargo dependency.
- No public CLI flags or serialized state schema changes.
- JSONL entries remain valid JSON values; oversized string values are shortened with a truncation marker.

## Outcomes & Retrospective
`step`, `run`, and `watch-loop` now fail fast under a live repo lock and recover dead-PID locks conservatively. Existing app file writes route through centralized IO helpers, reducing direct `fs::write`/JSONL append behavior in the conductor without broad callsite churn. The release binary was intentionally not rebuilt in this pass.
