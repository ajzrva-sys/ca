# TRM-079 — Durable Terminals

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Add an idle-only `/term [filter]` manager around an existing user-installed `tmux`, supporting bounded list, create, attach, detach, and Cancel-first confirmed kill while treating every tmux session as an external user-owned resource that survives normal `ca` shutdown.

## Non-Goals

- Installing, updating, configuring, or embedding tmux; persisting terminal metadata in config or Session V4; starting terminals automatically; killing tmux servers at `ca` shutdown; remote tmux, terminal multiplexing inside ratatui, GUI tabs, or model-visible terminal tools.
- Replacing process-owned shell jobs or changing their shutdown guarantees.
- Cursor parity. This is an oracle-informed `ca` extension without a clean Cursor fixture.

## Evidence and Current Behavior

- `src/jobs.rs::JobManager` owns `ca` child jobs and deliberately cancels them at shutdown. Durable terminals require a separate boundary because tmux sessions are user-owned and process-external.
- `src/tui/commands.rs` has no `CommandId::Term`; `AppState` has only process-owned job surfaces; no tmux source module exists.
- `src/tui/event.rs` owns raw/alternate-screen lifecycle and a blocking crossterm input pump. Attaching safely requires pausing that pump, temporarily restoring the normal terminal, awaiting tmux off the async runtime, and restoring ratatui afterward.
- `tmux` is not installed in the current validation environment. Product behavior must therefore degrade visibly, while protocol tests and walkthroughs use a fake executable in disposable roots.
- Baseline exact-string/source scan found no existing `/term` implementation.

## Scope

Allowed files: a new `src/terminal_sessions.rs`; narrow additions in `src/{main,app}.rs`; `src/tui/{commands,event,input,render,render_tests}.rs`; focused snapshots; this plan and required harness documents.

Forbidden files/areas: oracle/reference trees; `JobManager` ownership/shutdown semantics; provider/tool/session/config protocols; real tmux servers; real user config/sessions/Keychain; installed binary.

Runtime state affected: process-only manager rows/notices/errors and explicitly requested external tmux operations. Tests use only fake executables and temporary roots. Normal shutdown performs no tmux command.

## Design and Invariants

### External tmux boundary

- `TmuxManager` owns only an executable path and bounded command methods. Session names are validated before spawn; output is UTF-8-safe, size-bounded, and parsed from one explicit tab-separated format.
- List/version/create/detach/kill use argument vectors, no shell, no inherited stdin, a short timeout, and safe body-free errors. Create uses the current workspace directory and never attaches automatically.
- `ca` never owns a tmux server or session. There is no destructor, global registry, shutdown hook, cleanup sweep, or config/session persistence for this manager.

### TUI manager

- `/term [filter]` opens a searchable list. Rows show safe name, windows, and attached-client count; an unavailable installation renders concrete install guidance without failing startup.
- Typing filters; arrows/Home/End/Page navigate. Enter attaches. `Shift+A` opens validated name entry and creates detached. `Shift+D` detaches clients. `Shift+K` opens a kill confirmation with Cancel selected. Esc returns/closes; busy operations lock mutation controls.
- Refresh after each mutation preserves filter and selects the affected row when present. Failures remain inline and do not close the manager.

### Attach lifecycle

- Attach pauses the crossterm event pump before leaving the alternate screen/disabling raw mode. The blocking tmux process runs on a blocking worker. Ratatui raw/alternate state is restored before event intake resumes, including spawn/nonzero failures.
- Attaching does not mark the terminal as a `ca` job and does not affect conversation, queue, goal/workflow, usage, sessions, approvals, or shutdown.

## Milestones

### M1 — Characterization and tmux adapter

- Add failing command/adapter tests for absence, malformed output, bounds, name validation, exact argv, timeout/nonzero/unavailable behavior, and no shutdown cleanup.
- Implement typed safe summaries and bounded manager operations against a fake executable.

### M2 — Manager and terminal suspension

- Add process-only overlay/input/actions/rendering plus wide/narrow snapshots for unavailable, empty, populated, creating, busy, confirmation, and failure states.
- Add tested lifecycle suspend/resume and attach orchestration with exact restore-on-error behavior.

