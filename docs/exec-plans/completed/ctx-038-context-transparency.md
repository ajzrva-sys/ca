# CTX-038 — Context Transparency

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-10

## Goal

Make `/context` describe the complete prepared provider request by category and make persisted checkpoints visible without changing canonical messages.

## Non-Goals

- Do not add a provider tokenizer or persist request breakdowns.
- Do not start MCP servers merely to render `/context`.
- Do not edit Session V3 or oracle/reference files.

## Evidence and Current Behavior

- `/context` currently estimates only checkpoint-plus-suffix messages.
- Provider preparation separately adds instructions, skills, built-in/MCP tool schemas, hook context, and images.
- Checkpoint summaries already persist, but successful compaction is visible only as a fading status.
- Ante documents category reporting and a collapsed compaction marker; this is descriptive evidence for a `ca` extension, not parity evidence.

## Scope

Allowed: `src/context.rs`, shared app/request orchestration, TUI rendering/tests/snapshots, and current harness documents.
Forbidden: oracle/reference edits, credentials, user runtime state, provider-dependent validation.

## Design and Invariants

- `ContextBreakdown` stores estimated counts only; it never stores prompt text, schemas, image data, or secrets.
- The actual snapshot is produced after tool filtering and MCP discovery but before provider I/O.
- Before the first prepared request, `/context` uses a preview that does not call MCP discovery and labels MCP as unprepared.
- System prompt components are tagged for measurement while their concatenated provider bytes remain unchanged.
- Checkpoint markers are derived during rendering and summaries are sanitized before entering pager state.
- Canonical messages and persisted sessions remain unchanged.

## Milestones

- [x] Add pure breakdown types, attribution helpers, and characterization tests.
- [x] Integrate preview/actual snapshots and full-request threshold accounting.
- [x] Render the checkpoint marker and expanded dashboard with wide/narrow snapshots.
- [x] Run focused and completion validation, reconcile docs, and archive this plan.

## Acceptance Criteria

- `/context` reports instructions, skills, built-in tools, MCP tools, checkpoint, conversation, attachments, and an estimated total.
- Actual tool counts reflect the final provider tool list; preview rendering performs no MCP/network work.
- Automatic checkpoint decisions use the complete current estimate.
- A collapsed marker appears exactly at the covered transcript boundary and `/context` shows the sanitized summary.
- Existing request text, canonical messages, and Session V3 serialization remain unchanged.

## Validation

Focused context/unit/render tests, then `cargo test --no-fail-fast`, format, strict Clippy, release build, and help/version smoke tests.

## Completion Record

Completed on 2026-08-10. Focused attribution, prompt-byte, tool-partition, orchestration, and render tests passed. Four wide/narrow snapshots were reviewed. Repository gates passed with 397 unit tests and 23 CLI integration tests, formatting, strict Clippy, release build, and rebuilt help/version smoke tests.

## Recovery and Rollback

Revert only CTX-038 files with targeted patches. No migration or runtime-state rollback is required.
