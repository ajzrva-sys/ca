# SES-142 — Exit Session Handoff

Status: COMPLETE
Owner/session: Codex / current user-requested session UX repair
Last updated: 2026-08-18

## Goal

After a normal interactive session exit, restore the terminal and print the
session name plus a copyable `ca -s <ses-id>` continuation command.

## Non-Goals

- Do not print unusable resume hints for empty or persistence-disabled runs.
- Do not change session identity, title generation, serialization, storage,
  resume lookup, headless output, service protocols, or exit codes.
- Do not add provider traffic, mutate runtime configuration, install a binary,
  modify oracle/reference evidence, or claim Cursor parity.

## Evidence and Current Behavior

- `src/main.rs::session_report` already presents the session name, public
  `ses_…` identifier, and `ca -s …` command inside the TUI.
- Before this change, `run_tui` returned `Result<()>`; the terminal lifecycle
  restored only when its local `Tui` value dropped, and `run_standard` printed
  no successful interactive-exit message.
- Official MiMoCode source at
  `packages/opencode/src/cli/cmd/tui/routes/session/index.tsx` configures its
  exit message as a `Session` line containing the bounded title and a
  `Continue` line containing `mimo -s <session-id>`.
- Inferred: returning a small handoff value from `run_tui` and printing it in
  `run_standard` structurally guarantees output occurs after terminal restore.

## Scope

Allowed files:

- `src/main.rs` and focused tests
- `docs/.INDEX.md`, `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`,
  `NEXT_STEPS.md`, and this plan

Forbidden files/areas:

- `oracles/`, `reference/`, session/config/credential runtime state
- TUI snapshots unless a visible in-TUI surface changes
- installed binaries and Git history

Runtime state affected:

- None beyond existing normal interactive session persistence. Tests use
  in-memory state and existing disposable fixtures.

## Design and Invariants

- Add a bounded terminal-safe exit handoff derived from the same current
  session name and `session::public_session_id` used by `/session`.
- `run_tui` returns the optional handoff only after completing its existing
  owned-resource shutdown. `run_standard` renders it after `run_tui` returns,
  when the terminal lifecycle has necessarily dropped and restored stdout.
- Match MiMoCode's semantic layout: `Session` and `Continue` labels, with the
  public identifier embedded in the copyable continuation command.
- Omit the block when no session identity exists or persistence is disabled.
- Sanitize controls, collapse the title to one line, and bound it to 50 display
  cells without altering the stored title.

## Milestones

### M1 — Characterization

- Add a failing focused test for the exact name/continuation block, title
  sanitization/bounding, and omission boundaries.
- Validation: run the exact focused test before implementation.

### M2 — Implementation

- Add the pure handoff formatter.
- Change the interactive return boundary and print only in `run_standard`.
- Run session/CLI and focused lifecycle regressions.

### M3 — Validation and Harness Reconciliation

- Run minimum Rust completion gates from `TESTING.md`.
- Reconcile architecture, continuity, testing, next-step, and index records.
- Move this plan to completed only after required checks pass.

## Acceptance Criteria

- Ending a resumable interactive session prints its sanitized name and exact
  `ca -s ses_…` command.
- The handoff is emitted after alternate-screen/raw-mode restoration.
- Empty and persistence-disabled sessions print no handoff.
- `/session`, saved identity, `-s` parsing, headless output, and exit codes are
  unchanged.
- Focused and required completion checks pass with actual results recorded.

## Validation

Focused:

- Expected pre-fix compile failure for the new characterization.
- Exact formatter/omission test: PASS.
- Existing `/session`, short-resume, and public-ID tests: PASS.

Completion:

- Default suite: PASS, 1,602 main and 37 CLI tests; one ignored.
- All-target/all-feature suite: PASS, 1,631 main, 118 experiment, and 37 CLI
  tests; two ignored.
- Formatting, strict Clippy, all-feature release, released help/version,
  diff/snapshot hygiene, and oracle/reference immutability: PASS.

Manual/visual: not run. Safely isolating a native provider-backed TUI would
require runtime-state authority outside this task. Pure formatter coverage and
the structural return/print ownership boundary are the recorded evidence.

Blocked checks: native TUI walkthrough only, for the isolation reason above.

## Progress

- [x] Traced session identity/report and interactive shutdown boundaries.
- [x] Verified the MiMoCode exit-message shape from official source.
- [x] Added failing characterization coverage.
- [x] Implemented post-restore exit handoff.
- [x] Passed focused and completion checks.
- [x] Reconciled harness records and completed the plan.

## Decisions

- Decision: return the handoff across the TUI boundary instead of printing
  inside `run_tui`.
  Evidence: `Tui` owns alternate-screen/raw-mode restoration through Drop.
  Consequence: successful output cannot be swallowed by the alternate screen.

## Surprises and Discoveries

- The existing `/session` work already provides every identity primitive; only
  the process-exit presentation boundary is missing.

## Recovery and Rollback

All changes are local source/test/document hunks. Revert only SES-142 hunks;
do not reset, checkout, or disturb the broader dirty worktree.

## Completion Record

Changed files: `src/main.rs`, `CONTINUITY.md`, `ARCHITECTURE.md`, `TESTING.md`,
`NEXT_STEPS.md`, `docs/.INDEX.md`, and this plan.

Commands and results: focused formatter/session/identifier tests, default and
all-feature suites, formatting, strict Clippy, all-feature release,
help/version, and repository hygiene checks all pass with counts recorded
above.

Remaining risks: no native TUI walkthrough was run; output formatting and
omission boundaries are deterministic, and post-restoration ordering follows
from Rust ownership because printing occurs only after `run_tui` returns.

Harness updates: architecture, continuity, testing, next steps, and index now
describe the completed behavior and evidence. `PARITY.md` remains unchanged
because this is an intentional ca usability extension, not a Cursor claim.
