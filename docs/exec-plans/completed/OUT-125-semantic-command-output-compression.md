# OUT-125 — Semantic Command-Output Compression

Status: COMPLETE
Owner/session: Codex / current reliability continuation
Last updated: 2026-08-16

## Goal

Add an opt-in, conservative human-only semantic presentation for proven
single Git status/diff and Cargo test/Clippy commands, retain the unchanged
bounded model result for expansion, and provide an exact confirmed native
full-output rerun without changing provider/session/export history.

## Non-Goals

- No `rtk`, shell parser dependency, arbitrary command summarizer, model-written
  transform, automatic rerun, provider-result rewrite, or command authority
  widening.
- No semantic handling for wrappers, pipelines, substitutions, redirects,
  compound shell input, background jobs, or unsupported Git/Cargo subcommands.
- No session, provider, tool-schema, wire-format, branch/worktree, installed
  binary, oracle/reference, or intermediate-commit change.

## Evidence and Current Behavior

- `src/jobs.rs::JobInfo::formatted_output` creates the existing 4-KiB
  model-facing bounded stdout/stderr result from independent 64-KiB captures.
- `src/agent/tools.rs::classify_job_result` sends that same bounded result
  through `ToolExecutionResult`; `src/main.rs` later copies it into canonical
  `ToolCall.output`, which is rendered, continued to the provider, and saved.
- `src/tui/render.rs::render_tool_call` shows at most 20 result lines until the
  stable tool-call block key is expanded. There is no separate process-only
  command-presentation record.
- Direct `!` commands use the same `JobManager` and synthesize a visible
  `run_command` call, but no current command path can stream a fresh full
  capture into native scrollback.
- `src/tui/event.rs::Tui::suspend_for` and the `/raw` presenter establish the
  terminal restoration and Enter-acknowledgement conventions.
- Inferred: semantic output must be a process-only map keyed by tool-call ID;
  mutating `ToolCall.output` would violate the explicit history boundary.

## Scope

Allowed files: a focused command-output module; app/config/job/tool and TUI
orchestration/render/input source; focused tests/snapshots; this plan; and
reconciled harness documents.

Forbidden files/areas: `oracles/`, `reference/`, provider/session/export
schemas, model tool schemas, credentials, branches, worktrees, stashes,
preservation directories, intermediate commits, and installed binaries.

Runtime state affected: one backward-compatible agent config enum and
process-only semantic-presentation/latest-rerun state. A confirmed rerun may
execute only the exact previously eligible command under current sandbox,
approval, network, workspace, and timeout policy.

## Design and Invariants

- `CommandOutputPolicy::{Bounded, Semantic}` defaults to `Bounded` and saves
  through the comment-preserving config store. Failed or bare saves leave the
  active setting unchanged.
- A small tokenizer recognizes only a single direct `git` or `cargo`
  invocation. Any shell control operator, redirect, substitution, newline,
  wrapper, environment assignment, unsupported flag placement, or ambiguous
  grammar returns ineligible and preserves current rendering byte-for-byte.
- Eligible terminal results are parsed from the existing bounded capture.
  The presentation truthfully labels source-capture omissions, parser
  uncertainty, transformations, and omitted patch/diagnostic/failure content.
  Parser doubt falls back to the current bounded output.
- The canonical `ToolCall.output` remains the existing bounded raw text.
  Process-only metadata keyed by call ID supplies collapsed semantic text;
  expansion renders the original result and does not serialize.
- The latest successful eligible semantic record stores exact command, cwd,
  network request, and call identity. `/output full` is human-only and opens a
  Cancel-first exact command/cwd confirmation. Current persistent Deny and
  distinct network authority remain stronger than confirmation.
- A confirmed rerun uses the ordinary command timeout, current workspace and
  sandbox, no model/tool continuation, and no prior-result mutation. It
  suspends the alternate screen, incrementally sanitizes stdout/stderr into
  native scrollback, prints exact terminal status/omission markers, waits for
  Enter, and restores the TUI on every return path.

## Milestones

### M1 — Characterize configuration and conservative eligibility

- Add failing tests for serde/default/save behavior, config-picker semantics,
  safe argv tokenization, supported forms, and every shell/ambiguity fallback.
