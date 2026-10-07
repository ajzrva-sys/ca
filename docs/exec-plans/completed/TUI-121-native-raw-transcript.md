# TUI-121 — Native Raw Transcript

Status: COMPLETE (2026-08-16)
Owner/session: Codex / current reliability continuation
Last updated: 2026-08-16

## Goal

Add visible argument-free `/raw` that renders a deterministic sanitized
plain-text transcript from canonical visible conversation data, suspends the
alternate screen, prints into native terminal scrollback, waits for Enter,
and restores the TUI on every outcome.

## Non-Goals

- No headless/service output mode, session-format change, provider request,
  raw system prompt, hidden reasoning, private email content, credentials, or
  unavailable binary/media bytes.
- No oracle/reference access or Cursor parity claim.

## Scope

Allowed: a display-only transcript projection, slash-command routing,
terminal suspend/restore orchestration, bounded sanitized tool summaries,
focused tests, a disposable manual walkthrough, and documentation.

Forbidden: canonical-message mutation, export/session changes, user runtime
state, installed binaries, oracles/reference, branches/worktrees/stashes,
intermediate commits, or unrelated terminal redesign.

## Design Invariants

- Preserve canonical visible user/assistant/tool ordering while omitting
  private or unavailable content explicitly.
- Sanitize terminal controls and bound tool summaries without revealing raw
  binary/media payloads or private email material.
- Leave alternate-screen/raw-mode state before native output, wait for Enter,
  and restore a fully redrawn TUI even after presentation failure.
- Reject `/raw` arguments deterministically and expose no equivalent in
  headless or service modes.

## Milestones

1. Add failing transcript-projection, command, and terminal-lifecycle tests.
2. Implement the bounded privacy-safe plain-text projection.
3. Integrate suspend/print/wait/restore and deterministic coverage.
4. Run focused, local, manual, and physical-FreeBSD gates.
5. Reconcile docs, archive this plan, and activate USG-122.

## Validation

Focused: PASS. Five projection/presenter tests cover ordering, every private
omission class, configured and secret-shaped credentials, controls, media,
Unicode-safe bounds, exact bytes, Enter acknowledgement, and I/O failures.
Registry and orchestration tests prove argument-free visible idle-only routing.

Completion: PASS. Local default passes 1,409 main plus 37 CLI tests; local
all-feature passes 1,438 main, 118 experiment, and 37 CLI tests. Formatting,
strict all-feature Clippy, all-feature release, help/version, diff check, and
snapshot hygiene pass.

Manual/visual: PASS. A disposable bare/no-session TUI with a loopback provider
showed the searchable command, native transcript, exact credential redaction,
Enter wait, full TUI restoration, and clean shutdown. The fixture was moved to
Trash and no user runtime or installed binary changed.

Physical FreeBSD: PASS on 15.1-RELEASE-p2 amd64 as unprivileged `aji`.
Focused tests pass; default passes 1,396 main plus 37 CLI tests; all features
pass 1,425 main, 116 experiment, and 37 CLI tests, with the two documented
ignores. Native formatting, strict Clippy, release, help/version, snapshot
hygiene, x86-64 ELF identity, and linkage pass. The exact disposable source
tree was removed.

## Progress

- [x] Plan activated after SEC-120 completion.
- [x] Failing characterization tests added before implementation.
- [x] Transcript projection and terminal lifecycle implemented.
- [x] Focused/local/manual/FreeBSD gates complete.
- [x] Harness reconciled and plan archived.

## Recovery

TUI-121 has no persistent format. Source rollback removes its command,
projection, terminal orchestration, tests, and documentation while retaining
OBS-117 through SEC-120.
