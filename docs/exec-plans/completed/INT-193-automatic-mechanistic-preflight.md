# INT-193 — Automatic Mechanistic Preflight and Governed Verification

Status: COMPLETE
Owner/session: current Codex implementation session
Last updated: 2026-08-27

## Goal

When an ordinary Main/headless CA profile enables `mcp.mechanistic`, CA should
obtain bounded deterministic repository context before its first provider
request and should gate continuation after repository edits on codex-team-owned
focused verification, without widening the delegate MCP tier or creating
recursive CA/codex-team orchestration.

## Non-Goals

- No model-facing delegation in Plan, review, checkpoint, service, scheduled,
  subagent, bare, or `role-run` requests.
- No arbitrary shell/script execution, network authority, product edits,
  commits, service activation, or protected-tree mutation.
- No clean-tree gate, baseline commit, reset, stash, or unrelated cleanup.

## Evidence and Current Behavior

- `src/mechanistic_delegation.rs` implements the first-turn facade and host-owned
  polling, but the model must still select and construct every action.
- `src/main.rs::start_llm_turn` assembles request-only logical-turn context and
  is the provider boundary where automatic preflight must finish first.
- `ResolvedToolResult` and `FileMutationReceipt` retain exact successful edit
  paths, while repository revision receipts can detect unscoped mutations.
- `crates/codex-team/src/app.rs` already contains affected-work,
  file-affinity, verification-hint, repo-index, and cache-key scaffolds.
- The installed profile is delegate-only. The MCP catalogs remain 11/15/28
  tools with zero resources; ordinary CA repair therefore remains in the
  current provider turn while codex-team-owned workflows retain `ca role-run`.
- The production isolation backend currently passes as
  `macos-seatbelt-source-projection-v1`.

## Scope

Allowed files: protocol, CA mechanistic/config/request/event/schedule modules,
codex-team mechanistic/MCP/verification/learning modules, focused tests, and
current continuity/testing/architecture documentation.

Forbidden files/areas: `oracles/`, `reference/`, credentials, default profile,
session state, Git index/history, and unrelated dirty files.

Runtime state affected: only temporary test roots until the completion gates
pass; then recoverable replacement of `~/.local/bin/ca` and
`~/.local/bin/codex-team` is authorized. The named profile bytes remain
unchanged.

Pre-edit SHA-256 digests for the initially expected integration files were
captured in the initiating turn. Exact bytes must be rechecked before each
narrow edit if another process is observed changing the repository.

## Design and Invariants

- Add bounded composite query/evidence variants to the shared protocol; expand
  them only inside codex-team through the existing tracked-file and typed
  verification boundaries.
- Automatic intent recognition is deterministic, local, bounded, and sends
  only extracted operands—not the raw prompt—to codex-team.
- Preflight evidence is request-only context, cached only in process memory,
  and reused by retry only at the same repository revision.
- A successful typed edit batch triggers focused verification before provider
  continuation. Unknown mutating tools trigger full verification only when the
  repository revision actually changes.
- Real verification failure blocks successful completion and permits at most
  two current-turn repair continuations. Failed or stale evidence never becomes
  silent fallback.
- Only successful isolated evidence can be cached or contribute to learning.
  Explicit host-unisolated execution is advisory and never authoritative.
- Event and schedule execution are bounded, deduplicated, model-free where
  specified, and do not expose delegation to excluded model surfaces.

## Milestones

### M1 — Characterization and contracts
- Add failing protocol/config/intent/continuation tests for the new behavior.
- Implement shared composite types and closed schemas.
- Validation: focused protocol and configuration tests.

### M2 — Codex-team execution
- Implement bounded composite queries, affected verification selection,
  receipt metadata, isolation-aware success cache, and failure diagnosis.
- Reuse the existing MCP executors/jobs and learned-manifest lifecycle.
- Validation: focused mechanistic, verification, MCP, and learning tests.

### M3 — CA automation coordinator
- Run deterministic preflight before first provider I/O and inject compact
  request-only evidence.
- Capture tool-batch revisions/mutation receipts, gate continuation on durable
  verification, enforce repair limits, and surface jobs/toasts.
- Validation: focused main/headless/tool-batch/retry/exclusion tests.

### M4 — Scheduled model-free execution and documentation
- Extend schedule V2 with a mutually exclusive pure query/evidence payload,
  preserving V1 and all model-facing exclusions.
- Reconcile continuity, testing, architecture, and task index documentation.
- Validation: focused schedule/event/catalog tests.

### M5 — Completion and installation
- Run the broad workspace gates once, plus snapshot/protected-tree/release/MCP
  and isolation checks.
- Recoverably install both validated release binaries and smoke-test the named
  profile without live provider traffic.

## Acceptance Criteria

- “Fix the left-arrow behavior” receives relevant tracked code/test/diff context
  before provider I/O and a typed edit triggers only focused checks.
- Query ambiguity or explicit opt-out produces no automatic call.
- Unchanged shell/process work does not trigger verification; a changed
  unscoped repository revision triggers the full detected test suite.
- Provider continuation waits for verification and cannot report success while
  authoritative evidence is failing; two repair continuations are the maximum.
- Retry never re-executes completed preflight, edits, or verification at the
  same revision.
