# CTX-013 — Context Usage and Model-Generated Checkpoints

Status: COMPLETE (2026-08-09)
Owner/session: current Codex roadmap implementation
Last updated: 2026-08-09

## Goal

Replace destructive local compression with structured model-generated checkpoints while preserving the complete canonical transcript for display, sessions, branching, and export.

## Non-Goals

- Do not introduce tokenizer dependencies or provider-specific hidden limits.
- Do not summarize with tools or mutate canonical history.
- Do not implement background shell jobs or diff review.

## Scope

Allowed: context/config/session/app/client request assembly, event-loop checkpoint orchestration, command/render tests, harness docs.
Forbidden: oracle/reference edits, provider credentials in tests, unrelated tool schemas.

## Design and Invariants

- `ContextCheckpoint` records a complete-turn message boundary, structured summary, timestamp, and source-token estimate.
- Provider requests use system prompt + checkpoint note + post-checkpoint canonical messages.
- Session/application display always retains every canonical message.
- Automatic compaction occurs only for models with known windows, at explicit threshold or 80% by default.
- Checkpoint generation is a dedicated no-tools request. Failure leaves the prior checkpoint unchanged and records the attempted transcript length to prevent retry loops.

## Completed Milestones

### M1 — Context model and pure boundaries

- Added backward-compatible settings, estimates, complete-turn cutoff, request reconstruction, structured prompt, and fake-generator tests.

### M2 — Async checkpoint orchestration

- Added manual/automatic requests, success/failure events, persistence, stale-result rejection, rollback, and no-retry behavior.

### M3 — Context dashboard

- Added `/context`, functional `/compact`, and two wrapped wide/narrow dashboard snapshots.

### M4 — Validation and handoff

- Focused/full tests, targeted formatting, release build, and local smoke pass.
- Global formatting and strict clippy remain red only on the recorded pre-existing baseline.

## Acceptance Results

- Fake-model generation, boundary selection, tool-pair integrity, automatic/manual thresholds, failure rollback, restored checkpoints, and dashboard rendering are covered hermetically.
- Canonical session and application transcripts are not replaced during compaction.
- Existing live TUI PID 51440 was not restarted or terminated.

## Validation Evidence

- `cargo test context::tests --no-fail-fast`: PASS, 7 tests.
- Context dashboard snapshots: PASS, 2 reviewed snapshots.
- Session checkpoint restoration remains covered by the v2 round-trip test.
- `cargo test`: PASS, 136 tests.
- `rustfmt --edition 2021 --check src/context.rs`: PASS.
- `cargo fmt --check`: FAIL on pre-existing repository-wide drift.
- `cargo clippy --all-targets -- -D warnings`: FAIL on the same three pre-existing diagnostics; no CTX diagnostic.
- `cargo build --release`: PASS.
- Local CLI smoke and live PID check: PASS.

## Recovery and Rollback

Revert only CTX-013 changes. Never send a live compaction request during validation.
