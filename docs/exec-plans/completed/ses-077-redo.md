# SES-077 — Redo

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Make `/undo` functional for successful built-in file mutations and add `/redo`, using a bounded process-only history whose filesystem work runs off the TUI thread, refuses externally drifted targets, clears redo after a new mutation, and never changes conversation branches or saved-session history.

## Non-Goals

- Undoing arbitrary shell commands, MCP tools, hooks, LSP operations, subagent/worktree/config/session mutations, Git commits, provider output, or edits made outside `ca`.
- Persisting undo/redo across process restarts or into Session V4; changing `/rewind`, `/retry`, `/fork`, timeline, queue, or lazy-history semantics; multi-file transactions; binary-file editing; Cursor parity.
- Weakening workspace preparation, approval, sandbox, tool-selection, symlink, or path-authority checks. Undo/redo restores only exact already-prepared files recorded by successful main-session built-in tools.

## Evidence and Current Behavior

- `CommandId::Undo` and visible `/undo` already exist, but `src/undo.rs::UndoStack::snapshot` has no caller anywhere in `src/`; therefore every ordinary `/undo` reports `Nothing to undo`.
- `UndoStack::undo` performs `std::fs::write`/`remove_file` synchronously from `execute_command` in the TUI loop, does not check whether the target changed after the recorded mutation, drops an entry even when restoration fails, stores no redo state, and has no bounded-content validation.
- The four bounded UTF-8 built-in file mutations are `write_file`, `edit_file`, `apply_patch`, and `delete_file` in `src/agent/tools.rs`. They already execute after central workspace preparation/approval and know both before/after content at their exact successful write boundary.
- Interactive tool results converge through typed `ToolExecutionResult`, then either a direct `ResolvedToolResult` batch or ordered manual execution before `apply_tool_result` in `src/main.rs`. Headless, review, checkpoint, subagent, and MCP paths must retain their current result behavior and must not gain a TUI history.
- `/rewind` and `/retry` are immutable conversation/session branch operations and are unrelated to file mutation reversal. The approved program explicitly requires current branch/session semantics to remain intact.
- Baseline characterization confirmed the inert stack before edits. The completed focused suite covers the core, tool receipt, command, and orchestration boundaries. This remains an oracle-informed `ca` extension; no clean Cursor or oracle redo fixture is claimed.

## Scope

Allowed files: `src/undo.rs`; narrow mutation-receipt additions in `src/agent/{registry,tools}.rs`; interactive orchestration/state in `src/{app,main}.rs`; `/redo` registry/input/render coverage in `src/tui/{commands,input,render,render_tests}.rs`; focused snapshots if visible state warrants them; this plan and required harness documents.

Forbidden: oracle/reference changes or copied code; Session V4/config/provider/protocol/approval changes; real user files during tests/walkthrough; installed binary; broad Git cleanup; undo support for shell/MCP/subagent/config/session operations.

Runtime state affected: up to 20 process-only UTF-8 mutation records containing exact canonical paths and bounded before/after content. Explicit undo/redo changes exactly one recorded local file after drift validation. Tests and walkthrough use a disposable workspace/home only.

## Design and Invariants

### Mutation receipt boundary

- Add a secret-free `FileMutationReceipt` to successful typed tool execution only for `write_file`, `edit_file`, `apply_patch`, and `delete_file`. It carries a bounded action label, the exact prepared/canonical target path, and before/after `FileState::{Missing,Utf8(String)}`.
- Capture the before state immediately before the tool write/delete and create the receipt only after success. The after state is the exact bytes written or Missing after deletion. Failed/denied/cancelled tools never create an entry.
- Receipt content is process-only, excluded from `Debug`, model text, hooks, request/session state, snapshots, usage, and headless/scripted output. Other runners may transport and discard the receipt but never retain it.
- Enforce the same bounded textual-file size supported by the mutating tool schemas. An unexpectedly over-bound before image fails the mutation before changing the file rather than creating an unsafe/unredoable history entry.

### Process-safe history

