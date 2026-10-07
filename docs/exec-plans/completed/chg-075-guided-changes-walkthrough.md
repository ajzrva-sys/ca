# CHG-075 — Guided Changes Walkthrough

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Add an idle-only `/walkthrough [working|base <branch>|commit <sha>]` flow that captures one complete bounded Git change snapshot, obtains a validated ordered explanation from the current provider without tools or conversation context, displays the explanation with its source hash, and visibly marks it stale if the source changes.

## Non-Goals

- Replacing the local `/diff` viewer or findings-oriented `/review`; suggesting or applying fixes; tool execution; canonical/session messages; automatic commits/staging; unbounded/binary/untracked-content upload; persistent walkthrough history; Cursor parity.
- Explaining a partial snapshot. Omitted tracked files, excess file count/bytes, or untracked files are labelled locally and block provider traffic. Users may stage the untracked files or narrow to an exact base/commit target before retrying.
- Provider-assisted file discovery or live reads. The provider receives exactly the captured diff snapshot and one reporting schema, never chat, checkpoint, skills, memory, MCP, hooks, project instructions, endpoint, credentials, headers, or workspace paths beyond repository-relative changed-file names.
- Live edit tracking outside an open completed walkthrough. Staleness monitoring is process-only and stops when the overlay closes.

## Evidence and Current Behavior

- `GitChangeService::review_input` already captures tracked working/base/commit diffs with argument-array Git commands, terminal sanitization, a 1-MiB aggregate cap, explicit `included_files`/`omitted_files`, and untracked names only (`src/changes.rs`). `/review` may proceed with partial input and read tools; CHG-075 must instead fail closed before traffic.
- `/diff`/`/changes` owns `ChangeReviewState`, loads status/diffs asynchronously, and must remain behaviorally unchanged (`src/{changes,main}.rs`, `src/tui/{commands,input,render}.rs`).
- Fresh review and goal/multi-run judges establish prepared-request, no-secret usage accounting, sole-report-schema validation, task generation, and stale-event patterns. `/btw` establishes an independent overlay/cancellation path that does not mutate canonical/session state.
- `RequestStore` can retain a safe semantic snapshot of this request, while resolved credentials remain inside `AgentClient` only.
- The approved oracle program names guided change walkthroughs as an oracle-informed product idea. The attached all-oracle review provides only a gap label, not a verified command or runtime contract, so the command/state design here is a `ca` extension.
- Baseline `cargo test changes --no-fail-fast` passed 10 matching unit/render tests, `cargo test review --no-fail-fast` passed 30 matching unit/render plus one CLI test, and `cargo test command_registry --no-fail-fast` passed before the first behavior edit.

## Scope

Allowed files: a dedicated `src/change_walkthrough.rs`; narrow additions to `src/{changes,app,main,request,usage}.rs`; `src/tui/{commands,input,render,render_tests}.rs`; focused snapshots; this plan and required harness documents.

Forbidden: oracle/reference changes or copied code; config/session schema changes; `/diff` or `/review` behavior changes; real user config/session/Keychain data; external providers; installed binary; provider calls for incomplete input; executable model tools.

Runtime state affected: one process-only overlay, one cancellable capture/provider task, one cancellable staleness monitor, one usage entry, and the existing process-only request inspector. No canonical message, session, work-plan, approval, model/default, config, Git, or filesystem mutation occurs.

## Design and Invariants

### Complete exact snapshot

- Reuse `ReviewTarget::{Working,Base,Commit}` parsing/validation and `GitChangeService::review_input`, but convert it immediately into a dedicated bounded `ChangeSnapshot`. Reject `Custom` targets.
- A provider-eligible snapshot has 1–128 tracked changed files, no `omitted_files`, no untracked names, a nonempty sanitized diff at or below the existing 1-MiB cap, and bounded repository-relative labels. Empty changes return a local no-changes state.
- The source hash is lowercase SHA-256 over an explicit version tag, target/scope, ordered included-file names, and exact diff bytes. Only the hash, safe target/scope, bounded file labels, result, status, and provider/model IDs enter `AppState`; the raw diff remains task-local and reaches only the prepared request.
- Partial input never starts `AgentClient`. The refusal screen distinguishes omitted tracked paths, untracked names-only, excess count, and empty/no-text changes. Lists and error text are independently bounded and terminal-sanitized.