### M3 — Gates and disposable walkthrough

- Run focused and full gates, release/profile/help/version smokes, then exercise the released TUI with a fake tmux executable and disposable log proving create/list/attach/detach/confirmed kill argv and zero implicit shutdown command.

## Acceptance Criteria

- Missing tmux leaves `ca` usable and `/term` explains how to install it. Valid fake/real tmux rows are searchable and bounded.
- Create, detach, and kill target exactly one validated session. Kill is impossible without explicit confirmation and defaults to Cancel.
- Attach restores the terminal on success, nonzero exit, and spawn failure; the event pump cannot consume attached-session input.
- Normal `ca` shutdown invokes no tmux command and leaves all external sessions untouched.
- JobManager behavior, canonical/session/config bytes, and repository-wide gates remain unchanged.

## Validation

Focused: PASS: 30 terminal-filtered tests across adapter, command, state, input, lifecycle, and render boundaries; 14 reviewed wide/narrow snapshots.

Completion: PASS: `cargo test --no-fail-fast` (957 unit plus 28 CLI integration tests), formatting, strict Clippy, release build, and help/version smokes.

Manual/visual: PASS. A released disposable TUI walkthrough used a fake tmux executable to prove list/create/attach/detach/default-Cancel/confirmed-kill and exact terminal restoration. It left a `survivor` session in fake state, then exited `ca`; the argv log ended after list refresh and contained no shutdown/kill command for `survivor`. The inspected fixture moved recoverably to `/Users/aji/.Trash/ca-trm079-walk.eFYtFa`.

Blocked checks: real tmux walkthrough unavailable because tmux is not installed; the hermetic fake-executable path is authoritative for this plan.

## Progress

- [x] Existing command, overlay, job-ownership, and terminal-lifecycle seams traced.
- [x] Missing local tmux recorded as an expected unavailable environment.
- [x] Adapter characterization and implementation complete.
- [x] TUI manager and safe attach lifecycle complete.
- [x] Focused/full gates and released disposable walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: keep tmux completely separate from `JobManager`.
  Evidence: process jobs are intentionally cancelled on shutdown, directly conflicting with durable user ownership.
  Consequence: no shared cleanup or ID namespace can accidentally destroy tmux sessions.
- Decision: validate conservative tmux names rather than accept arbitrary targets.
  Evidence: target names beginning with options or containing control/separator bytes make safe argv/error/UI handling ambiguous.
  Consequence: names use bounded ASCII letters, digits, dot, underscore, and hyphen with an alphanumeric first byte.
- Decision: use an explicit TUI suspend/resume API for attach.
  Evidence: disabling raw mode alone leaves the alternate screen and crossterm reader active.
  Consequence: attached tmux receives the terminal exclusively and every exit path restores `ca`.

## Surprises and Discoveries

- The existing external-editor path only toggles raw mode and the event-polling task keeps running; it is insufficient for a long-lived full-screen attach and is not reused as-is.
- Existing tmux session names are broader than creation names; list/attach/detach/kill therefore accept bounded control-free names and use tmux's exact `=name` target syntax, while creation keeps the conservative portable grammar.

## Recovery and Rollback

Remove the command/state/adapter/lifecycle additions and focused tests with local patches. No schema/config migration or owned external resource exists, so rollback requires no runtime repair.

## Completion Record

Changed files: `src/terminal_sessions.rs`, `src/main.rs`, `src/app.rs`, `src/tui/commands.rs`, `src/tui/event.rs`, `src/tui/input.rs`, `src/tui/render.rs`, `src/tui/render_tests.rs`, 14 reviewed snapshots, this plan, and required harness documents.

Commands and results: focused tests, 957-unit plus 28-CLI full suite, formatting, strict Clippy, release build, help/version smokes, snapshots, and released fake-tmux walkthrough passed.

Remaining risks: a real tmux binary was unavailable locally, so real attach interoperability remains environment-dependent; hermetic exact-argv and lifecycle coverage passed.

Harness updates: architecture, continuity, testing, next steps, and parity reconciled; plan archived.