- Replace the inert one-stack API with bounded undo and redo vectors. Recording a successful new mutation pushes one undo entry and clears redo. At most 20 entries are retained per direction; evicted content is dropped process-locally.
- An undo operation peeks rather than pops, reads the exact target on a blocking worker, and requires it to equal the recorded after state. If equal, it restores before; only then does the entry move undo→redo. A redo symmetrically requires current state equal before, restores after, and moves redo→undo.
- Drift, missing-parent, non-regular/symlink target, permission, read, write, or delete failure leaves both stacks unchanged and returns a safe inline error naming only the workspace-relative or bounded target label. Atomic same-directory replacement is used for content restoration; Missing restoration deletes only the exact validated regular file. Creation refuses an unexpected existing target.
- Undo/redo tasks have a monotonically increasing generation and one busy lock. Esc cannot cancel a write already executing, but stale results cannot mutate a replaced `AppState`; ordinary composer/modal actions remain locked only for the short blocking operation.

### Interactive behavior and compatibility

- Add visible idle-only `/redo` beside `/undo`. `/undo` and `/redo` queue typed blocking requests; completion shows `Undid <action>: <path>` or `Redid <action>: <path>`. Empty stacks report `Nothing to undo`/`Nothing to redo` without spawning work.
- Direct and approval-ordered successful main-session file results record exactly once. A new tracked mutation after one or more undos clears redo. Successful non-file mutating built-ins conservatively clear redo because their side effects may invalidate a prior forward history, but do not create undo entries.
- Undo/redo never append canonical messages, save sessions, change `current_session`, create branches, alter checkpoints, invoke hooks/tools/provider traffic, or modify tool-call outputs. Existing `/rewind`, `/retry`, `/fork`, and lazy hydration paths remain byte/behavior compatible.
- Tests cover command discovery/idle-only behavior, exact create/modify/delete/patch round trips, multiple-step ordering, redo clearing, capacity, drift/failure retention, symlink/non-regular refusal, one-busy lock, stale generations, direct/approval result single-recording, no receipt leakage, and branch/session immutability. Add wide/narrow status coverage only if a durable visible overlay is introduced; concise status messages use existing render coverage otherwise.

## Milestones

### M1 — Characterize and replace the inert core

- Record baseline undo/command/tool tests and add failing temporary-root tests proving the current empty-history defect, reversible create/modify/delete transitions, drift refusal, failure retention, redo ordering/clearing, capacity, and safe Debug behavior.
- Implement bounded mutation states/receipts and a pure history transition API whose I/O request/result can run on a blocking worker.

### M2 — Emit exact receipts from built-in file tools

- Add focused tool tests for all four successful mutation handlers, failed writes/edits/deletes, bound refusal, and exact before/after states.
- Thread receipts through typed execution without changing model-visible text or other runner behavior.

### M3 — Integrate `/undo` and `/redo`

- Add `/redo` command/palette tests, AppState busy/generation/request state, blocking-worker orchestration, direct/ordered mutation recording, conservative redo invalidation, stale result rejection, and branch/session/canonical invariants.
- Add deterministic input/render coverage and review snapshots only for any new persistent UI state.

### M4 — Gates and released disposable walkthrough

- Run focused suites, `cargo test --no-fail-fast`, formatting, strict Clippy, release build, and help/version/profile smokes.
- With a disposable home/workspace and credentialless loopback provider, perform model-driven create then edit, run `/undo` twice and `/redo` twice, prove exact bytes at each step, undo once then make a new edit and prove `Nothing to redo`, and separately prove external drift blocks undo without changing the file/history. Stop fixtures and move the exact root recoverably to Trash.

## Acceptance Criteria

- Every successful main-session `write_file`, `edit_file`, `apply_patch`, or `delete_file` produces exactly one bounded process-only undo entry; no failed/denied/cancelled or non-file result does.
- `/undo` and `/redo` perform no TUI-thread filesystem I/O, restore only an exact expected current state, retain entries on failure, and move entries between bounded stacks only after successful mutation.
- A successful new mutation after undo clears redo. Multiple undo/redo operations preserve order and exact UTF-8 content across create, replace, patch, and delete cases.
- Receipts/content never enter model-visible tool text, hooks, Debug, snapshots, provider requests, canonical messages, Session V4, usage, or headless/scripted output.
- Existing tool approval/execution outcomes, `/rewind`, `/retry`, `/fork`, lazy session hydration, and full repository gates pass unchanged; the released disposable walkthrough proves real TUI behavior.

