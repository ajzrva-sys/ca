# APR-003 — Deterministic Multi-Tool Approval State Machine

Status: COMPLETE
Owner/session: Hermes coding session
Last updated: 2026-08-01

## Goal

Every approval-requiring tool call in one assistant response receives exactly one ordered decision and coherent result, and the provider loop continues exactly once only after the whole batch resolves.

## Non-Goals

- No oracle modifications or source copying.
- No provider, tool-schema, config-format, or headless behavior redesign.
- No new dependencies.
- No broad formatting or unrelated lint cleanup.

## Evidence and Current Behavior

- `src/app.rs` stores one `Option<PendingApproval>`.
- `src/main.rs::execute_pending_tools_async` can emit multiple approval messages, while the receive handler overwrites that single slot.
- The key handler clears the slot and starts the next LLM turn after one decision.
- Baseline `cargo test` on 2026-08-01: PASS, 27 tests.
- Detailed source analysis and the approved implementation sequence are in `.hermes/plans/2026-08-01_000000-apr003-approval-state-machine.md`.

## Scope

Allowed files:
- `src/app.rs`, `src/main.rs`, `src/approval.rs`
- approval-specific `src/tui/render.rs`, `src/tui/render_tests.rs`, and snapshots
- `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`
- this ExecPlan and the approved `.hermes` plan

Forbidden files/areas:
- `oracles/`, `reference/`
- runtime config/session files, credentials, installed binaries
- unrelated source and broad formatting

Runtime state affected: none during automated tests. Persistent-rule key paths must be tested without writing user config.

## Design and Invariants

- Model approvals as an ordered batch with explicit current and remaining items.
- Classify persistent allow/deny rules before prompting.
- Never represent an unresolved approval as a completed empty tool result.
- Approved calls execute at most once; denied calls receive an explicit denial result.
- Resolving one item advances to the next; only batch exhaustion emits one continuation signal.
- Keep tool execution asynchronous. Do not add `block_on`, `block_in_place`, or synchronous long work to the TUI event loop, even where the earlier `.hermes` plan suggested retaining the old blocking helper.
- Cancellation/denial leaves every affected call and queue counter consistent.

## Milestones

### M1 — Queue model and pure state tests
- Add failing focused tests first for ordering, activity, progress, exhaustion, and reset.
- Add the minimal queue model to pass.
- Validation: focused approval tests and compile check.

### M2 — Batch classification and async decision orchestration
- Add failing tests for mixed pre-resolved/pending calls and no silent empty outputs.
- Populate one batch atomically.
- Route decisions through async execution without blocking the event loop.
- Prove exactly-once continuation and coherent cancellation/error results.

### M3 — Visible progress and render snapshots
- Render `[N/M]` for the current item.
- Add wide/narrow deterministic snapshots and review diffs.

### M4 — Review, completion gates, and harness reconciliation
- Fresh read-only spec review, then fresh read-only code-quality review.
- Address valid findings through RED→GREEN.
- Run focused tests, full suite, targeted formatting, repository format baseline, strict clippy baseline, release build, help smoke, and zero-pending-snapshot check.
- Update harness docs and move this plan to completed.

## Acceptance Criteria

- No pending call is skipped, duplicated, overwritten, or serialized as a silently empty completed result.
- Persistent allow/deny policy is applied before prompting.
- Mixed batches preserve assistant call order.
- Each decision advances exactly one item; provider continuation occurs once after exhaustion.
- Denial, cancellation, and execution error produce internally coherent explicit results.
- Tests require no network, shell side effects, credentials, user config, or user session files.
- Approval prompt has reviewed wide/narrow deterministic coverage.

## Validation

Focused:
- `cargo test approval::tests -- --nocapture`
- approval orchestration test names added during M2
- `cargo test tui::render_tests -- --nocapture`

Completion:
- `cargo test`
- targeted `rustfmt --check` for touched Rust files
- `cargo fmt --check` (record known baseline separately)
- `cargo clippy --all-targets -- -D warnings` (record known baseline separately)
- `cargo build --release`
- `./target/release/ca --help`
- zero `*.snap.new`

Manual/visual:
- Review generated text snapshots at 100×30 and 60×24.
- No live provider claim unless explicitly run.

Blocked checks:
- Clean Cursor parity observation is outside APR-003 unless a conflict requires it.

## Progress

- [x] Baseline source trace and 27-test suite recorded.
- [x] M1 queue model and pure tests complete.
- [x] M2 async batch orchestration complete.
- [x] M3 render progress snapshots reviewed.
- [x] M4 two-stage review and completion gates complete.

## Decisions

- Decision: follow one-writer-per-file delegation; dependent slices run sequentially.
  Evidence: `src/main.rs` owns classification, event handling, and continuation, so parallel writers would overlap.
  Consequence: fresh implementation workers are dispatched per bounded milestone, not concurrently.
- Decision: the harness async-responsiveness guard overrides the old plan’s suggestion to retain nested runtime blocking.
  Evidence: `AGENTS.md` explicitly forbids `block_on` and `block_in_place` in the TUI event loop.
  Consequence: M2 must introduce an asynchronous decision execution path.

## Surprises and Discoveries

- The approved `.hermes` plan exists, but the harness also requires an active ExecPlan under `docs/exec-plans/active/`; this file reconciles that requirement.
- The repository has no committed project baseline; verification relies on current files and command output rather than Git history.
- Parent verification caught and corrected two worker false-green claims: cancellation initially cleared `current` without advancing, and duplicate persistent denials initially lost their `Error` status through the result cache.
- Strict clippy exposed four new APR-003 diagnostics; those were fixed, leaving only the same four documented baseline diagnostics.

## Recovery and Rollback

Do not use destructive Git operations. If a delegated slice conflicts, stop writers, re-read the current files, apply targeted patches only, and rerun its original RED/GREEN commands.

## Completion Record

Changed files: `src/app.rs`, `src/main.rs`, `src/approval.rs`, `src/tui/render.rs`, `src/tui/render_tests.rs`, six approval snapshots, and harness/plan docs.
Commands and results: focused approval 45 PASS; duplicate-denial regression 1 PASS; render 9 PASS; full suite 71 PASS; release build and help smoke PASS; 14 accepted snapshots and 0 pending. Repository-wide format remains red on documented drift. Strict clippy reports only the same 4 documented baseline diagnostics.
Remaining risks: live provider and manual Cursor parity were not claimed; async responsiveness outside the approval path remains tracked separately.
Harness updates: `CONTINUITY.md`, `NEXT_STEPS.md`, and `TESTING.md` record APR-003 completion and actual validation.