- Add parser fixtures for Git status/diff and Cargo test/Clippy success,
  failure, binary, diagnostics, truncation, malformed, and unsupported output.
- Validation: `cargo test command_output --no-fail-fast` and `cargo test
  output_policy --no-fail-fast`.

### M2 — Separate semantic presentation from canonical output

- Add typed process-only presentation/rerun records, populate them for model
  and direct foreground `run_command` results, and render semantic collapsed
  text while preserving raw expansion.
- Prove messages, provider continuation input, sessions, exports, background
  jobs, and unsupported commands remain byte-identical.

### M3 — Implement `/config` and `/output full`

- Add the policy picker and Cancel-first rerun confirmation with wide/narrow,
  empty, error, busy, and fallback snapshots.
- Apply current approval/sandbox/network/timeout authority, stream sanitized
  native output, wait for Enter, and restore terminal state without updating
  prior tool results.

### M4 — Validate and reconcile

- Run focused and complete local gates, disposable native walkthrough, and
  exact-source physical-FreeBSD gates.
- Reconcile harness documents, archive this plan, and activate PRJ-126.

## Acceptance Criteria

- Only proven supported single commands gain semantic collapsed presentation;
  unsupported or ambiguous commands render exactly as before.
- Git status/diff and Cargo test/Clippy summaries carry required groups,
  totals, diagnostics/failures, status, and truthful omission/truncation
  labels without silently dropping captured information.
- Ctrl+O expansion always exposes the unchanged bounded raw tool result.
- `/output full` is unavailable without an eligible semantic record,
  confirmation defaults to Cancel, policy remains authoritative, and a
  confirmed rerun cannot alter model history or the earlier result.
- No semantic metadata reaches provider requests, sessions, exports, or
  canonical messages.

## Validation

Focused: command-output/parser/config/picker/rerun tests passed locally and on physical FreeBSD.

Completion: local default (1444 passed, 1 ignored), all-feature main (1473 passed, 1 ignored), experiment (118 passed, 1 ignored), CLI (37 passed), formatting, strict Clippy, release build, help/version, diff and snapshot hygiene passed.

Manual/visual: deterministic wide/narrow picker, confirmation, collapsed semantic, and raw expansion snapshots reviewed. Native streaming uses the established suspend/restore boundary.

Physical FreeBSD: focused, default (1431 passed, 1 ignored), all-feature main (1460 passed, 1 ignored), experiment (116 passed, 1 ignored), and CLI (37 passed) passed. The user requested a pause during the subsequent native Clippy/release continuation, so that continuation was stopped and is not claimed.

## Progress

- [x] Plan activated after CFG-124 completion.
- [x] Failing characterization tests added.
- [x] Conservative parser and process-only presentation implemented.
- [x] Config picker and native full-rerun workflow implemented.
- [x] Focused/local gates and physical-FreeBSD test suites complete.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: semantic summaries are TUI presentation metadata, never a
  replacement for the model-facing command result.
  Evidence: the same `ToolCall.output` currently feeds rendering,
  continuation, and session persistence.
  Consequence: the implementation must keep a separate non-serialized map.
- Decision: eligibility is a closed parser over direct invocations, not a
  best-effort shell heuristic.
  Evidence: the requested policy explicitly rejects composition, redirection,
  substitution, ambiguous wrappers, and unsupported commands.
  Consequence: false negatives safely retain bounded behavior; false positives
  are treated as test failures.

## Surprises and Discoveries

- The command result had to gain a process-only observation variant so the semantic parser could see the existing 64-KiB captures without changing the 4-KiB model/session result.
- A prior foreground-job test intentionally matched the old result variant; it was updated to assert the new typed command outcome while preserving its model text and success semantics.

## Recovery and Rollback

OUT-125 adds one defaulted config field and process-only state. Source rollback
removes the module, enum/store update, picker, rerun workflow, render metadata,
tests, snapshots, and docs. It does not require rewriting existing sessions,
tool results, provider history, or user commands.

## Completion Record

Implemented the conservative Git/Cargo semantic policy, comment-preserving picker, process-only render metadata, raw Ctrl+O expansion, and Cancel-first native full-output rerun. No oracle/reference, provider/session/export schema, installed binary, branch/worktree/stash, or intermediate commit was introduced.