## Validation

Focused: PASS. `cargo test undo --no-fail-fast`, `cargo test redo --no-fail-fast`, and `cargo test file_mutation --no-fail-fast` cover seven undo-filtered, four redo-filtered, and two file-mutation receipt tests.

Completion: PASS. `cargo test --no-fail-fast` passed 933 unit plus 28 CLI integration tests; formatting, strict Clippy, release build, and help/version/profile smokes passed.

Manual/visual: PASS. The existing command palette visibly exposes both commands. A released disposable TUI walkthrough proved approval-ordered create, direct edit, exact two-step undo/redo, redo invalidation after a new edit, and external-drift refusal without byte/history loss.

Blocked checks: none currently.

## Progress

- [x] Existing undo command, inert stack, mutation handlers, and interactive result convergence traced.
- [x] Characterization and failing core tests recorded.
- [x] Mutation receipts and bounded drift-safe history implemented.
- [x] Interactive direct/ordered execution plus `/redo` integrated.
- [x] Focused and repository-wide gates pass.
- [x] Released disposable walkthrough passes.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: repair `/undo` as part of adding redo instead of layering redo on its current inert API.
  Evidence: `UndoStack::snapshot` has zero callers, so there is no forward state from which redo could be correct.
  Consequence: SES-077 changes the internal mutation-result boundary but keeps the existing user-facing `/undo` intent.
- Decision: record only exact built-in UTF-8 file mutations and conservatively invalidate redo for other successful mutating built-ins.
  Evidence: these four handlers know exact before/after file states; arbitrary shell, MCP, hook, and subagent side effects are not enumerable or safely reversible.
  Consequence: the feature is truthful about scope and never pretends a partial filesystem snapshot can reverse arbitrary work.
- Decision: compare-and-restore on a blocking worker.
  Evidence: external edits can occur after a tool completes, and current synchronous I/O violates the TUI responsiveness guardrail.
  Consequence: drift fails closed without consuming history, while the event loop remains responsive.
- Decision: keep mutation history process-only and independent from conversation undo.
  Evidence: `/rewind`, `/retry`, and branches already own immutable Session V4 semantics; the approved roadmap requires those semantics preserved.
  Consequence: resuming a saved session starts with empty file undo/redo history.

## Surprises and Discoveries

- The existing `/undo` surface is user-visible but cannot succeed in ordinary operation because no production path calls `snapshot`.
- Existing restoration uses direct synchronous writes/removal and consumes entries before knowing whether restoration succeeded; both must change for safe redo.

## Recovery and Rollback

Remove `/redo`, receipt retention, and async history requests while retaining the typed tool-result compatibility. Do not delete or broadly rewrite workspace files. Tests use exact temporary roots; walkthrough artifacts move recoverably to Trash. If a blocking restoration fails, leave the target and both stacks unchanged and report the safe error.

## Completion Record

Changed files: `src/undo.rs`, `src/agent/registry.rs`, `src/agent/tools.rs`, `src/app.rs`, `src/main.rs`, `src/headless.rs`, `src/tui/commands.rs`, this plan, and the five required harness documents.

Commands and results: focused undo/redo/mutation suites PASS; full tests PASS (933 unit + 28 CLI); format PASS; strict Clippy PASS; release build PASS; help/version/profile smokes PASS.

Remaining risks: arbitrary shell/MCP/subagent/config/session mutations are intentionally not reversible. Exact UTF-8 file history is capped at 20 entries and 1 MiB per state; larger/binary targets fail before the built-in mutation rather than producing an unsafe partial history. OS-level races after live-state comparison remain bounded to the exact target and publish through regular-file checks plus durable replacement.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` record the process-only boundary and evidence. The walkthrough root is recoverable at `/Users/aji/.Trash/ca-ses077-walk.Ro7XcE`.
