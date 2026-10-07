# BUG-235 — Bounded Pre-Output Recovery and Clean Failed-Turn Follow-Ups

Status: COMPLETE (one workspace-level protected-tree gate remains blocked)
Owner/session: Codex implementation session
Last updated: 2026-09-03

## Goal

Make transient provider recovery invisible and bounded before any usable output, while ensuring a normal prompt submitted after a failed user-only turn is sent from a clean immutable child branch instead of producing consecutive user messages in provider history.

## Non-Goals

- Changing CLI flags, configuration, provider wire schemas, or the session format.
- Retrying HTTP 400, protocol errors, malformed streams, or any request after usable output begins.
- Changing explicit `/retry`, partial-response recovery, resolved-tool continuation, provider fallback, or queue semantics.
- Reading provider error bodies, credentials, or runtime configuration.
- Modifying `oracles/`, `reference/`, or installing the release binary.
- Claiming Cursor parity for this CA-specific reliability policy.

## Evidence and Current Behavior

- `src/agent/client.rs::stream_adapter_request` limits response-header transport/status recovery with `ProviderCallPolicy::max_attempts`, but the spawned body-stream loop terminates on transport failure or deadline without sharing that attempt budget.
- `src/main.rs::apply_stream_failure` leaves a replay boundary at the failed user message and displays the generic `Use /retry to branch. Stream failed: ...` copy.
- `src/main.rs::continue_tui_submission` and `apply_before_submit_hook_result` append a validated ordinary prompt without first resolving a dangling failed replay boundary. Inferred: the resulting consecutive user messages can be rejected by a provider and match the screenshot sequence, although the exact server-side HTTP 400 reason is unproven because CA intentionally discards provider response bodies.
- `src/main.rs::prepare_retry_branch` already establishes the required source-save, immutable-child, queue-isolation, and in-memory behavior for explicit retry.
- Existing loopback tests prove bounded `Retry-After` handling and no replay after partial text, but do not cover a body-stream transport failure before usable output.
- Official OpenAI API documentation classifies HTTP 400 as a bad request and documents that an oversized Responses input may return 400 when truncation is disabled. This plan keeps HTTP 400 non-retryable and does not introduce silent truncation.

## Scope

Allowed files:
- `src/agent/client.rs`
- `src/agent/client_tests.rs`
- `src/agent/stream.rs` when a shared usable-output classifier is required
- `src/main.rs`
- `src/tui/render_tests.rs` and only BUG-235 snapshot artifacts
- `docs/exec-plans/active/BUG-235-bounded-pre-output-recovery.md`, later moved to `docs/exec-plans/completed/`
- `CONTINUITY.md`, `NEXT_STEPS.md`, `ARCHITECTURE.md`, `PARITY.md`, `TESTING.md`, and `docs/.INDEX.md` for localized durable updates

Forbidden files/areas:
- `oracles/`, `reference/`, nested repositories, credentials, provider response bodies, runtime config/session state, and unrelated dirty-worktree files

Runtime state affected:
- Hermetic loopback servers and disposable temporary session homes only. No provider traffic, credential access, installation, or user session mutation.

## Design and Invariants

- One shared provider attempt budget owns response headers and body streaming. Ordinary routes allow at most two physical POSTs; one-attempt callers remain one-attempt.
- A request is frozen before the first POST and identical bytes, headers, route, and request identity are reused for the one allowed safe replay.
- Events that are not usable output remain attempt-local until the attempt commits. The first nonempty text, nonempty reasoning, tool-call start, or nonempty tool arguments commits the attempt and permanently suppresses replay. A clean terminal result also commits and releases buffered metadata.
- Only transport failures, 429, and 500/502/503/504 may consume the remaining safe replay before commit. `Retry-After` remains capped and waits/request/stream work observe cancellation.
- Request-store records, logical usage, transcript entries, and tool execution remain one per logical turn.
- Automatic replacement branching is eligible only when the last failed turn consists solely of its terminal user message and the retry boundary is that message. The source is saved, a child is branched immediately before that user message, and only the new validated prompt is appended to the child.
- Attachments and before-submit hooks complete before branching. Hook rejection changes neither source nor composer. Branch failure restores the composer and prevents provider I/O.
- Persistent sessions use the existing session-store branch path; bare and `--no-session` states use the existing in-memory branch path. Source queues stay on the source.
- Partial output, resolved-tool continuation, and explicit `/retry` retain existing behavior.
- Footer copy distinguishes exhausted pre-output recovery, HTTP 400 rejection, and failure after partial output; existing 401/403 and missing-credential guidance stays intact and secret-safe.

## Milestones

