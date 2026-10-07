# SES-140 — Automatic Session Names and Public Identifiers

Status: COMPLETE
Owner/session: Codex implementation session
Last updated: 2026-08-18

## Goal

Give each interactive session a stable copyable `ses_…` identifier and a useful
automatic title derived from its first user request, then make `ca -s <id>` and
the session UI expose a direct continuation path.

## Non-Goals

- No model request, provider traffic, or added naming cost.
- No session schema or storage-key migration.
- No replacement of UUIDs at internal authority, hook, job, or persistence
  boundaries.
- No overwrite of an explicit CLI or `/name` title.
- No oracle parity claim or oracle/reference edit.

## Evidence and Current Behavior

- `SessionV4::new` already creates a UUID, and V4 serialization persists it.
- `save_current_session` creates that identity lazily and copies
  `AppState::session_name`; an empty name remains untitled.
- `--name` and `/name` are manual-only. There is no automatic title assignment.
- `--session <UUID>` resumes a session, but there is no `-s` alias or `ses_…`
  public identifier.
- `/session` prints the raw UUID, and the session picker shows only its first
  eight hexadecimal characters; neither surface prints a continuation command.
- Inferred: a reversible encoding of the existing UUID can provide a stable
  public identifier without changing serialization or introducing an identity
  mapping store.

## Scope

Allowed files:

- `src/session.rs`, `src/main.rs`, `src/tui/render.rs`
- focused tests and affected snapshots under `src/tui/`
- this plan and repository harness documents

Forbidden files/areas:

- `oracles/`, `reference/`, user runtime state, installed binaries, Git history

Runtime state affected: process-owned session title/identity and existing saved
session metadata through the normal save path only.

## Design and Invariants

- Encode the existing 128-bit UUID into fixed-width base62 with a `ses_` prefix;
  decoding must be exact, bounded, and reversible.
- Continue accepting UUIDs and legacy storage keys. A public ID resolves through
  the same UUID-checked session-store path.
- Allocate the normal in-memory `SessionV4` before the first provider turn so
  the first request and process-owned integrations share the final identity;
  persistence remains governed by existing run capabilities.
- When `session_name` is empty, derive a bounded Unicode-safe title from the
  first user message. Normalize whitespace, omit appended attachment bodies,
  and never overwrite explicit names.
- Keep UUIDs internal for authorization and compatibility. Public presentation
  and resume commands use the `ses_…` identifier.

## Milestones

### M1 — Characterize and add identity/title primitives

- Add failing tests for public-ID round trips, malformed IDs, store resolution,
  prompt title normalization/truncation, and explicit-title preservation.
- Implement bounded session helpers and early identity creation.

### M2 — Expose continuation UX

- Add `-s` as the short form of `--session`.
- Show public IDs in session filtering/pickers and show `ca -s <ses-id>` in
  `/session`.
- Add deterministic wide/narrow render coverage.

### M3 — Complete validation and handoff

- Run focused, default, all-feature, formatting, strict Clippy, release,
  help/version, diff/snapshot, and oracle/reference gates.
- Reconcile harness documents and archive this plan.

## Acceptance Criteria

- `public_session_id(id)` is stable, starts with `ses_`, and decodes to the
  exact UUID; malformed and overflowing values are rejected.
- Existing UUID and storage-key resume behavior remains valid.
- `ca -s ses_…` resumes the exact active saved session.
- The first user request assigns a nonempty bounded title before the first
  provider turn; `--name` and `/name` remain authoritative.
- `/session` and the session picker expose the public identifier, and
  `/session` prints a copyable continuation command.
- No session serialization field changes.

## Validation

Focused:

- public-ID round-trip/strictness, automatic-title, public store resolution,
  `-s` parsing, early identity/explicit-name, and session-report tests: PASS.
- all 155 deterministic render tests: PASS; wide/narrow report and session
  picker updates reviewed.

Completion:

- default: PASS, 1,600 main and 37 CLI tests; one ignored.
- all-target/all-feature: PASS, 1,629 main, 118 experiment, and 37 CLI tests;
  two ignored.
- formatting, strict Clippy, all-feature release, released help/version, diff,
  snapshot, and oracle/reference checks: PASS.

Manual/visual: reviewed deterministic 100×40/60×36 session report and
100×30/60×24 session picker snapshots. Native runtime state was not mutated.

Blocked checks: none.

## Progress

- [x] Current title, UUID, save, resume, picker, and report flows traced.
- [x] Failing focused characterization added.
- [x] Session identity/title behavior implemented.
- [x] Continuation UX implemented.
- [x] Completion gates and harness reconciliation complete.

## Decisions

- Decision: derive the public identifier reversibly from the existing UUID.
  Evidence: UUID is already the stable persisted identity at every internal
  boundary.
  Consequence: no new schema, lookup table, or migration is needed.
- Decision: use a local first-request title rather than another model request.
  Evidence: the requested example is a concise restatement of the prompt, and
  naming must not add provider cost, latency, or failure modes.
  Consequence: naming is immediate, deterministic, offline, and manually
  replaceable with `/name`.

## Surprises and Discoveries

- The first provider request can currently begin before `current_session` is
  allocated, so its tracing/session identity is absent until a later save.

## Recovery and Rollback

Revert only SES-140 hunks and generated snapshot updates. Do not reset the dirty
worktree. Existing UUIDs, saved sessions, and storage keys remain valid because
the schema and underlying identity are unchanged.

## Completion Record

Changed files: `src/session.rs`, `src/main.rs`, `src/tui/render.rs`,
`src/tui/render_tests.rs`, eight session report/picker snapshots, this plan,
`ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`, and
`docs/.INDEX.md`.

Commands and results: all focused, render, default, all-feature, formatting,
strict Clippy, release, help/version, diff/snapshot, and oracle/reference gates
listed above passed.

Remaining risks: local first-request naming is intentionally heuristic and may
be less polished than a model-generated title, but it is deterministic,
immediate, free, Unicode-safe, and remains replaceable through `/name`.

Harness updates: SES-140 is recorded complete in the repository index,
continuity, architecture, testing, and next-step documents. No parity claim was
added.