### Isolated structured explanation

- Advertise exactly one non-executable `submit_change_walkthrough` function schema. The request has exactly two messages: a fixed system contract and a user message containing source hash, scope, ordered file list, and diff. It uses `RequestKind::ChangeWalkthrough`, `UsageKind::ChangeWalkthrough`, current provider/model options, and no tools other than the sole reporting schema.
- The sole final call contains a bounded summary, 1–32 ordered steps, step title/explanation, repository-relative file references drawn only from the captured included set, and up to 16 bounded notes. Reject text-only answers, multiple/other tool calls, malformed/unknown fields, skipped/duplicate order values, unknown files, empty content, terminal controls, or aggregate output over 64 KiB.
- Provider response text is discarded. The validated structured result is rendered locally in step order. Safe provider failures identify only the provider/status and preserve retry guidance; response bodies, endpoints, headers, or credentials never enter state/output.

### TUI lifecycle and staleness

- `/walkthrough` and alias `/explain-changes` are visible idle-only commands. Opening starts capture/generation immediately for the parsed target and records provider/model on the overlay.
- States are Capturing, Generating, Complete, Refused, Failed, and Stale. `Esc` cancels/invalidates in-flight work or closes a terminal state; `Shift+R` explicitly recaptures/regenerates; arrows/Page/Home/End scroll completed/refusal details. Additional keys are locked while busy.
- After a complete result, a separate low-frequency monitor recaptures the same bounded target and compares the exact source hash. A difference or transition to incomplete/no-changes marks the result stale and stops monitoring. Verification failures are shown without falsely declaring freshness. Closing/retrying increments the generation and aborts both tasks; stale generations cannot update reopened state.
- Complete rendering shows scope, short source hash, provider/model, included count, summary, ordered steps, notes, and `Fresh`. Stale rendering retains the explanation but prominently says the working source changed and requires `Shift+R`; it never silently refreshes or sends another provider request.
- Wide/narrow snapshots cover capturing, complete, stale, partial refusal, provider failure, and empty changes. `/diff` and `/review` snapshots/commands remain unchanged.

## Milestones

### M1 — Characterize and build the complete-snapshot boundary

- Record baseline focused change/review/command/render tests.
- Add failing temporary-Git tests for working/base/commit capture, stable/source-changing hashes, empty input, omitted byte/file limits, untracked refusal, file-count bounds, terminal/path sanitization, and zero-provider eligibility for incomplete input.
- Implement `ChangeSnapshot` and result/refusal types in the dedicated module using existing Git validation/capture.

### M2 — Add isolated structured generation

- Add fake/loopback backend tests proving exactly two messages, one sole schema, no canonical/project/skill/MCP/tool context, exact source hash/diff, current route/options, usage ownership, safe failures, and validation of every malformed/oversized/unknown-file report case.
- Implement the provider runner over `PreparedModelRequest` with no executable continuation.

### M3 — Integrate command, overlay, cancellation, and staleness

- Add command/palette/idle-only/target-parser tests; AppState generation/request flags; main-loop capture/provider and monitor tasks; stale result rejection; cancellation/retry; and no session/canonical/work/config mutation assertions.
- Add input/render tests and review wide/narrow snapshots for every visible state.

### M4 — Gates and released disposable walkthrough

- Run focused suites, `cargo test --no-fail-fast`, formatting, strict Clippy, release build, and help/version/profile smokes.
- In a disposable Git repository/home with a credentialless loopback provider, create a tracked change, run `/walkthrough`, prove one isolated sole-schema request and an ordered rendered result, mutate the source while the overlay remains open, prove `Stale`, refresh explicitly, and prove the new hash/request. Separately prove an untracked or over-cap input produces zero traffic.
- Stop owned fixtures and move the exact disposable root recoverably to Trash.

## Acceptance Criteria

- Every provider request is derived from one complete bounded hash-labelled Git snapshot. Any omitted/untracked/excess/empty input is visibly local-only and produces zero traffic.
- The provider sees no conversation, session, checkpoint, active skill body/catalog, memory, MCP, hooks, project instruction, executable tool, endpoint, header, credential, or unrelated path/content.
- A result exists only after one valid sole `submit_change_walkthrough` call with consecutive ordered steps and captured-file-only references; unsafe/malformed/oversized responses fail without partial display.
- The overlay is responsive and generation-safe; cancellation/close/retry discard late results. Source changes mark the retained result stale without automatic regeneration or canonical/session/config/Git mutation.
- `/diff`, `/changes`, `/review`, provider/default selection, approvals, and session behavior pass unchanged. Every new visible state has reviewed 100x30 and 60x24 snapshots and the released disposable walkthrough matches deterministic tests.

