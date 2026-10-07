# INT-018 — Structured Interaction Engine

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-09

## Goal

Replace marker-string questions and synthetic user replies with typed, stable-ID interaction requests that resolve the originating tool call exactly once through a dedicated TUI modal.

## Non-Goals

- General plan-artifact approval (`PLN-023`) or workspace/security prompts (`WSP-020`).
- Exposing masked input to model-visible tools or persisting host secrets.
- Cursor parity claims; behavior is a `ca extension` while `PAR-005` remains blocked.

## Evidence and Current Behavior

- `ask_question` returns a `[QUESTION]` marker from `src/agent/tools.rs`.
- `src/main.rs` scans completed output for that marker, writes `pending_question: Option<String>`, and appends `User response: ...` as a synthetic user message.
- Question input reuses `AppState.input`, so composer/queue drafts are not isolated.
- Approval and question batches are discovered through separate asynchronous events, so mixed-call order is not an explicit invariant.

## Scope

Allowed files: new `src/interaction.rs`, `src/app.rs`, registry/tool/orchestration paths, TUI input/render and snapshots, tests, and harness docs.

Forbidden: oracle/reference trees, user sessions/config/credentials, installed binary, live TUI, broad approval redesign beyond ordered mixed resolution.

Runtime state affected: deterministic test states only.

## Design and Invariants

- `request_user_input` is the only advertised model tool; `ask_question` remains an unadvertised compatibility alias.
- `InteractionId`, request, state, and resolution are typed. Model requests support free text, single choice, multi-select, confirmation, descriptions, numbered shortcuts, cancellation, and optional Other within a nine-visible-choice cap.
- The modal owns its own draft/cursor/selection; composer, queue, kill ring, history, and canonical user messages are untouched.
- Manual resolutions (approvals and interactions) share one ordered coordinator. Pre-resolved tools may execute, but provider continuation occurs only after every manual item resolves.
- Answers write exactly one result onto the originating assistant tool call. Stale IDs are no-ops.
- Masked requests are host-only. Secret bytes/chars are zeroized and cannot be serialized, logged, queued, or converted to model result text.
- Headless recognizes interaction capability and exits 4 without execution.

## Milestones

### M1 — Typed model and pure state machine
- Add red tests for parsing/validation, IDs, all modes, shortcuts, Other, cancellation, stale resolution, ordering, and secret redaction/zeroization.

### M2 — Registry and orchestration
- Advertise canonical tool, retain alias, classify interactions before execution, and introduce one ordered manual-resolution batch.
- Remove marker scanning and synthetic user responses.

### M3 — Dedicated TUI modal
- Give interaction input precedence, render wide/narrow modal states, and prove composer/queue isolation.

### M4 — Validation
- Run focused/full/format/Clippy/release/help/version gates and reconcile harness docs.

## Acceptance Criteria

- Every answer/cancel resolves the exact original call ID once and provider continuation waits for the complete mixed batch.
- Single/multi/confirmation/free-text/Other and shortcuts behave deterministically with at most nine visible choices.
- No interaction answer appears as a `Role::User` message.
- Headless exits 4 for canonical and compatibility interaction calls.
- Secret values are inaccessible to serialization/model result/session/history/queue/kill-ring paths and zeroized on consumption/drop.
- Reviewed wide/narrow snapshots cover choice and free-text states.

## Validation

Focused: PASS — 5 pure interaction tests, 3 orchestration/ordering tests, registry/headless interaction coverage, and 4 reviewed snapshots.

Completion: PASS — 199 unit tests plus 15 CLI integration tests; format, strict Clippy, release, help, and version all pass.

Manual/visual: deterministic wide/narrow snapshots; no live TUI interaction.

Blocked checks: clean Cursor comparison unavailable under `PAR-005`.

## Progress

- [x] Traced marker, modal, approval batch, render, and continuation paths.
- [x] Added typed state-machine and secret tests.
- [x] Integrated registry and ordered orchestration.
- [x] Added modal input/render snapshots.
- [x] Completed validation and harness updates.

## Decisions

- Decision: coordinate approval and interaction prompts in one ordered queue while retaining the existing approval executor for each approval item.
  Evidence: mixed tool-call order cannot be preserved by independently emitted approval/question events.
  Consequence: ordinary tool results may be precomputed, but provider continuation is centralized after the manual queue is empty.

## Surprises and Discoveries

- Preserving mixed-call order requires deferring otherwise automatic tool execution after the first unresolved approval/interaction; the ordered coordinator now represents those later executions explicitly.
- `AppState`'s blanket `Clone` and derived secret-draft debug output would have weakened host-secret guarantees; `AppState` is no longer cloneable and masked drafts render as `[REDACTED]` in debug output.

## Recovery and Rollback

Inverse-patch only listed files; no persisted state migration is involved.

## Completion Record

Changed files: new `src/interaction.rs`, `src/app.rs`, `src/main.rs`, `src/headless.rs`, registry/tool files, TUI renderer/tests/snapshots, and harness docs.

Commands and results: focused interaction/orchestration/registry/render suites PASS; `cargo test` PASS (199 + 15); format, strict Clippy, release, help, and version PASS; zero pending snapshots.

Remaining risks: host masked requests have a safe state/handle API but no product consumer yet; restored unresolved legacy transcripts still rely on normal protocol restoration rules.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, and `TESTING.md` record interaction ownership and actual evidence.
