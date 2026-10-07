# SES-050 — Session Import and Export

Status: COMPLETE (2026-08-10)
Owner/session: `/root`
Last updated: 2026-08-10

## Goal

Let users explicitly export the current conversation as bounded Markdown or normalized portable JSON, and import one local portable JSON file as a new sanitized, inert saved session without importing execution authority or unresolved tool state.

## Non-Goals

- Importing Markdown, remote URLs, arbitrary OpenCode/Cursor/Codex session formats, or raw internal Session V1-V4 files.
- Exporting or restoring queued drafts, active skills, checkpoints, work/review state, agent summaries, workspace roots, parent lineage, approvals, credentials, endpoints, images, or runtime jobs.
- Replacing an existing saved session, preserving a source UUID, importing into `bare`/`--no-session`, or changing the current session automatically.
- Cursor parity. MiMo/OpenCode documents session export/import commands and Kun documents safe Markdown export, but this plan is an oracle-informed `ca` extension without a clean Cursor fixture or runtime observation.

## Evidence and Current Behavior

- `src/session.rs::SessionV4` persists conversation messages together with workspace, parent, skills, work/review state, agent summaries, checkpoint, queue, provider/model, and pin metadata. Serializing that type directly would cross the portability boundary.
- `SessionStore::save` assigns storage by UUID and writes atomically; `/resume` already refreshes active/archive summaries through blocking workers and protects profile boundaries.
- `src/agent/client.rs::append_provider_messages` requires every assistant tool call to have a result but ignores `Role::System`; imports therefore need stricter local validation before persistence.
- `bare` and `--no-session` deliberately expose no persistent session store. Explicit file export can remain available because it is not session persistence; import must keep the existing no-persistence boundary.
- `oracles/MiMo-Code-main/packages/web/src/content/docs/tui.mdx` documents `/export` for the current conversation, while its CLI docs document local JSON export/import. `oracles/Kun-master/docs/kun-tui.en.md` documents Markdown export without silent overwrite. These are product-design lessons only.

## Scope

Allowed files:

