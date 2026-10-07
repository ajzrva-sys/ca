# ORC-218 — Complete Remaining Local Oracle Guidance

Status: COMPLETE
Owner/session: ORC-218 goal session
Last updated: 2026-08-29

## Goal

Implement, validate, document, and install every approved stable local
capability tracked by ORC-218 while preserving CA safety, compatibility,
active-plan ownership, and unrelated working-tree changes.

## Non-Goals

- No hosted account, subscription, telemetry, remote marketplace, automatic
  commit/push/merge, background auto-approval, arbitrary JavaScript workflow,
  model-created executable tool, or ambient login-shell snapshot.
- No Responses WebSocket provider dialect while the official surface remains
  beta-only; PRV-228 is a deferred readiness record.
- No paid provider or hosted-service request during validation.
- No modifications under `oracles/` or `reference/`, no destructive Git
  operation, and no mutation of user configuration, sessions, or credentials.
- Oracle observations remain implementation guidance, not runtime-parity claims.

## Evidence and Current Behavior

- AGT-157 is complete under its original ownership: deterministic
  artifact/closed-command predicates, source/workspace drift checks,
  serialized writable lanes, explicit cross-lane handoffs, host-created pass
  receipts, explicit declared human review, and monotonic graph budgets are in
  place. Later ORC acceptance packets may consume its receipt digests.
- CA already has stable ACP v1, declarative workflows and agent graphs,
  read-only review, metadata-only runtime diagnostics, foreground/background
  jobs with exactly-once completion delivery, bounded sessions, worktrees,
  permission enforcement, and verified local plugins.
- The pre-ORC checkout contains 562 tracked/indexed records and 306 untracked
  records. The canonical
  `git status --porcelain=v1 --untracked-files=all` SHA-256 is
  `2dcaf84dc86d3ae27bfea593bc811ce16f46f9782382780901fdcf890ff39978`.
  The exact 868-record gzip/base64 payload is retained in
  `ORC-218-dirty-baseline.status.gz.b64`.
- Direct status/diff inspection for `oracles/` and `reference/` was empty at
  program start. Existing changes under `crates/codex-team/` belong to prior
  protected-tree work and are baseline-only unless separately required and
  authorized.

## Scope

Allowed files: the existing AGT-157-owned paths; additive modules and focused
edits under `src/`; Cargo manifests/lockfile; focused tests and reviewed
snapshots; root architecture, continuity, parity, testing, queue, and index
documents; ORC-218 child plans and readiness records.

Forbidden files/areas: `oracles/`, `reference/`, unrelated baseline bytes,
credentials, provider payloads, user runtime configuration/session stores,
Git history, arbitrary remote code, and protected codex-team paths not already
owned by an explicit active plan.

Runtime state affected: disposable fixtures and build output until REL-229.
After all stable children and gates pass, exactly one recoverable atomic install
to `/Users/aji/.local/bin/ca` is authorized.

## Design and Invariants

- Execute sequentially: existing AGT-157, DIA-219, RES-220, REV-221, WFL-222,
  SYS-223, ART-224, PAN-225, EDT-226, IMP-227, PRV-228, REL-229.
- Only a valid host-created pass receipt or explicit human review releases an
  agent-graph dependency. Later acceptance packets may reference completed
  receipt digests but cannot manufacture verified claims.
- All persisted additions are versioned and defaulted; older configuration and
  sessions load without eager rewrites.
- New diagnostics, provenance, artifacts, acceptance records, and import
  previews store bounded metadata and digests, never raw secrets, hidden
  reasoning, provider payloads, or excluded transcript/tool content.
- Mutations retain existing approval, permission, sandbox, undo, drift,
  containment, and receipt boundaries. Background panes and reviewers never
  widen authority.
- Provider streaming guards preserve delivered output, never execute prose as a
  tool, and never replay automatically after usable output.
- Built-in workflows are immutable declarative CA definitions. No arbitrary
  runtime or automatic repository integration action is introduced.
- Stable ACP v1 remains the only ACP wire surface. Embedded context is explicit
  per-prompt user context and anchored edits remain host-verified.
- PRV-228 adds documentation only. Reopening WebSocket work requires an official
  stable contract and separate authorization.
- Installation occurs once, only after exact source, validation, protected-tree,
  artifact, process, signature, and binary-integrity checks pass.

## Active-Plan Overlap Audit

- `AGT-157`: prerequisite and sole owner of artifact predicates,
  workspace-lane serialization, and aggregate graph budgets. ORC-218 completes
  this plan in place and consumes its receipt digests later.
