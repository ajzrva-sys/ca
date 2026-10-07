# LRN-167 — Manual MiMoCode Session Learning

Status: COMPLETE
Owner/session: Codex / completed user-requested implementation
Last updated: 2026-08-23

## Goal

Make `ca learn run` harvest completed user/assistant evidence from the local
MiMoCode session database through the existing bounded, sanitized,
deduplicated Dream/Distill pipeline while autonomous service execution remains
disabled.

## Non-Goals

- Do not install, load, or enable a learning LaunchAgent.
- Do not import MiMoCode transcripts into `/sessions` or ingest MiMoCode
  memories, checkpoints, reasoning, tool traffic, files, or synthetic text.
- Do not write to the MiMoCode database, expose credentials, copy oracle code,
  or modify `oracles/` or `reference/`.
- Do not expand publication beyond memory and inert skills.

## Evidence and Current Behavior

- `src/codex_learning.rs` currently auto-registers and scans only
  `~/.codex/sessions` and `~/.codex/archived_sessions`; native `ca` sessions are
  merged later through `LearningCorpus`.
- The local MiMoCode store is
  `~/.local/share/mimocode/mimocode.db`. Read-only schema inspection found
  `session`, `message`, and `part` tables. Session rows contain an absolute
  `directory` and optional parent; message JSON distinguishes user/assistant,
  assistant `time.completed`, and errors; part JSON distinguishes visible text
  from reasoning, tools, files, and step records.
- Read-only MiMoCode oracle evidence in
  `oracles/MiMo-Code-main/packages/opencode/src/{storage/db.ts,session/session.sql.ts,session/message-v2.ts}`
  confirms the database location, main-agent slice, record ordering, text-part
  shape, and assistant completion marker. This is behavioral/schema evidence,
  not implementation source.
- `src/foreign_import.rs` already proves this workspace can open compatible
  SQLite histories read-only, but its explicit ordinary-session import is a
  different authority and does not enforce learning completion semantics.

## Scope

Allowed files: `src/codex_learning.rs`, the focused knowledge-enrollment root
validator in `crates/codex-team`, focused tests, learning architecture,
continuity, testing, next-step/index documentation, and this plan.

Forbidden files/areas: `oracles/`, `reference/`, provider secrets, MiMoCode
runtime files, installed binaries/services, and unrelated dirty-tree work.

Runtime state affected: none during implementation/validation. A later manual
`ca learn run` may update only the existing default-profile learning index,
queue, and approved memory/inert-skill journals.

## Design and Invariants

- Treat the MiMoCode database as one automatically discovered local evidence
  source. Open it read-only/query-only and derive stable per-session revisions
  from bounded selected rows, never from transcript persistence in `ca` state.
- Read only `agent_id = 'main'`. A completed turn consists of a user message and
  one or more following assistant messages with `time.completed`; assistant
  errors and unfinished assistant messages do not authorize a turn.
- Accept only non-synthetic, non-ignored `type = "text"` parts. Apply the same
  terminal sanitation, learning sanitation, secret-shaped rejection, message,
  session, source, and byte bounds as existing archive evidence.
- Partition by normalized `session.directory`, link fork roots through
  `session.parent_id`, and use content-bound message IDs so retries, database
  updates, forks, and repeated manual runs remain stable and deduplicated.
- Keep MiMoCode evidence inside the archive-learning index/corpus boundary; no
  ordinary saved session is created.

## Milestones

### M1 — Characterization

- Add a synthetic MiMoCode SQLite fixture covering complete/incomplete turns,
  visible/non-visible parts, secrets, workspace/fork metadata, and immutable
  reading.
- Validation: focused MiMo scanner tests fail before implementation.

### M2 — Import and harvest wiring

- Add the bounded read-only scanner and automatically discover the default
  MiMoCode database beside Codex roots.
- Extend combined-harvest, source-count, deduplication, and materialization
  tests.
- Validation: focused `codex_learning` tests pass.

### M3 — Reconciliation and completion gates