### M1 — Characterize provider replay boundaries

- Add hermetic tests for pre-output body transport replay, two-attempt exhaustion with identical requests, non-retryable HTTP 400, output/tool replay suppression, and cancellation.
- Expected observable result: new pre-output body recovery tests fail against the current implementation while existing header and partial-output tests remain green.
- Validation command: focused `cargo test --locked` filters for the added client tests.

### M2 — Unify the provider attempt budget

- Refactor the shared provider stream loop to cover headers and body failures, buffer pre-commit metadata, and observe cancellation.
- Expected observable result: exactly one safe replay before output and no replay afterward.
- Validation command: BUG-235 client tests plus existing retry, deadline, trace, and adapter tests.

### M3 — Clean failed-turn replacement branching

- Add state/session tests for persistent and in-memory branching, immutable source history, repeated failure cleanliness, branch failure, hooks, attachments, queues, paged history, partial output, and resolved-tool exclusions.
- Implement branching after validation and hooks but before appending the replacement prompt.
- Expected observable result: provider history never accumulates consecutive replacement user messages after a failed user-only turn.
- Validation command: focused BUG-235 state/session tests and existing retry-branch tests.

### M4 — Actionable failure surfaces

- Add distinct footer copy and wide/narrow deterministic render coverage.
- Expected observable result: the generic HTTP 400 footer is absent and recovery guidance matches whether output started.
- Validation command: focused render snapshot test and snapshot hygiene checks.

### M5 — Completion and durable handoff

- Run completion gates, record actual results, update harness documents, and move this plan to completed.
- Expected observable result: all available checks pass or any external blocker is recorded without guessing.

## Acceptance Criteria

- A body-stream transport failure before output performs one identical replay and can complete successfully.
- Two eligible failures produce exactly two identical physical requests and one terminal logical error.
- 429/5xx delay remains bounded; HTTP 400 produces exactly one request.
- Nonempty text, reasoning, tool-call start, or tool arguments suppress replay.
- Cancellation promptly stops retry delay, request, and stream work.
- A validated ordinary prompt after a dangling failed user turn creates a child whose replacement region contains only the new prompt; the source remains immutable with the failed request.
- Repeated failures do not accumulate consecutive replacement user messages.
- Branch failure preserves the composer and produces no provider request.
- Bare, `--no-session`, paged history, hook, attachment, queue, partial-output, and resolved-tool behavior stays consistent with its existing contract.
- Wide and narrow deterministic renders show one prompt with actionable recovery copy and never the generic HTTP 400 footer.
- No CLI/config/provider-wire/session-format change and no installation occur.

## Validation

Focused:
- BUG-235 client retry/cancellation tests
- BUG-235 state/session tests
- Existing retry-after, deadline, partial-output, provider-trace, adapter, manual retry, hooks, attachments, and queue tests
- BUG-235 wide/narrow render snapshot