- `SEC-158`: service, approval, sandbox, and privilege boundaries. ORC-218
  does not expose privileged-secret flows or change service-v2 ownership.
- `REL-156`: reproducible release/CI ownership. REL-229 adds a separately
  recorded integrated install without rewriting REL-156 conclusions.
- `INT-151`: central agent/codex-team integration. Built-in workflows and
  acceptance records compose with, rather than reassign, its graph surfaces.
- `LRN-150`, `LRN-166`, and `LRN-170`: sessions, workflows, learning,
  commands, and TUI overlaps. ORC additions are defaulted, bounded, and keep
  learning ownership unchanged.
- Completed CUR-204/TST-216/TST-217 changes are part of the frozen baseline.
  ORC-218 will not reopen their protected-tree fixes.

## Milestones

### M1 — Prerequisite and diagnostics

Complete AGT-157 M2/M3, then implement DIA-219 support bundles and configuration
provenance with private atomic archives and secret-safe read-only explanation.

### M2 — Stream safety, review, and workflows

Implement RES-220 provider-neutral anomaly protection, REV-221 dual-route
convergence and immutable acceptance packets, then WFL-222 audited built-in
workflows and structure inspection.

### M3 — Active lifecycle and session state

Implement SYS-223 keep-awake/job stopping, ART-224 artifact ledger/browser, and
PAN-225 bounded pane-local interactive sessions.

### M4 — Editor interoperability and import

Implement EDT-226 stable ACP embedded context and hash-anchored editing, then
IMP-227 digest-bound transactional Claude setup import.

### M5 — Deferral and integrated release

Record PRV-228 stable-readiness requirements without provider code, then execute
REL-229 documentation, full validation, inventory audit, and one recoverable
installation.

## Acceptance Criteria

- Every stable child exposes the specified complete interface and passes its
  failure-first scenario matrix; placeholders or ignored options do not count.
- AGT-157 dependents cannot run on assistant assertion, drift, missing evidence,
  a bad receipt, a lane conflict, or exhausted aggregate budget.
- New archive/import/session formats are bounded, private where persisted,
  drift-checked, and backwards compatible through defaults.
- Review convergence requires two distinct routes and both pass; fact-check
  requires three explicitly selected distinct jurors. No same-route fallback.
- Background panes/jobs, keep-awake helpers, reviewers, and workflows terminate
  without orphan processes and without automatic mutation/network approval.
- No code or experimental public surface is added for PRV-228.
- Both workspace matrices, formatting, strict all-feature Clippy, all-feature
  release, smokes, signature, diff/snapshot hygiene, protected-tree checks, and
  exact inventory attribution pass before installation.
- Candidate, staged, and installed SHA-256 values match; mode, ownership,
  signature, path lookup, help/version, and representative local smokes pass.

## Validation

Focused: each prerequisite/child plan records its failure-first tests, focused
suite, formatting, strict all-feature Clippy, and release result.

Completion:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`
- `cargo test --workspace --all-targets --all-features --no-fail-fast -- --test-threads=1`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo build --workspace --release --all-features`
- help/version and every new command/schema smoke; strict signature;
  `git diff --check`; zero `.snap.new`; direct protected-tree status/diff and
  protected-tree verifier; orphan-process checks; exact baseline comparison.

Manual/visual: reviewed wide/narrow snapshots for every specified new TUI
surface and PTY coverage for panes, focus, resize, restoration, and helper
cleanup.

Blocked checks: paid providers and hosted services are deliberately excluded.
Responses WebSocket implementation is deliberately deferred pending a stable
official contract.

## Progress

- [x] ORC-218 goal created with the exact requested objective.
- [x] Exact dirty baseline and protected/oracle state captured.
- [x] Active-plan overlaps recorded.
- [x] Umbrella ExecPlan and queue entry created.
- [x] Existing AGT-157 M2/M3 completed under its current ownership.
- [x] DIA-219 complete.
- [x] RES-220 complete.
- [x] REV-221 complete.
- [x] WFL-222 complete.
- [x] SYS-223 complete.
- [x] ART-224 complete.
- [x] PAN-225 complete.
- [x] EDT-226 complete.
- [x] IMP-227 complete.
- [x] PRV-228 deferral record complete.
- [x] REL-229 complete and installed.

## Decisions

- Decision: preserve the exact 868-record initial status as compressed canonical
  evidence.
  Evidence: the integrated checkout is intentionally large and dirty.
  Consequence: REL-229 can attribute additions without resetting user work.
