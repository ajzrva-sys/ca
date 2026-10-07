# REL-215 — CUR-204 Audit, Documentation, and Installation

Status: COMPLETE (2026-08-29)
Owner/session: CUR-204 goal session
Last updated: 2026-08-29

## Goal

Prove the integrated CUR-204 feature set, reconcile durable documentation and
initial-tree ownership, then perform one recoverable verified installation.

## Non-Goals

No provider/hosted request, commit/push/tag, oracle mutation, cleanup of user
changes, or installation before every gate passes.

## Evidence and Current Behavior

The exact initial status payload/digest and overlap audit are stored by the
umbrella plan. The currently installed binary was last changed by PRV-201 and
must remain recoverable until this plan completes.

## Scope

Allowed: CUR-204 source/tests/docs/Cargo files, build output, and the authorized
final binary path plus one timestamped recovery copy. Evidence trees and user
runtime configuration/sessions remain forbidden.

## Design and Invariants

Audit every current path against the decoded baseline or CUR-204 manifest.
Reconcile all umbrella/child plans, architecture, continuity, queue, parity,
testing, and index. Source comparisons cannot claim observed runtime parity.
Run all gates serially as listed by CUR-204. Preserve old binary as timestamped
SHA-labeled regular file, stage candidate on the destination filesystem, atomically
rename, and verify candidate/staged/installed hashes, mode/owner, strict
signature, path lookup, help/version, and representative local commands. Any
integrity failure restores backup and reports failure.

## Milestones

### M1 — Integrated audit

Finish all child plan records; validate schemas, public interfaces, default
compatibility, changed-path attribution, snapshots, and protected trees.

### M2 — Completion gates

Run both exact workspace matrices, format, strict all-feature Clippy, all-feature
release, smokes, signature, diff and snapshot hygiene, and protected verifier.

### M3 — Recoverable install

Record hashes/mode/owner, preserve backup, stage/rename/verify, restore on any
failure, then move completed plans and complete the goal.

## Acceptance Criteria

Every child is complete; every required gate actually passes; no `.snap.new` or
protected-tree change exists; exact path ownership is accounted; installed
bytes match the validated candidate and all local smokes pass.

## Validation

Exactly the umbrella completion matrix plus strict code signature and install
integrity checks. No network/provider check.

## Progress

- [x] Child plans complete and reconciled.
- [x] Exact baseline/path audit complete.
- [x] All completion gates pass.
- [x] Recovery copy and atomic install verified.
- [x] Plans moved and docs reconciled; goal completion is the final host action.

## Decisions

One final installation avoids validating or exposing partial child binaries.

## Surprises and Discoveries

- Two protected `codex-team` integration tests exposed pre-existing timing
  races during repeated complete matrices. Both exact matrices have clean full
  observations, every CA target passed throughout, and each protected failure
  passed on isolated rerun. Protected source remained byte-untouched.
- The first installation transaction was rejected before execution because its
  cleanup contained a force-delete command. Two later transactions installed
  and then correctly restored the old binary when smoke assertions were flawed:
  first a pipefail/SIGPIPE artifact, then incorrect case in the shell marker.
  Direct candidate inspection resolved both harness issues before the final
  transaction passed completely.

## Recovery and Rollback

If installation checks fail, atomically restore the preserved prior binary and
retain candidate/diagnostics. Never use destructive Git recovery.

## Completion Record

All eleven child plans are complete. Exact inventory comparison found all 810
baseline records still present with unchanged status and exactly 58 additive
CUR-204 paths: one lint configuration, 13 plans/evidence records, 18 source
modules, and 26 reviewed snapshots. There are no missing baseline paths or
additional status transitions. Direct staged/unstaged protected-tree checks and
`codex-team verify protected-trees --json` pass.

The default workspace matrix has a clean full observation (2,267 CA passes,
four documented ignores, plus every workspace/integration target). The exact
all-target/all-feature matrix passed 2,296 CA tests with four ignores, 118
experiment tests with one ignore, and every CLI, role, watcher, scheduler,
integration, protocol, example, and documentation target. Formatting, strict
all-feature Clippy, all-feature release build, diff/snapshot hygiene, strict
signature, help/version, ACP, repository-schema, worktree-preview, and shell
generator smokes pass. A later repeated default matrix hit the documented
protected cancellation race after all CA/library tests passed; its exact rerun
passed.

Candidate, staged, and installed SHA-256 are all
`30660c4e21598363feb3c99d11f271716f84e6f3e95907d07f633faa0af83111`.
`/Users/aji/.local/bin/ca` is a regular single-link executable owned by `aji`,
mode `0755`, strictly signed, selected by path lookup, and passed representative
local smokes. The prior SHA-256
`82c72a53a0092c1a274a92b550d5790d530fef39515247bd525144ec1fd603ea`
is preserved at
`/Users/aji/.local/bin/ca.recovery-20260829T022330Z-sha256-82c72a53a0092c1a274a92b550d5790d530fef39515247bd525144ec1fd603ea`.
No provider, hosted service, user configuration, session, oracle, reference, or
protected-tree source was read for credentials or mutated.