- Scheduled mechanistic jobs use no provider/session and scheduled model
  requests still omit the facade.
- Catalogs remain 11/15/28 and resources remain empty.

## Validation

Focused: protocol, mechanistic query/executor, MCP, config, intent,
tool-batch/continuation, retry, schedule, learning, and exclusion filters.

Completion:

```text
cargo test --workspace --no-fail-fast -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace --release --all-features
git diff --check
```

Manual/visual: `/delegate`, `/jobs`, exact MCP discovery, automatic preflight
with a hermetic provider, scheduled model-free receipt, clean shutdown, release
help/version, production isolation probe.

Blocked checks: live provider traffic is not authorized or required.

## Progress

- [x] Source boundaries, current behavior, safety constraints, and pre-edit
  integration-file digests characterized.
- [x] Composite protocol and receipt contracts implemented.
- [x] Codex-team composite execution, cache, and learning hooks implemented.
- [x] CA preflight and governed verification implemented.
- [x] Model-free schedule/event path implemented.
- [x] Focused and completion validation recorded.
- [x] Release binaries installed recoverably and smoke-tested.
- [x] Harness reconciled and plan moved to completed.

## Decisions

- Decision: enabling the existing mechanistic profile enables automation by
  default, with explicit per-feature opt-outs.
  Evidence: user selected existing-profile activation.
  Consequence: the installed profile requires no mutation.
- Decision: classification is deterministic only.
  Evidence: user selected deterministic recognition.
  Consequence: ambiguous requests go directly to the provider.
- Decision: verification gates provider continuation and allows two repair
  continuations in the same CA turn.
  Evidence: user selected gated continuation, two repairs, and current-turn
  ownership.
  Consequence: delegate authority and recursion exclusions remain unchanged.
- Decision: successful unknown-scope mutations run the full detected suite, but
  revision-unchanged process calls do not.
  Evidence: user selected full verification for unknown mutations.
- Decision: unisolated evidence requires an explicit profile override and is
  advisory only.
  Evidence: user selected explicit host override after the network/isolation
  limitation was explained.

## Surprises and Discoveries

- The affected-work and action-cache types already exist but the cache is inert
  and the current verification runner records misses unconditionally.
- CA's typed mutation receipt supplies one exact path, allowing verification to
  avoid Git-based absorption of unrelated dirty files.
- The first broad suite exposed a provider-strict schema regression after the
  composite action schema gained reusable definitions. Role schemas now inline
  local definitions before making all object properties required, while the
  public task-import MCP schema stays backward-compatible and self-contained.
- The real 28-tool fixture found the two task-import schemas were the only
  affected operate entries; exact 11/15/28 compatibility passes after the
  boundary fix.
- The old release-readiness audit still treated unrelated dirty/untracked and
  plan bookkeeping as static blockers. Those facts are now informational and
  the checkout is compared to its captured pre-gate state so validation-caused
  mutation still fails closed.

## Recovery and Rollback

Keep edits narrow and use the pre-edit digests plus ordinary diffs to separate
INT-193 changes from existing user changes. Do not use destructive Git
commands. Installation must retain timestamped hashed backups and restore them
if hash/help/version verification fails.

## Completion Record

Changed files: the shared protocol and generated schemas; codex-team
mechanistic query/executor, verification/cache, MCP/schema, learning, and
release-readiness boundaries; CA config, MCP facade, automatic preflight,
tool-batch verification, retry/interaction, event coordination, headless/TUI,
role-run exclusions, and Schedule V2 boundaries; focused tests and repository
continuity/testing/architecture/integration documentation. Protected trees,
the default profile, credentials, sessions, Git index, and Git history were not
modified.

Commands and results: six protocol tests, twelve composite tests, 27 MCP
server tests, six real SDK tests, focused CA intent/event/schedule/retry and
42+17 role-run exclusions, four learning tests, and 14 release-readiness tests
pass. The one broad workspace run passed 2,085 CA tests with two ignores, 38
CLI, 17 role CLI, 15+3 ca-watch, 13 codex-team integration, and six protocol
tests. It exposed one strict-schema regression plus one non-reproducing
clean-filter failure; after the schema fix the complete failed codex-team
library target passed 590 tests with one ignore. Formatting, strict workspace
Clippy, all-feature release, diff/snapshot/protected checks, production
Seatbelt, exact release and installed 11/15/28 compatibility, reverse release-
CA compatibility, help/version, `/delegate`, `/jobs`, and clean shutdown pass.

Remaining risks: deterministic intent parsing is deliberately conservative,
so ambiguous requests still reach the provider without preflight. A linker
compact-unwind size warning appears in debug test linking on macOS but does not
fail strict Clippy or release builds. The root remains intentionally
uninitialized for codex-team operate mode. Live provider traffic was not
required or used.

Harness updates: root and nested continuity/testing records, architecture and
integration ownership documentation, and `NEXT_STEPS.md` now record INT-193 as
complete. Validated binaries are installed at `/Users/aji/.local/bin`; backups
are `/Users/aji/.local/bin/ca.backup-int193-20260827-153936` and
`/Users/aji/.local/bin/codex-team.backup-int193-20260827-153936`. The existing
named profile remains byte-identical and mode 0600.
