# PAR-005 — Clean Cursor Evidence Lane

Status: COMPLETE — BLOCKED OBSERVATION RECORDED
Owner/session: Codex `/root`
Last updated: 2026-08-09

## Goal

Capture one clean reproducible Cursor user-visible slice, or—if no clean runtime exists—record a precise blocked observation and preserve `ca extension` labels.

## Non-Goals

- Treating patched/minified artifacts as observed behavior.
- Copying bundle content or using modified authless/provider wrappers.
- Contacting Cursor services or reading credentials.
- Changing `ca` behavior without a clean reference fixture.

## Evidence and Current Behavior

- `PARITY.md` states the live Cursor tree is patched and requires direct clean observation before a parity claim.
- The dated backup contains a clean bundle artifact, but its executable wrapper is locally modified, provider-specific, and tied to an unavailable developer path.
- The clean bundle is not a standalone CLI entrypoint under the bundled Node runtime.

## Scope

Allowed files: this plan, `tests/fixtures/parity/`, `PARITY.md`, `TESTING.md`, `CONTINUITY.md`, and `NEXT_STEPS.md`.

Forbidden: oracle/reference edits, credential output, network access, production behavior changes, installed binaries, user runtime state, and the live TUI.

Runtime state affected: an empty temporary home was created for the network-denied attempt and removed afterward.

## Design and Invariants

- Only a stock clean runtime with a known version can produce an `Observed` row.
- A failed launcher attempt does not become a behavior fixture.
- Diagnostics containing bundled implementation text are not retained or quoted.
- Blocked evidence leaves every parity row `Unverified` and preserves `ca extension` labels.

## Milestones

### M1 — Resolve clean runtime
- Inspected the artifact types and dated backup boundaries without modifying them.
- Rejected the modified wrapper as stock evidence.

### M2 — Safe local attempt
- Ran the clean bundle with empty temporary state and network denied.
- The unsupported direct entrypoint exited before CLI/TUI rendering; no valid slice existed.

### M3 — Record and reconcile
- Added a secret-free blocked fixture and updated parity/harness documentation.
- Re-ran repository completion checks to ensure documentation-only work did not alter behavior.

## Acceptance Criteria

- The fixture records the reference artifact, attempted input, environment, missing version/dimensions/keystrokes/output, and reason each is unavailable.
- It contains no bundled source, secret, token, certificate, or provider value.
- No parity row is promoted.
- No oracle or runtime state remains changed.

## Validation

Focused: manual review of the blocked fixture for sensitive/bundled content.

Completion: 175 unit + 3 CLI tests, format, release, help/version, and temp-cleanup check PASS; strict Clippy retains the 3 documented pre-existing errors.

Manual/visual: blocked before a TTY-rendered slice; no visual claim.

Blocked checks: clean Cursor version/startup/composer behavior.

## Progress

- [x] Resolved available clean/patched artifact boundaries.
- [x] Attempted a network-denied empty-home local exit.
- [x] Recorded the blocked fixture without retaining diagnostic bundle text.
- [x] Completed final repository gates and harness reconciliation.

## Decisions

- Decision: the modified dated wrapper is not a clean runtime.
  Evidence: it is an authless/provider-specific launcher tied to an external developer installation.
  Consequence: no output from it may support parity.

- Decision: do not promote help strings extracted from the clean bundle to observed behavior.
  Evidence: the bundle could not execute as a standalone CLI.
  Consequence: clean-backup strings remain corroborating artifact evidence only.

## Surprises and Discoveries

- The first strict sandbox profile was incompatible with the bundled Node allocator; a second network-denied profile reached the bundle and established that the direct entrypoint is unsupported. Neither produced a user-visible Cursor slice.

## Recovery and Rollback

Delete only this plan and the blocked fixture, then revert the small harness-doc edits. No product/runtime rollback exists.

## Completion Record

Changed files: documentation and one textual blocked fixture only.

Commands and results: full tests, format, release, help/version, and temporary-directory cleanup PASS; strict Clippy unchanged at 3 unrelated findings.

Remaining risks: a future stock Cursor installation/version is required to reopen observation.

Harness updates: `PARITY.md`, `TESTING.md`, `CONTINUITY.md`, and `NEXT_STEPS.md` reconciled.