- New `src/session_portability.rs` plus `src/{session,app,main}.rs`
- `src/tui/{commands,input,render}.rs`, focused tests, and deterministic snapshots
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`
- `Cargo.toml`/`Cargo.lock` only if a direct low-level file-open dependency is required for safe local import
- This ExecPlan

Forbidden files/areas:

- `oracles/`, `reference/`, real user configuration, Keychain entries, real saved sessions, external services, and the installed binary
- Session V5/lazy history, retention, archive deletion, remote import, or generic attachment/file-picker work

Runtime state affected:

- One user-selected export file, only after an explicit `/export` command and Cancel-first overwrite confirmation when it already exists.
- One new UUID-named active session in the selected profile after a successful `/import`. Tests and walkthroughs use only exact temporary roots.

## Design and Invariants

- Add visible idle-only `/export [markdown|json] [path]` and `/import <path>` commands. Export defaults to Markdown and an explicit generated filename in the current workspace; a `.json` suffix selects JSON when the format is omitted. The complete remaining argument is the path, so spaces work without shell evaluation.
- Export snapshots the current process conversation without forcing a session save. It emits either bounded UTF-8 Markdown or a `ca.session-export.v1` JSON document with only title, source provider/model labels, creation time, and normalized conversation messages/tool records. It contains no source UUID, workspace path, lineage, extension state, queue, checkpoint, runtime agent state, config, endpoint, or credential.
- Both portable formats have a 4-MiB encoded ceiling, 4,096-message ceiling, per-message/tool-field caps, and explicit errors rather than silent truncation. Markdown labels unresolved tool calls; JSON may preserve their typed status for recovery, but import rejects them.
- Export uses a 0600 same-directory temporary file and atomic rename. A missing target is created without confirmation. An existing regular file produces a dedicated Cancel-first overlay; confirmation is tied to the exact path/generation. Symlink, directory, and non-regular targets fail closed. Rename replaces the path entry rather than following a target symlink.
- Import accepts only a local regular file opened without following symlinks and read through a 4-MiB-plus-one bound. The root JSON object denies unknown fields and must identify `ca.session-export.v1`; raw Session V4 files and remote URLs are rejected before any session write.
- Import validates all counts and byte lengths, rejects system/legacy-tool roles, unresolved/pending/running calls, missing results, duplicate call IDs, invalid/non-object JSON arguments, invalid role/status values, and protocol-incomplete histories. External text is terminal-sanitized before persistence.
- A successful import creates a fresh V4 session with a new UUID and current workspace/provider/model/reasoning selection. It forces `pinned=false`, `parent=None`, and empty roots, skills, work state, reviews, agent summaries, checkpoint, and queue. Imported messages are inert historical values; no tool, hook, MCP, provider, or queue action runs.
- Import never replaces an existing session and does not auto-resume. Success refreshes `/resume`, opens its Active view, and selects the new row. Failure leaves the current/default session unchanged and remains inline in a portability overlay.
- All file parsing/writing and session-store mutation runs on one blocking worker. Busy states lock duplicate actions; stale generations are ignored. Bare/no-session block import before I/O with existing guidance, while export remains an explicit file operation.

## Milestones

### M1 — Characterize the portable boundary

- Add failing tests for command discovery/argument preservation and the absent portable schema, bounds, sanitization, unresolved-call rejection, UUID reset, and runtime-state stripping.
- Add storage tests for no-overwrite, confirmed atomic overwrite, regular-file-only import, and no-write failures.
- Validation: focused tests fail only on missing SES-050 behavior.

### M2 — Implement format and filesystem core

- Add the bounded portable schema, deterministic Markdown/JSON encoders, strict import sanitizer/validator, and safe local file operations.
- Build a fresh inert Session V4 only from explicitly accepted fields plus current runtime-safe defaults.
- Validation: portability/session focused tests pass without TUI orchestration.

### M3 — Add commands and asynchronous TUI flow

- Add typed process-only request/result/overlay state, command parsing, blocking-worker orchestration, stale-result handling, Cancel-first overwrite, import failure recovery, and successful `/resume` selection.
- Add reviewed wide/narrow snapshots for overwrite confirmation, export/import busy, import failure, and narrow path rendering.
- Validation: command/input/orchestration/render suites and snapshot review pass.

### M4 — Complete gates and isolated walkthrough

- Run focused suites, `cargo test --no-fail-fast`, formatting, strict Clippy, release build, and help/version/profile smoke tests.
- Under a temporary home/workspace, export Markdown and JSON, confirm no-overwrite/overwrite behavior, inspect bounded portable fields, import the JSON into default and named profiles, verify fresh UUID/runtime-state stripping/inertness, and exercise malformed/unresolved rejection. Remove or trash only the recorded temporary root.
- Reconcile the five harness documents, move this plan to completed, and mark SES-051 next without starting it.

## Acceptance Criteria

- `/export` writes bounded Markdown by default and normalized `ca.session-export.v1` JSON on request without persisting or mutating the current session.
- No export silently truncates. Existing regular files require an explicit Cancel-first confirmation; symlink/non-regular targets are never followed or overwritten.
- `/import` performs no network/tool/hook/MCP/provider action and writes nothing for an invalid, oversized, non-regular, raw-internal, or protocol-incomplete file.
- Every imported session has a new UUID, current safe runtime provider/model/workspace defaults, no lineage/pin/extensions/work/review/agent/checkpoint/queue state, and sanitized inert messages only.
- Unresolved tool calls, duplicate IDs, missing outputs, invalid arguments, and system/legacy tool messages are rejected before persistence.
- Import never overwrites an existing saved session; success opens `/resume` on the new active row without changing the current/default session.
- Bare and `--no-session` can export explicitly but cannot import or create session roots.
- Wide/narrow snapshots cover confirmation, busy, failure, and long-path behavior; all focused/full gates and the isolated walkthrough pass.

## Validation

Focused:

- `cargo test session_portability --no-fail-fast`: PASS, 16 schema, bounds, sanitization, filesystem, command, input, and orchestration tests.
- `cargo test session_export_ --no-fail-fast`: PASS.
- `cargo test session_import_ --no-fail-fast`: PASS.
- Reviewed eight wide/narrow portability snapshots plus the intentional `/export` and `/import` command-palette updates; zero `.snap.new` files remain.

Completion:

- `cargo test --no-fail-fast`: PASS, 557 unit tests plus 25 CLI integration tests.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- Rebuilt help/version/profile smoke: PASS; named-profile doctor created no missing profile/runtime files, and bare resume failed with the expected exit 2.

Manual/visual:

- PASS under disposable home/workspace `/tmp/ca-ses050.9sJAvi`: imported one fixture, resumed its exact markers, exported 0600 Markdown/JSON, cancelled an existing-file replacement by default, confirmed an atomic replacement after renaming, and rejected an unresolved `touch NEVER` call without creating the file.
- PASS for profile isolation: default and named `work` imports received distinct fresh UUIDs in separate roots, with current safe workspace/provider/model defaults and empty lineage/pin/queue/tool state.
- The walkthrough and smoke roots were moved to `/Users/aji/.Trash/ca-ses050.9sJAvi` and `/Users/aji/.Trash/ca-ses050-smoke.qVPLqi`; no real runtime state was used.

Blocked checks:

- None currently.

## Progress

- [x] Traced Session V4, provider-message validation, `/resume` mutation orchestration, and bare/no-session persistence boundaries.
- [x] Created the active ExecPlan before implementation.
- [x] Added failing portability, command, state/input, and filesystem characterization tests.
- [x] Implemented bounded portable formats and strict inert import validation.
- [x] Implemented asynchronous command/confirmation/import-result orchestration.
- [x] Reviewed all wide/narrow portability snapshots.
- [x] Passed focused and repository-wide validation gates.
- [x] Completed the isolated walkthrough and reconciled documentation.

## Decisions

- Decision: use a dedicated `ca.session-export.v1` schema rather than serializing Session V4.
  Evidence: Session V4 contains profile/workspace/runtime extension state that must not cross the import boundary.
  Consequence: portability stays narrow and can evolve independently of persistence schema versions.
- Decision: import creates a saved-but-not-resumed session using current runtime-safe provider/model/workspace defaults.
  Evidence: preserving imported runtime selection or workspace would let external files redirect later provider/filesystem behavior; auto-resume would unexpectedly replace the user's active task.
  Consequence: imported content is inert until the user explicitly resumes it, and even then carries no imported authority.
- Decision: unresolved histories may be exported for recovery but cannot be imported.
  Evidence: export is a user-directed data copy, whereas an imported unresolved assistant/tool suffix is not a valid provider continuation and could be mistaken for executable state.
  Consequence: Markdown/JSON can document failures, but only protocol-complete JSON becomes a saved session.
- Decision: import supports local normalized JSON only.
  Evidence: remote URLs add network/SSRF/redirect authority and foreign schemas add separate compatibility/security boundaries not approved in this slice.
  Consequence: no provider or network fixture is required and arbitrary oracle formats remain unsupported.

## Surprises and Discoveries

- `Role::System` is silently excluded by current provider-message conversion. Import must reject it explicitly rather than relying only on `AgentClient::build_messages`.
- The existing session type combines portable conversation content with workspace, skill, work, review, agent, checkpoint, and queued-input state; direct round-trip serialization would violate the approved inert-history boundary.
- A hard-link publication step is needed for no-overwrite export: a preflight existence check followed by ordinary rename would have a race that could replace a file created between those operations. Confirmed overwrite can use atomic rename because replacing that exact path is the approved action.

## Recovery and Rollback

- Code rollback is patch-only and limited to SES-050-owned files.
- A failed import writes no session. A failed export leaves the prior destination unchanged because content is written to an exact same-directory temporary file before rename.
- Tests and walkthroughs use exact disposable roots; real runtime paths and oracle files are never targets.

## Completion Record

Changed files:

- Added `src/session_portability.rs` and a direct `libc` dependency for safe no-follow/nonblocking local file opens.
- Updated `src/{app,main}.rs` and `src/tui/{commands,input,render,render_tests}.rs` for command routing, typed process state, blocking-worker orchestration, safety overlays, and deterministic snapshots.
- Updated command-palette and eight portability snapshots.
- Reconciled `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md`.

Commands and results: all focused tests, 557 unit tests, 25 CLI tests, format, strict Clippy, release build, local smoke checks, and the isolated default/named-profile walkthrough passed as recorded above.

Remaining risks: the portable schema is intentionally `ca`-specific and JSON-import-only; foreign/raw internal formats and Markdown import remain unsupported. Export is bounded by rejection rather than partial truncation, so very large conversations require a future explicit segmented-export design.

Harness updates: SES-050 is complete, documented as an oracle-informed `ca` extension, and SES-051 is the next sequential plan.
