# ORCH-ROLE-COMMIT-001: Role-Authorized Rust Commits

## Purpose / Big Picture
PM and Senior Dev can now authorize commits as typed Rust work instead of asking a human to manually commit or stash bounded dirty slices. AI roles still do not run `git` directly. Rust owns staging, verification, commit execution, event recording, and route feedback.

## Progress
- [x] Added typed PM commit requests.
- [x] Added Senior `rust_commit_job` progress actions.
- [x] Added one Rust-owned commit executor.
- [x] Updated role prompts, schemas, and architecture docs.
- [x] Added focused tests for selected-path commits, rejection, verification failure, and direct git-command refusal.
- [x] Re-ran full validation after the final docs update.

## Surprises & Discoveries
- The existing PM control-doc reconciliation auto-commit path remains valid and is separate from the new typed role-authorized commit lane.
- The first implementation is path-scoped. It stages only explicitly allowed dirty paths, but it does not split hunks inside a mixed file.
- Verification commands need a stricter cleanliness check than general task verification: a `noMatches` diagnostic result, such as an `rg` command with no output, is not enough to authorize a commit.

## Decision Log
- Keep Rust as the only commit executor because this preserves the repository boundary that AI role output is advisory until converted into typed Rust state or actions.
- Permit unrelated dirty files to remain unstaged because current recovery needs to commit ORCH-LEARN and CHURN-style slices independently.
- Run `git diff --check -- <selected>` before requested deterministic verification so whitespace errors stop before commit creation.
- Defer rebuilding `target/release/codex-team` while the user has a running release process; rebuild it once that process is finished so the direct-run binary includes this change.

## Context and Orientation
- PM planner output is parsed in `src/app.rs` and validated by `schemas/planner.schema.json`.
- Senior progress output is parsed in `src/app.rs` and validated by `schemas/foreman_batch.schema.json`.
- Deterministic verification command parsing and execution already exist in Rust and are reused by the commit executor.
- Existing PM control-doc auto-commit tests live in `src/app_tests.rs` near the PM mutation tests.

## Plan of Work
Add a shared `RustCommitRequest` contract, route PM `commit_requests` and Senior `rust_commit_job` actions into one executor, and teach the executor to select only allowed dirty paths, verify them, stage them explicitly, and commit with the requested message. Update prompts and docs so the new rule is precise: AI roles may authorize typed commits but never run `git` directly.

## Concrete Steps
1. Extend PM and Senior schemas and Rust parsing structs.
2. Add a single commit executor in `src/app.rs`.
3. Add PM and Senior routing from typed outputs into the executor.
4. Add focused tests for success, disallowed/direct-git rejection, verification failure, and unrelated dirty-file preservation.
5. Update README, architecture, quality, prompt, continuity, testing, and next-step ledgers.

## Validation and Acceptance
- Passed: `cargo test rust_commit`
- Passed: `cargo test pm_commit_request`
- Passed: `cargo test foreman_progress`
- Passed: `cargo test planner_work_plan`
- Passed: `cargo test watch_loop`
- Passed: `cargo test pm_control_doc`
- Passed: `cargo test pm_mutated`
- Passed: `cargo fmt -- --check`
- Passed: `cargo test`
- Passed: `git diff --check`
- Passed after the attached watch-loop finished: `cargo build --release`

Acceptance criteria:
- PM commit requests commit only selected allowed dirty files and leave unrelated dirty files unstaged.
- PM commit requests refuse disallowed or unverifiable selected work and create no commit.
- Senior `rust_commit_job` commits selected source/docs changes only after deterministic verification passes.
- Failed verification records a rejection event and creates no commit.
- Direct `git add` or `git commit` requests remain rejected outside the Rust commit executor.

## Idempotence and Recovery
If a commit request fails, Rust records a rejection event and leaves the worktree dirty for later reconciliation. Re-running the same request after fixing the dirty scope or verification command is safe because the executor recalculates selected dirty paths each time and refuses when nothing selected is dirty.

## Artifacts and Notes
- The current worktree also contains preserved ORCH-LEARN-001 and CHURN-related dirty files. This plan does not attempt to commit or revert them.
- Hunk-level staging is intentionally left for future work if mixed-file recovery becomes common.

## Interfaces and Dependencies
- PM JSON schema: `schemas/planner.schema.json`
- Senior JSON schema: `schemas/foreman_batch.schema.json`
- Prompts: `prompts/planner.md`, `prompts/foreman_batch.md`, `prompts/agents.md`
- Rust integration and tests: `src/app.rs`, `src/app_tests.rs`

## Outcomes & Retrospective
Role-authorized commits are now represented as typed Rust requests. Watcher/self-heal can process PM or Senior commit authorizations non-interactively when guard and verification pass, while true human-disposition pauses still stop. The next useful refinement is hunk-scoped selection or a preflight warning when an allowed file contains mixed unrelated edits.