- Decision: finish AGT-157 in place before creating dependent review/workflow
  behavior.
  Evidence: the user explicitly preserved its ownership and required it as a
  prerequisite.
  Consequence: acceptance evidence has one authoritative receipt boundary.
- Decision: defer Responses WebSocket code and public configuration.
  Evidence: the official current reference exposes the relevant event surface
  under Beta Responses multi-agent APIs, not CA's stable Responses contract.
  Consequence: PRV-228 is a readiness checklist only.

## Surprises and Discoveries

- The initial checkout has grown from CUR-204's 810 records to 868 records.
  Central files are already shared by multiple active/completed plans, so every
  hunk requires narrow attribution and focused regression testing.
- AGT-157 exposed a pre-existing numeric-ordering defect in exact decimal USD
  values and two stale integration fixtures. Numeric ordering and both fixtures
  were repaired before its two complete workspace matrices passed.
- DIA-219's complete default matrix found only the expected command-discovery
  count change from the new slash command. After reviewing and accepting that
  one-line snapshot delta, the full CA target rerun passed 2,289 tests with
  four documented ignores; every other workspace target in the original
  matrix had already passed.
- Private support archives can be published atomically without overwrite by
  linking a fully synchronized mode-0600 staging file into place. This retains
  collision safety and permits cleanup to remain strictly writer-owned.
- RES-220's guard belongs in the concrete client producer rather than a
  provider decoder or turn assembler: that boundary has the advertised tool
  set, sees every decoded protocol event, can drop the live body immediately,
  and forwards safe earlier events through all existing interactive/headless
  consumers without changing their schemas.
- REV-221 requires complete usage subsets for every priced reviewer request;
  otherwise exact cost accounting fails closed. Its content-addressed
  acceptance packets retain only host-verifiable digests and safe metadata,
  so reviewer prose and tool output never become durable acceptance evidence.
- WFL-222's existing scheduler serializes ready nodes. Fact-check independence
  therefore comes from request-level isolation and identical dependency-bound
  prompts, not a false concurrency claim: each juror excludes the canonical
  transcript and prior verdicts, while the final report receives all three
  bounded outputs.
- SYS-223 attaches keep-awake authority to host lifecycle leases rather than
  model-visible state. A model job and its parent turn may overlap safely; the
  helper stops only after the last lease and a debounce. Graceful `await_job`
  stop reuses the existing owner and completion-delivery marker, so it cannot
  seize or duplicate another run's job outcome.
- ART-224 records root UUIDs and relative paths rather than absolute paths.
  File content remains outside session state, while a content digest permits a
  selected view to reject drift after containment and symlink revalidation.
  The complete CA target found two command-discovery fixtures whose broad
  fuzzy/count assumptions changed when `/artifacts` joined the registry; both
  were corrected and the clean 2,341-test rerun passed.
- PAN-225 keeps complete pane states behind a process-only layout owner and
  routes conversation events through pane IDs before the existing generation
  check. Existing-session split loading runs off the event loop; background
  mutation/network activity remains pending until that pane is focused. The
  clean serial CA target passes 2,353 tests with four documented ignores.
- REL-229's two complete matrices and every completion gate pass. PRT-230
  excludes exact `.DS_Store` components under an explicit v2 algorithm; after
  the separately authorized manifest-only commit, the HEAD-trusted verifier
  passes without modifying an oracle/reference byte. The release installation
  is recoverable and its installed bytes match the validated candidate.

## Recovery and Rollback

Reverse only ORC-218-labelled hunks and delete only ORC-created paths with
`apply_patch`; never reset or clean. Test persistence uses disposable roots.
The final installer retains a timestamped full-SHA recovery binary and restores
it if any integrity check fails.

## Completion Record

Changed files: ORC-218-owned source, focused tests/snapshots, Cargo metadata,
versioned records, and harness documentation recorded by the child plans.
Commands and results: every stable child is archived; both serial workspace
matrices, formatting, strict all-feature Clippy, all-feature release,
help/version/schema smokes, strict candidate signature, diff/snapshot hygiene,
baseline attribution, protected verification, orphan checks, and installed
binary verification pass. Exact counts and candidate hash are in `TESTING.md`
and REL-229.
Remaining risks: PRV-228 remains intentionally deferred until an official
stable Responses WebSocket contract exists and separate implementation
authority is provided.
Harness updates: architecture, continuity, parity, testing, queue, index,
capability audit, umbrella, and child plans are reconciled. ORC-218 is complete.
