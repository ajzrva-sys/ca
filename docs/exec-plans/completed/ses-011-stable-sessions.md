# SES-011 — Stable Sessions, Branching, and Timeline

Status: COMPLETE (2026-08-09)
Owner/session: current Codex roadmap implementation
Last updated: 2026-08-09

## Goal

Replace timestamp-per-save persistence and destructive conversation rewinds with stable version-2 JSON sessions, atomic upserts, searchable resume metadata, and explicit non-destructive branches.

## Non-Goals

- Do not add SQLite, a daemon, cloud sync, or oracle dependencies.
- Do not change filesystem `/undo` semantics.
- Do not implement checkpoint generation beyond the optional persisted field required for forward compatibility with CTX-013.

## Scope

Allowed: `Cargo.toml`/lock for UUID support, session/app/main command and TUI modules, focused tests/snapshots, harness docs.
Forbidden: `oracles/`, `reference/`, runtime user sessions during tests, unrelated command or provider refactors.

## Design and Invariants

- `SessionStore` owns an injected root; production constructs it from the existing user-data location.
- `SessionV2.id` is stable across saves. Atomic replacement uses a same-directory temporary file followed by rename.
- Version-1 content is converted in memory, with a deterministic ID derived from its filename; its file is rewritten as version 2 only on a later save.
- Branches always receive a new ID and record source ID plus selected completed-turn index.
- The source session and its message vector are never changed by branch creation.
- Timeline cut points occur only after completed assistant exchanges with resolved tool calls.

## Completed Milestones

### M1 — Store/schema compatibility

- Added v2 types, injected store, atomic stable upsert, v1 conversion, summaries, and six temp-root tests.

### M2 — Application integration

- Application state tracks the current session; completed turns use stable autosave; CLI resume and the picker load through `SessionStore`.

### M3 — Timeline and branching UX

- Added `/timeline`, full-history `/fork`, timeline-confirmed `/rewind`, typed input coverage, and four wide/narrow snapshots.

### M4 — Validation and handoff

- Focused and full tests, targeted formatting, release build, and local CLI smoke pass.
- Global formatting and strict clippy remain red only on the recorded pre-existing baseline.

## Acceptance Results

- Round trip, stable upsert, v1 migration, failure preservation, sorting/search metadata, full/historical fork, source immutability, and complete-turn boundaries have hermetic coverage.
- Every normal completed turn updates the current session instead of creating a new timestamped file.
- Resume and timeline UI expose stable identity and explicitly state that branch creation does not change source history.
- Existing live TUI PID 51440 was not restarted or terminated.

## Validation Evidence

- `cargo test session::tests --no-fail-fast`: PASS, 6 tests.
- `cargo test timeline_confirmation --no-fail-fast`: PASS, 1 test.
- session picker and timeline snapshot tests: PASS, 4 reviewed snapshots.
- `cargo test`: PASS, 114 tests.
- `rustfmt --edition 2021 --check src/session.rs`: PASS.
- `cargo fmt --check`: FAIL on pre-existing repository-wide drift.
- `cargo clippy --all-targets -- -D warnings`: FAIL on the same three pre-existing diagnostics, with no SES diagnostic.
- `cargo build --release`: PASS.
- `./target/release/ca --version` and `--help`: PASS.

## Recovery and Rollback

Revert only SES-011 files/blocks. Never delete user session files or touch the live process during validation.
