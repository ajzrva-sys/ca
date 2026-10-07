# SEC-120 — Agent-Harness Audit

Status: COMPLETE (2026-08-16)
Owner/session: Codex / current reliability continuation
Last updated: 2026-08-16

## Goal

Add one deterministic read-only scanner shared by `/audit` and `ca audit` for
the primary workspace and active profile's loaded/configured harness surfaces.
It must report bounded typed findings without a model, network, subprocess
execution, secret disclosure, or mutation.

## Non-Goals

- No automatic repair, permission change, definition rewrite, model judgment,
  network lookup, subprocess launch, or broad arbitrary-file scanner.
- No oracle/reference access or Cursor parity claim.

## Scope

Allowed: a dedicated audit module, CLI/TUI command routing, deterministic text
report/pager rendering, bounded local metadata/content inspection, focused
tests/fixtures/snapshots, and harness documentation.

Forbidden: runtime state mutation, provider traffic, command execution,
credentials, installed binaries, oracles/reference, branches/worktrees/stashes,
intermediate commits, or changes to unrelated workflows.

## Design Invariants

- Scan only declared instruction, skill, hook, MCP, permission, credential-
  reference, and executable surfaces belonging to the primary workspace or
  active profile.
- Bound file count, per-file bytes, depth, and aggregate evidence; any skipped
  or unreadable area makes the report visibly partial.
- Findings carry severity, stable rule ID, sanitized path/evidence, and
  remediation. Secret-shaped values are reported by field/type only.
- Detect unsafe ownership/permissions, symlink escape, writable harness paths,
  secret-shaped literals, suspicious executable resolution, remote MCP trust,
  broad permissions, invalid definitions, and incomplete scans.
- `/audit` uses the existing pager. `ca audit` exits 0 without high/critical,
  1 with high/critical, and 2 only when the audit itself cannot complete.

## Milestones

1. Add failing scanner, CLI, command-registry, and render characterization.
2. Implement the bounded typed scanner and shared deterministic report.
3. Integrate `/audit` and `ca audit`, then focused/manual/local/native gates.
4. Reconcile docs, archive this plan, and activate TUI-121.

## Validation

Focused: PASS. Five scanner tests cover determinism, secret redaction,
symlink escape, Unix permissions, invalid configuration, controls, mutation
absence, and bounded partial reports. Command/parser/CLI focused tests and the
reviewed wide/narrow loading/report/empty/error snapshots also pass.

Completion: PASS. Local default: 1,402 main plus 37 CLI tests, one ignored.
Local all-feature: 1,431 main, 118 experiment, and 37 CLI tests, two ignored.
Formatting, strict all-feature Clippy, all-feature release, help/version/audit
help, diff check, and snapshot hygiene pass.

Manual/visual: PASS. A disposable-home TUI walkthrough showed the searchable
command, scanning state, complete report, pager close, and clean terminal
restoration without provider or external network work. The fixture was moved
to Trash and no user runtime or installed binary changed.

Physical FreeBSD: PASS on 15.1-RELEASE-p2 amd64 as unprivileged `aji`.
Focused audit tests pass; default passes 1,389 main plus 37 CLI tests; all
features pass 1,418 main, 116 experiment, and 37 CLI tests, with the two
documented ignores. Native formatting, strict Clippy, release, help/version,
audit help, snapshot hygiene, x86-64 ELF identity, and linkage pass. An initial
root-run permission test was invalid because root can bypass the intended
directory mode; the corrected unprivileged exact-tree run passed. The
disposable source tree was removed.

## Progress

- [x] Plan activated after LSP-119 completion.
- [x] Failing characterization tests added before the scanner implementation.
- [x] Scanner and both entry points implemented.
- [x] Focused/local/manual/FreeBSD gates complete.
- [x] Harness reconciled and plan archived.

## Recovery

SEC-120 is read-only. Source rollback removes only its module, command routes,
tests/snapshots, and documentation while retaining OBS-117 through LSP-119.
No persistent migration or cleanup is required.
