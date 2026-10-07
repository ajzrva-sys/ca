# HRS-250 — Hermes-Informed Context/Cache Compaction

Status: COMPLETE (2026-09-17; focused tests + fmt + clippy; not installed this turn)
Owner/session: Cursor/Hermes study execution 2026-09-17
Last updated: 2026-09-17

## Goal

Adopt the highest-value Hermes `2026.9.14` context/cache lessons into `ca`
without copying oracle source: lean-tail checkpoint fidelity (anchors + verbatim
user quotes + recall pointer), never-summarize-user invariant, in-place
compaction proof, and cache-aware skill-mutation status.

## Non-Goals

- No Hermes GUI/gateway/multiplexing surfaces.
- No middleware plugin contract (CA hooks remain authoritative).
- No oracle tree edits; no Python middleware port.
- Tier-2 micro-compaction, ESTOP sentinel, SQLite quarantine: deferred (not in this execution).

## Evidence (Hermes read-only)

- `oracles/hermes-agent-2026.9.14/website/docs/developer-guide/context-compression-and-caching.md`
- `.../micro-compaction.md`, `.../session-storage.md`
- CA sources: `src/context.rs` (`ContextCheckpoint`, `checkpoint_message`, `build_checkpoint_prompt`), skill status in `src/main.rs`

## Design shipped

1. `ContextCheckpoint` additive fields: `anchors`, `user_quotes`, `recall_pointer` (serde defaults; existing sessions load).
2. `checkpoint_message` emits Anchor index, verbatim User messages, Recall pointer sections.
3. `build_checkpoint_prompt` instructs the model not to summarize/replace user instructions.
4. `generate_checkpoint_*` fills anchors/quotes/pointer from the covered prefix.
5. In-place compaction: checkpoint prefixes the **same** canonical list (test).
6. Skill activate/deactivate status includes cache-aware note; optional `--now` suffix documents apply-now.
7. `docs/QUALITY.md` GRD-009a behavior-contract tests; `ARCHITECTURE.md` session-scoped capability note.

## Acceptance

- [x] Checkpoint projection carries anchors + verbatim user quotes + recall pointer
- [x] Prompt forbids summarizing user instructions
- [x] In-place same-session identity test
- [x] Skill mutation cache note test
- [x] Focused tests + fmt + clippy pass
- [x] No oracle/reference edits

## Completion Record

Changed files: `src/context.rs`, `src/session.rs` (checkpoint literals), `src/tui/render_tests.rs`, `src/main.rs` (literals + skill cache notes), `docs/QUALITY.md`, `ARCHITECTURE.md`, harness docs.

Deferred: micro-compaction, usage-anchor/cooldown, ESTOP sentinel, FTS quarantine, footprint-ladder lint.

Commands: focused `cargo test --bin ca checkpoint_* skill_mutation_cache_note`; `cargo fmt`; `cargo clippy --bin ca --all-targets -- -D warnings`.
