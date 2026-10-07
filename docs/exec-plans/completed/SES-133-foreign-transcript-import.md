# SES-133 — Foreign Transcript Import

Status: COMPLETE
Owner/session: Codex / current user-requested implementation
Last updated: 2026-08-16

## Goal

Extend the existing explicit session-import surface with bounded, previewed,
inert adapters for user-selected Codex, Claude, and OpenCode histories without
carrying foreign execution authority into `ca`.

## Non-Goals

- Do not scan home directories, discover sources automatically, or modify any
  source transcript/database.
- Do not import system prompts, hidden reasoning, provider state, approvals,
  grants, tool authority/output, credentials, media bodies, or foreign routes.
- Do not begin LRN-134, opt imported sessions into learning by default, add a
  drawer, MCP sampling, or a subprocess handoff.

## Evidence and Current Behavior

- `src/session_portability.rs` owns bounded portable `ca` export/import parsing
  and regular-file-only local I/O.
- `src/session.rs` owns backward-compatible canonical sessions and atomic
  `SessionStore` publication.
- `/import` already has an asynchronous preview/confirmation path in
  `src/main.rs`, `src/tui/input.rs`, and `src/tui/render.rs`.
- The current configured route is process state, so imported inert transcripts
  can resume without preserving a foreign provider route.

## Scope

Allowed files: a typed foreign-import module, optional backward-compatible
session provenance, import command/state/render orchestration, bounded format
fixtures, deterministic tests/snapshots, dependency configuration if needed,
and harness documents.

Forbidden files/areas: `oracles/`, `reference/`, source histories, user runtime
state outside disposable validation, provider credentials, installed binary,
and foreign execution databases except immutable/read-only parsing.

Runtime state affected: after explicit confirmation only, one fresh inert `ca`
session per successfully parsed foreign source session in the active persistent
profile. Preview and parsing are read-only.

## Design and Invariants

- Parse only an explicit regular file or directory, following no symlinks and
  accepting at most 1,000 source files and 64 MiB total input.
- Codex and Claude adapters consume bounded JSONL. OpenCode consumes an exact
  SQLite file opened immutable/read-only; no journal, migration, or write is
  permitted.
- Normalize only visible user/assistant text and safe timestamps into fresh
  session UUIDs. A malformed item is counted/skipped without widening accepted
  content or smuggling tool/system/provider fields.
- Preview source kind, safe source label, sessions/messages, skipped material,
  errors, destination profile, and learning eligibility. Learning defaults off.
- Persist bounded provenance containing format/count metadata and a safe source
  label, never an absolute source path. Old sessions remain readable.
- Confirmation is the sole publication boundary; partial source failures do
  not corrupt or overwrite existing sessions.

## Milestones

### M1 — Characterization and typed adapters

- Trace current portable import, session-store publication, overlay state, and
  session compatibility.
- Add source-kind parsing, explicit safe traversal, bounded JSONL/SQLite input,
  visible-message normalization, provenance, and fixture tests.

### M2 — Preview, confirmation, and persistence

- Extend `/import` grammar and asynchronous preview without regressing portable
  `ca` JSON.
- Render complete preview/error states and publish fresh inert sessions only
  after explicit confirmation.

### M3 — Validation and completion

- Cover all formats, malformed/oversized input, symlinks/traversal, immutable
  SQLite access, partial failures, learning exclusion, resume route neutrality,
  legacy compatibility, and wide/narrow snapshots.
- Run focused/full gates and a disposable native PTY, reconcile the harness,
  archive this plan, and activate LRN-134.

## Acceptance Criteria

- All four `/import` forms remain explicit, bounded, and asynchronous.
- Preview never writes sessions and reports skipped/error counts honestly.
- Confirmation produces fresh sessions containing only visible inert history,
  safe timestamps, and bounded non-path provenance.
- Foreign authority/private fields are absent from canonical sessions, exports,
  diagnostics, and learning eligibility unless the user explicitly opts in.
- Existing portable import and old/new session records remain compatible.

## Validation

Focused: PASS. `cargo test foreign_import --no-fail-fast` (11), `cargo test
session_portability --no-fail-fast` (20), foreign-preview snapshot coverage,
and command-palette snapshot coverage all pass.

Completion: PASS. `cargo test` passes 1,532 main and 37 CLI tests with one
documented ignore. `cargo test --all-targets --all-features --no-fail-fast`
passes 1,561 main, 118 experiment, and 37 CLI tests with two documented
ignores. `cargo fmt --check`, strict all-target/all-feature Clippy, all-feature
release build, release help/version, `git diff --check`, and snapshot hygiene
pass.

Manual/visual: PASS. A released native TUI with a disposable home/workspace and
Claude JSONL source showed asynchronous inspection, a complete Cancel-first
preview, default-off learning, confirmation, and one new session in the picker.
The saved session retained only two visible messages, safe source timestamps,
the current route, and filename-plus-digest provenance; hidden system,
reasoning, tool, and private-output sentinels were absent. The fixture moved
recoverably to `/Users/aji/.Trash/ca-ses133-pty-20260816`.

Blocked checks: Physical FreeBSD was not requested or run for this extension.

## Progress

- [x] Current import/session seams fully characterized.
- [x] Bounded format adapters and provenance complete.
- [x] Preview/confirmation/session publication and snapshots complete.
- [x] Native walkthrough, full gates, and harness complete.

## Decisions

- Decision: imported messages are inert reconstruction, never a replayable
  foreign session.
  Evidence: foreign tools, grants, provider state, and route semantics cannot
  be safely mapped into `ca` authority.
  Consequence: only visible user/assistant text and safe timestamps survive.

## Surprises and Discoveries

- OpenCode's on-disk SQLite schema is not interchangeable with its exported
  JSON. Treating exact tables as a closed adapter and opening the database with
  both read-only and immutable flags prevents migrations, WAL/SHM creation, and
  accidental format widening.
- Canonical session storage is a metadata commit file plus generation-scoped
  JSONL history, so preserving imported source timestamps requires the inert
  publication path to avoid the ordinary save-time `updated_at` rewrite.

## Recovery and Rollback

Preview is read-only. Confirmed imports create new UUIDs and never overwrite an
existing session. Rollback can delete only those explicit new sessions through
the ordinary session manager; never mutate or delete the foreign source.

## Completion Record

Changed files: `Cargo.toml`, `Cargo.lock`, `src/foreign_import.rs`,
`src/session.rs`, `src/app.rs`, `src/main.rs`, `src/tui/commands.rs`,
`src/tui/input.rs`, `src/tui/render.rs`, focused tests/snapshots, and harness
documents.

Commands and results: all focused, default, all-feature, formatting, strict
Clippy, release, help/version, diff, snapshot, and native checks above pass.

Remaining risks: foreign producers can change their formats; closed adapters
will report bounded skips/errors rather than infer new authority. Learning
eligibility is stored but will become actionable only through LRN-134.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`,
`TESTING.md`, and `docs/.INDEX.md` describe the completed boundary and the next
active plan.
