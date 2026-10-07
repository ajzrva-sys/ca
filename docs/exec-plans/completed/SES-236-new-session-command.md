# SES-236 — Start a fresh session with /new

Status: COMPLETE (pre-existing workspace protected-tree gate remains blocked)
Owner/session: Codex, current implementation task
Last updated: 2026-09-03

## Goal

`/new` saves the previous conversation and starts an empty, independent CA
session with a new identity, keeping the selected model and workspace.

## Non-Goals

Do not change `/clear`, provider authentication, wire formats, or session formats.
Installation has not been requested. Testing uses disposable session stores.

## Evidence and Current Behavior

- `src/tui/commands.rs` registers `/clear` but no `/new`. Unknown typed text can
  leave the slash palette selecting an unrelated command, as in the screenshot.
- `execute_command_action` clears messages for `CommandId::Clear` without
  rotating the session identity or saving the old conversation first.
- `save_current_session` captures current metadata, messages, queues and work
  state. `apply_loaded_session` already resets most session-scoped state.
- Read-only Hermes evidence: `hermes_cli/commands.py` describes `/new` as fresh
  identity plus history; `cli.py::new_session` flushes the previous history before
  rotating and avoids accumulating empty sessions. No oracle source is copied.
- This implements the user's explicit fresh-session behavior, not full Hermes
  or Cursor parity. Existing dirty changes and protected-tree discrepancy remain.

## Scope

Implementation: `src/tui/commands.rs`, `src/main.rs`, `src/app.rs`, focused
input/render tests and their snapshots. Harness: this plan, NEXT_STEPS,
CONTINUITY, TESTING, ARCHITECTURE, PARITY, and docs index.
Oracle/reference trees and unrelated changes remain untouched. No real user
session, runtime configuration, credential, or installed binary is modified.

## Design and Invariants

- A distinct, idle-only `/new` command is discoverable and executes on Enter.
- Capture the old session without mutating it; perform disk work off the TUI
  event loop. Save failure keeps the old session and displays recovery guidance.
- Paged history must be reconstructed and validated before saving; never replace
  a full saved session with only its visible tail.
- Allocate a fresh root identity, reset conversation/checkpoint/retry/queue/work
  state and sitting metrics, keep process configuration/model/workspace intact.
- Keep queued drafts saved on the old session. Bare and `--no-session` perform
  the same in-memory reset without writing files. Empty sessions do not create
  saved-history clutter. No provider call is made by `/new`.
- Use generation-bound completion and prevent submission during the transition.

## Milestones

1. Characterize missing command and fresh-session identity/preservation.
2. Add command and safe transition using existing session infrastructure.
3. Cover preservation, failure, paged history, in-memory behavior, and rendering.
4. Run focused and completion checks and reconcile the harness.

## Acceptance Criteria

`/new` appears in search and does not dispatch `/clear`; old saved messages and
queues survive; new identity has no parent/messages/checkpoint/retry/queue/goal;
model and workspace remain; failed saves do not switch; bare/no-session do not
write; wide/narrow rendering shows a fresh prompt and clear confirmation.

## Validation

Focused tests first, then `cargo test --locked -- --test-threads=1`,
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo build --release`, snapshot hygiene, `git diff --check`, protected-tree
verification, and a disposable-home TUI walkthrough if available.

## Progress

- [x] Inspect registry, session saving/restoration, and Hermes behavioral evidence.
- [x] Failure-first characterization: `/new` registry test failed with the missing-command assertion.
- [x] Implementation and focused verification: ten tests pass, including wide/narrow snapshots.
- [x] Completion checks and harness reconciliation; unrelated protected-tree mismatch recorded.

## Decisions

Preserve `/clear` as-is; the user explicitly requested `/new`, not a change to
both commands. Preserve the selected workspace and model as process context.

## Surprises and Discoveries

`/new` is absent, rather than being an alias for `/clear`.

The common command menu needs two baseline updates: the global command count
increases from 105 to 106 and the Session category from 15 to 16. The first two
full runs exposed those intentional changes in sequence; the complete palette
test now passes after reviewing both snapshots. Saving an unchanged session
does not change its storage generation, so the stale-generation fixture must
actually alter a message before resaving.

## Recovery and Rollback

Remove only this task's patches to recover; do not reset the dirty worktree.
Disk save failure must leave the active source intact. Tests use temporary files.

## Completion Record

Implementation lives in `src/main.rs`, `src/app.rs`, and
`src/tui/commands.rs`. Focused tests are in `src/main.rs`, the command registry,
`src/tui/input_tests.rs`, and `src/tui/render_tests.rs`. Two new combined
snapshots (`new_session_command` and `new_session_empty`) and two reviewed
command-menu snapshots cover discovery and the clean prompt.

- Ten focused new-session tests pass, including the failure-first registry
  case, save-before-switch worker, immutable source/queue preservation, new
  root identity, first-prompt title, paged prefixes, failed save, stale result,
  active work, repeated empty starts, and memory-only profiles.
- All 218 rendering tests pass. New wide/narrow frames were reviewed at
  100x30 and 60x24. Interface-writing guidance kept confirmation concise and
  made the location of the previous conversation explicit through `/sessions`.
- Formatting, strict all-target Clippy, release build, `--version`/`--help`,
  `git diff --check`, and pending-snapshot hygiene pass. The known non-fatal
  oversized macOS `__eh_frame` linker warning remains.
- Manual Codex PTY walkthrough (`TERM=xterm-256color`) used only a disposable
  home and deliberately missing fixture credential. Exact input sequence:
  `old session prompt`, `/new`, `new session prompt`, `/save`, `/sessions`,
  Enter to reopen the old session, `/quit`. The observed new-session
  confirmation replaced the old transcript, both saved roots contained only
  their own prompt and no parent, the old prompt reopened, and exit restored
  the terminal. No provider HTTP request was made.
- Protected-tree verification remains blocked by the pre-existing ignored
  `oracles/` baseline mismatch. Direct protected-tree Git status is empty;
  neither protected files nor baseline receipts were changed.
- Release SHA-256 is
  `1e91f11413c0d48cdfe6bbc9af2e1f9bee7c98e4d739e87d4865ea2fb2418f72`.
  Installed binary SHA-256 remains
  `682c45492e8e7060148f7d853cc783d394af1a2f8768195871042cd19aa1df12`.
  No installation or real runtime/configuration/credential mutation occurred.

`cargo test --locked -- --test-threads=1` passes: CA 2,431 passed/4 ignored,
CLI 38/38, role-run 17/17, ca-watch 18/18, codex-team 652 passed/1 ignored,
MCP stdio 8/8, and every remaining integration/protocol/doc-test target.
Logs and the exact disposable walkthrough home were moved recoverably to
`/Users/aji/.Trash/ca-ses236.bwusk8`. Harness records and index links are updated.
The requested `/new` behavior is complete; the unrelated protected baseline
discrepancy is the only blocked validation check.