Completion:
- `cargo test --locked`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`
- Snapshot hygiene and protected-tree verification from `TESTING.md`

Manual/visual:
- Inspect the wide/narrow snapshot artifact.
- Run a disposable-home loopback walkthrough if the existing harness can do so without credentials or user runtime mutation.

Blocked checks:
- Live provider reproduction is excluded because it requires credentials and would not reveal the intentionally discarded HTTP error body.
- The committed protected-tree receipt does not match the current ignored
  `oracles/` tree. Direct Git status/diff is empty, repeated preview scans are
  stable at `8dca8724…e258`, and `reference/` still matches. BUG-235 did not
  edit either protected tree or the receipt, so resolving the unrelated oracle
  baseline requires separate authority and work.

## Progress

- [x] Audited provider attempt, submission, retry-branch, session, and failure-render paths.
- [x] Recorded protocol/session evidence and the non-parity decision before source edits.
- [x] Added failing focused provider characterization tests; the pre-output body replay case failed with the current single-body-attempt `Transport(Request)` result.
- [x] Unified the provider attempt budget; all 58 shared client tests pass, including identical physical requests, one request-store record, metadata discard, mixed header/body exhaustion, HTTP 400/malformed suppression, output commit, and receiver cancellation.
- [x] Added and implemented clean failed-turn replacement branching. The new characterization first failed because the source ID did not change; ten BUG-235 state/session cases now pass, including no-hook provider-I/O suppression.
- [x] Added actionable failure copy and reviewed the two new wide/narrow snapshots. Both widths show exactly one prompt and the complete clean-branch action.
- [x] Passed tests, formatting, strict Clippy, release build, snapshot hygiene,
  and a disposable-home loopback walkthrough; recorded the unrelated
  protected-tree digest blocker and reconciled the harness.

## Decisions

- Decision: Cap ordinary route recovery at two total physical requests, independent from existing cross-provider fallback.
  Evidence: User-selected BUG-235 policy and existing `ProviderCallPolicy::max_attempts = 2`.
  Consequence: A body failure after a prior header/status retry cannot open a third POST.
- Decision: Treat only observable output as the commit boundary and buffer non-usable metadata until commit.
  Evidence: `provider_event_has_usable_output` already defines text/reasoning/tool boundaries used by fallback suppression.
  Consequence: Safely replayed attempts cannot leak duplicated metadata or tool effects.
- Decision: Branch only the exact dangling failed user-only shape.
  Evidence: Partial assistant and fully resolved tool histories already have distinct retry/continuation semantics.
  Consequence: Existing explicit recovery behavior remains unchanged outside the reported defect.
- Decision: Keep HTTP 400 non-retryable and do not inspect response bodies.
  Evidence: User plan, existing secret-safe provider boundary, and official OpenAI bad-request guidance.
  Consequence: The UI gives edit-and-resubmit guidance instead of blind replay.

## Surprises and Discoveries

- The default two-attempt policy currently applies only until successful response headers; the body task has no access to the remaining attempt budget.
- A text-format normalizer can temporarily retain a nonempty provider prefix. Such decoded text commits the physical attempt before normalization, even while no normalized text has reached the UI; it must never be replayed.
- Task-skill preparation runs before replacement branching, preserving its existing pre-request validation and frozen task snapshot without invoking it twice.
- The installed binary matched the existing release artifact during diagnosis, so the screenshot is not explained by a stale local install. Installation remains outside this plan.
- The disposable-home walkthrough observed six physical requests: the two
  replay pairs had byte-identical bodies, HTTP 400 used one request, and each
  replacement branch omitted the failed prompt while its source session kept
  that prompt.
- Protected-tree verification reports only an `oracles/` digest mismatch.
  Entry/file/directory/symlink/byte counts match the committed receipt, repeated
  previews are stable, `reference/` matches, and direct Git inspection is empty.
  The ignored evidence tree and receipt were not modified to clear the gate.

## Recovery and Rollback

- Revert only the localized BUG-235 hunks with `apply_patch`; do not reset or clean the dirty worktree.
- Keep source session data immutable. Tests use temporary session roots and loopback listeners.
- If the shared stream refactor cannot preserve exact request identity, trace accounting, or cancellation, stop before enabling replay and record the failed design here.

## Completion Record

Changed files: `src/agent/client.rs`, `src/agent/client_tests.rs`,
`src/agent/stream.rs`, `src/main.rs`, `src/tui/render_tests.rs`, two BUG-235
snapshot files, this plan, and localized `ARCHITECTURE.md`, `PARITY.md`,
`CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`, and `docs/.INDEX.md` entries.

Commands and results:
- Failure-first provider and session characterizations failed on the old body
  transport behavior and unchanged source session ID, then passed after the
  repair.
- `cargo test --locked -p ca agent::client::tests -- --test-threads=1`: PASS,
  58/58.
- `cargo test --locked -p ca replacement -- --test-threads=1`: PASS, 20/20
  matched tests, including all ten BUG-235 replacement cases.
- `cargo test --locked -p ca pre_output_recovery_snapshots -- --test-threads=1`:
  PASS, 1/1 with two reviewed combined wide/narrow artifacts.
- `cargo test --locked -- --test-threads=1`: PASS across the workspace.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo build --release`: PASS. The known non-fatal macOS oversized
  `__eh_frame` linker diagnostic remains.
- Snapshot hygiene and `git diff --check`: PASS.
- Disposable-home loopback TUI walkthrough: PASS; two identical requests per
  recoverable failure route, one request for HTTP 400, clean child histories,
  immutable failed source histories, actionable footers, and clean exit. Its
  exact temporary root was moved recoverably to
  `/Users/aji/.Trash/ca-bug235.hGzW2A`.
- `codex-team verify protected-trees --repo . --json`: BLOCKED by stable
  ignored `oracles/` digest drift (`bd0de373…189e` expected,
  `8dca8724…e258` observed); `reference/` and direct protected-tree Git checks
  pass.

Remaining risks: The exact provider-side cause of the screenshot HTTP 400
remains unproven by design. The unrelated protected-tree receipt mismatch needs
separate resolution before a repository-wide readiness claim can pass.

Harness updates: Architecture, CA-specific parity boundary, continuity,
testing evidence, task queue, and knowledge index now describe the shipped
behavior and the blocked protected-tree gate.