## Validation

Focused: baseline change/review/command suites PASS as recorded above. Current `cargo test walkthrough --no-fail-fast` PASS (16 matching unit/render/orchestration tests); final focused gate pending after review.

Completion: `cargo test --no-fail-fast` PASS (909 unit + 28 CLI); `cargo fmt --check` PASS; strict Clippy PASS; release build PASS; version/help/missing-profile/bare smokes PASS; zero pending snapshots.

Manual/visual: PASS. Fourteen reviewed 100x30/60x24 snapshots cover capturing, generating, complete, stale, empty, refused, and failed states. A released 100x30 disposable-home/Git walkthrough produced a fresh exact explanation, detected a source edit as stale, explicitly recaptured a different hash/report, and refused later untracked input without a third provider request.

Blocked checks: none currently.

## Progress

- [x] Existing Git-capture, `/diff`, `/review`, prepared-request, usage, task, and overlay seams characterized.
- [x] Failing complete-snapshot tests and pure boundary implemented.
- [x] Structured isolated runner implemented and tested.
- [x] Command/orchestration/input/render/staleness integrated with reviewed snapshots.
- [x] Repository-wide gates and released disposable walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: add `/walkthrough`; keep `/changes` as `/diff`.
  Evidence: `/changes` is an established alias for the non-model working-tree viewer, and changing it would silently break a fixed Ctrl+R workflow.
  Consequence: explanation is explicit/cost-bearing while local inspection stays unchanged; `/explain-changes` is a discoverable alias.
- Decision: refuse every partial input rather than letting the model fill gaps.
  Evidence: the approved roadmap explicitly requires complete-input refusal, while existing `/review` deliberately tolerates omissions and can use read tools.
  Consequence: untracked names and omitted tracked files are labelled locally; users narrow/stage/retry rather than receive an authoritative-sounding partial narrative.
- Decision: use one non-executable structured report schema, not free-form Markdown.
  Evidence: ordered output and file attribution need local validation, and existing fresh evaluators demonstrate sole-schema fail-closed handling.
  Consequence: provider prose outside the report is ignored, and malformed order/file references never become the displayed walkthrough.
- Decision: staleness compares a recaptured exact snapshot only while the result overlay is open.
  Evidence: `git status` alone cannot detect content changes that remain modified, while accepted snapshots are already bounded enough to recapture safely.
  Consequence: freshness is truthful without a persistent watcher or unbounded hashing lane.

## Surprises and Discoveries

- The existing review input is a strong safe capture seam but deliberately exposes partial-diff metadata and untracked names. CHG-075 must add a stricter eligibility conversion rather than weaken or overload review semantics.
- Exact freshness cannot be inferred from porcelain status because an already-modified file may change again without changing its status record. The completed overlay therefore recaptures the same bounded eligible snapshot and compares its SHA-256; incomplete/no-change transitions are stale.

## Recovery and Rollback

Remove the dedicated module, command/overlay/task branches, request/usage enum variants, and snapshots. No persistent format, config, Git state, or runtime file needs migration. `/diff` and `/review` remain independently recoverable and unchanged.

## Completion Record

Changed files: `src/change_walkthrough.rs`; narrow integrations in `src/{main,app,request,usage}.rs` and `src/tui/{commands,input,render,render_tests}.rs`; fourteen new walkthrough snapshots plus two updated wide usage snapshots; this plan and the required harness documents.

Commands and results: baseline change/review/command suites PASS; focused walkthrough suite PASS (16); full repository PASS (909 + 28); format, snapshot hygiene, strict Clippy, release, and release smokes PASS; isolated released-TUI walkthrough PASS. The sole lint finding was a test-only type-complexity warning, resolved with a local alias before the final clean Clippy run.

Remaining risks: provider-specific tokenization remains unavailable; large or untracked/partial change sets deliberately refuse rather than generate a partial explanation. The freshness monitor is intentionally process-only and active only while the completed overlay is open.

Harness updates: reconciled `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md`; next handoff is SES-076.
