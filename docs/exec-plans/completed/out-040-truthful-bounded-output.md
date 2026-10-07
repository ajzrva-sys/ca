# OUT-040 — Truthful Bounded Output

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-10

## Goal

Make file, job, web, generic, and headless output bounds explicit and truthful while preserving typed execution outcomes and ownership.

## Non-Goals

- Do not add PDF extraction, spill files, or a broad whole-input audit.
- Do not change approval/sandbox authority or provider protocol.
- Do not touch user runtime state or oracle/reference files.

## Evidence and Current Behavior

- `read_file` buffers the entire file, truncates to 8 KiB, then generic handling truncates again near 4 KiB without range metadata.
- Jobs retain only stream tails, foreground waits lose their originating owner, and model outcomes infer success from formatted text.
- WebFetch buffers the entire decoded body before silent double truncation.
- Ante documents bounded tool output; this work adopts the product lesson as a `ca` extension, not runtime/parity evidence.

## Scope

Allowed: built-in registry/schemas and handlers, job captures, headless previews, deterministic loopback/temp-file tests, TUI snapshots, and harness documents.
Forbidden: oracle/reference edits, unbounded response retention, real provider/user runtime mutation.

## Design and Invariants

- Every bound is UTF-8 safe and reports exact omitted bytes or unread response state.
- `read_file` streams a 1-based inclusive line window, defaults to 200 lines, rejects spans above 500, and caps its complete result at 64 KiB.
- Job stdout and stderr retain independent head/tail captures with exact total/omitted counts. Model formatting shares a near-4-KiB budget across non-empty streams.
- Job status, not formatted text, determines success/failure/cancelled; foreground waits keep the creator's owner.
- WebFetch consumes decoded chunks only through 1 MiB, rejects unsafe status/content type without retaining bodies, honors the configured timeout, and emits at most 4 KiB with an honest marker.

## Milestones

- [x] Implement ranged streaming `read_file` and schema/tests.
- [x] Implement independent truthful job captures, typed outcomes, and owner fix.
- [x] Implement bounded streaming WebFetch, UTF-8-safe generic/headless clipping, and tests.
- [x] Review snapshots, run completion gates, reconcile docs, and archive this plan.

## Acceptance Criteria

- File reads report emitted range, total line/byte facts, omissions, and a next-range hint without whole-file buffering.
- Neither stdout nor stderr can silently hide the other; nonzero/timeout/cancel classifications remain typed.
- WebFetch never stores an over-cap body and never returns an HTTP error body or unsafe binary payload.
- All clipped generic/headless output carries an explicit omission marker and remains valid UTF-8.

## Validation

Focused file/job/web/headless/render tests, then `cargo test --no-fail-fast`, format, strict Clippy, release build, and help/version smoke tests.

## Completion Record

Completed on 2026-08-10. Focused ranged-file, job capture/outcome/owner, loopback WebFetch, generic Unicode, headless preview, and render tests passed. Four wide/narrow output snapshots were reviewed. Repository gates passed with 417 unit tests and 23 CLI integration tests, formatting, strict Clippy, release build, and rebuilt help/version smoke tests.

## Recovery and Rollback

Revert only OUT-040 files with targeted patches. No config/session migration or runtime cleanup is required.