- Update architecture, continuity, testing, next-step, and index documentation
  to distinguish manual MiMo learning from disabled background activation.
- Run focused tests, workspace tests, format, strict all-target Clippy, release
  build, help/version smokes, and diff hygiene.

## Acceptance Criteria

- A manual due harvest sees native `ca`, Codex active/archive, and MiMoCode
  evidence when their stores exist.
- MiMoCode completed user/assistant text reaches a transient learning corpus;
  incomplete, errored, hidden, synthetic, ignored, sensitive, oversized, and
  non-main records do not.
- Repeated scans and forked copies do not duplicate evidence, and source text
  is absent from persistent index state and ordinary `/sessions` storage.
- The MiMoCode database and sidecars are byte-for-byte unchanged by tests.
- No background service is installed or enabled.

## Validation

Focused: PASS. `cargo test --bin ca codex_learning::tests` passed 32
scanner/registration/harvest tests; the focused knowledge-enrollment root test
and macOS background-sandbox file-root regression pass. The status pager's
wide/narrow snapshots pass with the generic session-evidence heading.

Completion: PASS. `cargo test --workspace --no-fail-fast` passed 2,510 tests
with three manual-only ignores. `cargo fmt --all -- --check`, strict workspace
all-target/all-feature Clippy, the all-feature release build, release
help/version smokes, and `git diff --check` pass.

Manual/visual: PASS. A release preview scanned the live MiMo database in 1.14
seconds, observing 114 sessions, 567 completed candidate messages, two
secret-shaped skips, zero provider calls, and zero profile writes. The generic
“Session evidence learning” wide/narrow pager snapshots were reviewed. The
learning LaunchAgent remains absent from the user launchd domain.

Blocked checks: live provider publication requires configured external
credentials and is not required to validate local manual harvesting.

## Progress

- [x] MiMoCode database location and schema characterized read-only.
- [x] Failing scanner/harvest fixtures added.
- [x] Bounded read-only importer and manual harvest wiring implemented.
- [x] Documentation reconciled and completion gates recorded.

## Decisions

- Decision: extend the existing archive evidence boundary instead of importing
  MiMoCode conversations as ordinary `ca` sessions.
  Evidence: `/sessions` storage and learning evidence have separate authority
  and retention contracts.
  Consequence: MiMoCode text is transient during scan/materialization and the
  persistent learning index retains only digests and counts.
- Decision: keep this slice manual.
  Evidence: the user explicitly chose manual learning after service activation
  friction.
  Consequence: implementation must not install or enable a LaunchAgent.

## Surprises and Discoveries

- MiMoCode now uses a SQLite database plus WAL rather than per-session JSON.
  Its message schema provides a precise completion marker, so incomplete-turn
  rejection does not require timing heuristics.
- Incremental source registration previously rebuilt the index from only the
  newly previewed roots. Adding MiMo now rescans/preserves registered roots so
  existing Codex evidence cannot disappear until the next scheduled sync.

## Recovery and Rollback

Remove the MiMoCode source discovery and scanner branch plus its focused tests;
existing native/Codex state remains readable because no persisted field or
state version is added.

## Completion Record

Changed files: `src/codex_learning.rs`,
`crates/codex-team/src/self_improvement/supervisor.rs`, the focused learning
status render fixture/snapshots, `ARCHITECTURE.md`, `CONTINUITY.md`,
`TESTING.md`, `NEXT_STEPS.md`, `docs/.INDEX.md`, LRN-166 continuity records,
and this plan.

Commands and results: focused tests PASS; 2,510 workspace tests PASS with three
ignored; format PASS; strict Clippy PASS; all-feature release PASS;
help/version, live read-only preview, diff, snapshot, and service-off smokes
PASS.

Remaining risks: live provider publication was intentionally not run; it uses
the already tested supervised bridge. MiMo schema changes fail closed and will
require a fixture/parser update. Background activation remains deferred.

Harness updates: architecture, index, continuity, testing, next-step, and
LRN-166 deferred-activation records reconciled. No parity claim was added.
