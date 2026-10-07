# SES-076 — Lazy History Storage

Status: COMPLETED
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Store newly saved sessions as small versioned metadata plus an append-only indexed message log, continue reading V1–V4 monolithic JSON without mutation, migrate legacy sessions only after a successful save, and resume through a bounded recent page whose older pages can be prepended without moving the visible transcript anchor.

## Non-Goals

- Changing canonical `SessionV4` semantics, message/tool formats, branch lineage, profile roots, archive/pin/delete/retention policy, session portability formats, context-checkpoint behavior, or provider-visible conversation order.
- Compressing/encrypting history; loading history from a remote service; editing arbitrary historical log records in place; background pruning; cross-profile migration; changing default/bare/no-session behavior.
- Claiming Cursor parity. The approved oracle program names lazy history as an oracle-informed `ca` extension without a clean runtime fixture.

## Evidence and Current Behavior

- Before this plan, `SessionStore` scanned and fully parsed every `*.json` session for `/resume`; `load` read the entire file, and every save or metadata mutation rewrote the full pretty-printed `SessionV4` including `messages` (`src/session.rs`).
- The in-memory compatibility type is `SessionV4` (aliased as `SessionV2`) and V1–V3 are converted/defaulted only in memory until `save`; all branches, checkpoints, fallback provenance, goals, workflows, queues, work state, skills, and worktree attachment derive from this canonical type.
- `apply_loaded_session` currently clones the entire loaded `messages` vector into `AppState`; transcript rendering builds all lines, and PageUp/PageDown currently store an absolute top scroll value rather than a bottom-relative anchor (`src/{main,app}.rs`, `src/tui/{input,render}.rs`).
- Archive/restore uses exact UUID-checked hard-link transfer of one legacy JSON file; delete and retention likewise validate and unlink one exact regular file. The new multi-file representation must preserve those user-visible invariants and fail closed on incomplete/colliding bundles.
- The focused characterization baseline passed before storage edits; the completed implementation retains all legacy compatibility/lifecycle tests and adds crash, bound, generation, paging, and orchestration regressions.

## Scope

Allowed files: `src/session.rs`; narrow session-load/save/orchestration state in `src/{main,app}.rs`; resume/transcript input/render and focused snapshots in `src/tui/{input,render,render_tests}.rs`; session portability/retention adaptations only where the representation boundary requires them; this plan and required harness documents.

Forbidden: oracle/reference changes or copied code; real user sessions/config/Keychain; provider protocol/tool/approval changes; installed binary; destructive cleanup outside exact validated temporary/profile session paths.

Runtime state affected: profile-scoped saved-session files only when persistence is enabled. Tests and walkthroughs use disposable profile roots. Bare and `--no-session` remain fileless.

## Design and Invariants

### Storage representation and compatibility

- Keep the in-memory/durable semantic schema at Session V4. Add a separately versioned `SessionBundleMetadataV1` that serializes every V4 field except `messages`, plus an exact storage key, UUID, committed message count, active log generation/name, committed byte length, and per-message byte offsets.
- A bundle uses sibling regular files under one profile session root: the existing stable `<key>.json` path becomes the metadata commit point and `<key>.<generation>.turns.jsonl` is the active append-only log. Every JSONL record carries its zero-based message index and one canonical `Message`; one record is one line. File names are derived and validated from the exact one-component storage key and UUID/generation only.
- New sessions write a temporary, fsynced log and temporary metadata, publish the log first and metadata last, then fsync the directory when supported. A metadata file never references a missing, symlinked, non-regular, outside-root, wrong-key, wrong-UUID, wrong-length, or malformed log.
- Existing bundle saves append only when the committed records are an exact prefix of the supplied canonical messages and the physical log has no uncommitted tail. Append/fdatasync succeeds before the atomically replaced metadata advances count/length/offsets. A metadata failure leaves extra bytes uncommitted; the next save rotates to a new generation rather than truncating or trusting the tail.
- Any canonical rewrite (undo/rewind/retry/historical branch) writes a fresh generation and atomically switches metadata only after the complete new log is durable. Old generations are removed only after commit through exact validated names; their failure is harmless bounded debris, never an alternate visible session.
- `list` prefers one valid bundle metadata record per storage key and reads no message bodies. Legacy `<key>.json` remains readable as V1–V4. A legacy save builds the complete generation log and then atomically replaces that same stable JSON path with bundle metadata; failure before the metadata rename preserves the original bytes. New sessions and successfully migrated sessions use bundles; no read alone creates or rewrites files.
- Reject over 1 MiB serialized individual message records, over 64 MiB total active history, offset/count disagreement, duplicate/missing/out-of-order indices, trailing committed bytes, symlinks, traversal, and unsupported bundle versions without echoing content. These are new-write safety bounds; existing legacy JSON remains readable, but an over-bound legacy migration fails without modifying it.

### Exact lifecycle operations

- `load` remains a compatibility API that reconstructs the complete canonical `SessionV4` for branching/export/tests. Add `load_recent(key, page_size)` and `load_page(cursor, before_index, page_size)`; production page size is 64 messages and every read seeks by committed offsets, decodes only the requested bounded records, and returns global `[start,end)` plus total count.
- Pinning changes metadata only for bundles and keeps message-log bytes/hash unchanged. Archive/restore transfers metadata plus only its exact referenced active log with UUID revalidation and rollback on partial failure; no descendant or obsolete generation moves. Delete/retention unlink the exact active pair and report truthful partial failures without broad recursive deletion. Legacy behavior remains byte-for-byte until migration.
- Portable export and branching request a complete load; import creates a fresh bundle on its first successful save. Session summaries, profile isolation, current-session protection, pinned-first ordering, archive semantics, and retention eligibility are unchanged.

### TUI paging and semantic safety

- Resuming a bundle loads only its most recent 64 messages and stores a process-only `SessionHistoryCursor` in `AppState` with storage key/UUID, global start/total, generation, busy/error, and pending action. Legacy sessions initially use the existing full-load path until migrated.
- Make transcript scrolling bottom-relative: PageUp increases distance from the live bottom and PageDown decreases it. Prepending older messages therefore increases total lines while leaving the same existing content at the same viewport rows; `auto_scroll` behavior at the bottom remains unchanged.
- PageUp requests one older page on the blocking session worker when the loaded prefix is incomplete. Only one page load is active, stale generations are ignored, and failures remain inline without changing the loaded window or scroll anchor.
- No provider request, checkpoint, retry/rewind, timeline branch, export, save that rewrites loaded history, or other canonical-history consumer may observe a suffix as the whole conversation. Such an action first requests complete hydration on the blocking worker and resumes the exact pending typed action only after all pages are validated. Ordinary append-only autosaves may persist new suffix messages against the cursor without decoding older records.
- `current_session`, global message indices, checkpoint boundaries, fallback provenance, and timeline entries remain semantically global. Page results prepend in order and adjust the cursor start; once start reaches zero the state is indistinguishable from an eagerly loaded Session V4. Cancellation/session switch invalidates in-flight page results.
- Wide/narrow snapshots cover a resumed recent page, older-history loading, a preserved scrolled anchor after prepend, end-of-history, and inline page failure. The pager labels loaded/total message counts without exposing paths or raw record metadata.

## Milestones

### M1 — Characterize and implement bundle storage

- Record focused session/resume/portability/retention baselines and add failing temporary-root tests for metadata-only listing, new bundle creation, exact append, failed-append commit isolation, generation rotation, V1–V4 no-write reads, successful/failed legacy migration, bounds, symlinks, traversal, profile separation, and safe Debug output.
- Implement bundle metadata/log codecs, atomic publication, full load, recent/page reads, and exact summary lookup behind `SessionStore` without TUI changes.

### M2 — Preserve lifecycle operations

- Add exact pair tests for pin, archive/restore rollback/collision, delete, retention partial failure, import/export, full/historical branches, queues/goals/workflows/worktrees/checkpoints, and mixed legacy/bundle sorting.
- Route every `SessionStore` mutation through representation-aware helpers while preserving legacy behavior and public compatibility.

### M3 — Add bounded resume paging

- Add `SessionHistoryCursor`, typed page/hydration requests/results, blocking-worker orchestration, stale-generation cancellation, bottom-relative scrolling, append-safe saves, and full-history gates for semantic consumers.
- Add input/orchestration/render tests and review all wide/narrow snapshots, including page failure and no-scroll-jump assertions.

### M4 — Gates and released disposable walkthrough

- Run focused suites, `cargo test --no-fail-fast`, formatting, snapshot hygiene, strict Clippy, release build, and help/version/profile smokes.
- Under a disposable home, create a large legacy V4 session and a new bundle session, prove bounded recent resume and older-page prepend without a visual jump, submit one loopback turn only after exact canonical hydration, restart and prove metadata-only listing plus append reuse, then archive/restore and inspect exact files. Stop fixtures and move the exact root recoverably to Trash.

## Acceptance Criteria

- New saves and successful legacy migrations produce one valid metadata commit point plus one exact append-only active log; listing reads metadata only and a read never mutates storage.
- Prefix saves append; rewrites rotate generations; every injected failure leaves either the prior committed session or the complete new session, never a mixed visible state. Legacy migration failure preserves the original bytes.
- Recent/page loads decode only requested bounded records by committed offsets. TUI resume initially owns the recent page, PageUp prepends valid older pages, and the viewport anchor does not jump.
- Every canonical-history consumer sees the exact full ordered history or waits/fails safely; provider requests and saved rewrites can never silently treat a suffix as the whole session.
- Pin/archive/restore/delete/retention/export/import/branch/profile/bare/no-session behavior remains compatible, exact, and traversal/symlink safe. V1–V4 files remain backward-readable and are not rewritten until a successful save.

## Validation

Focused: PASS. `cargo test session --no-fail-fast` passed 107 matching tests, and `cargo test prepending_lazy_history --no-fail-fast` passed the visible-anchor regression.

Completion: PASS. `cargo test --no-fail-fast` passed 923 unit plus 28 CLI integration tests; `cargo fmt --check`, strict Clippy, release build, and help/version/profile smokes passed.

Manual/visual: PASS. Eight reviewed 100x30/60x24 recent/loading/complete/failure snapshots and a released disposable TUI walkthrough proved legacy migration, 64/130 recent resume, two older-page loads, exact full-history submission, same-generation append, and exact-pair archive/restore.

Blocked checks: none currently.

## Progress

- [x] Current monolithic store, compatibility schema, lifecycle mutations, resume orchestration, and transcript scroll seams traced.
- [x] Characterization/failing bundle tests recorded.
- [x] Bundle storage and paged read APIs implemented.
- [x] Exact lifecycle mutations adapted.
- [x] TUI paging and full-history semantic gates implemented.
- [x] Repository-wide gates and released disposable walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: version the storage bundle separately and keep the semantic session at V4.
  Evidence: the approved program explicitly keeps Session V4 backward-readable and schedules lazy history as a separate storage backend; dozens of persisted work/goal/workflow/branch invariants already depend on the V4 type.
  Consequence: storage changes do not force a new portable/session semantic schema or rewrite existing JSON on read.
- Decision: flat sibling metadata/log files with metadata as commit point, not a recursively managed directory.
  Evidence: current archive/delete/retention safety is exact-file based; flat derived files preserve one-component validation and permit rollback without broad `remove_dir_all`.
  Consequence: lifecycle operations transfer/unlink a validated pair and can report partial failures precisely.
- Decision: rotate log generations for rewrites or uncommitted tails rather than truncating.
  Evidence: retry/rewind/branch can replace a suffix, while an append may reach disk before metadata publication.
  Consequence: the active log is append-only, crash recovery is deterministic, and stale generations are inert.
- Decision: scrolling is bottom-relative and incomplete-history actions hydrate through typed blocking work.
  Evidence: current PageUp state already expresses distance intent but rendering treats it as a top offset; bottom-relative offsets preserve the viewport automatically when lines are prepended.
  Consequence: lazy paging does not require mutable rendering or heuristic line-height reconstruction, and canonical consumers never see a suffix as complete history.

## Surprises and Discoveries

- `AgentConfig::default()` remains separately inconsistent for raw partial TOML without `[agent]`; it is unrelated to this storage plan and must not be folded in.
- Metadata-only actions currently rewrite the entire history, so pin/archive/retention tests are part of the storage acceptance boundary rather than incidental follow-ups.

## Recovery and Rollback

Remove bundle-aware helpers and paging state while leaving legacy V1–V4 JSON reads intact. Disposable bundle files can be moved to Trash during tests; production rollback must continue reading already-created bundles until a separate explicit migration tool exists, so the on-disk reader should be isolated enough to retain even if lazy TUI paging is reverted. Never use Git reset or delete broad session roots.

## Completion Record

Changed files: `src/session.rs`, `src/app.rs`, `src/main.rs`, `src/tui/input.rs`, `src/tui/render.rs`, `src/tui/render_tests.rs`, eight session-history snapshots, this plan, and the five required harness documents.

Commands and results: focused session tests PASS (107), visible-anchor regression PASS, full tests PASS (923 unit + 28 CLI), formatting PASS, strict Clippy PASS, release build PASS, and release help/version/profile smokes PASS.

Remaining risks: legacy monoliths still require a full read until their first successful save; an exact companion-log unlink failure can leave one inert orphan while returning a truthful cleanup warning. Old generation logs are never selected because metadata names one exact generation.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` now record the storage boundary, evidence, and extension status. The released walkthrough fixture is recoverable at `/Users/aji/.Trash/ca-ses076-walk.0kIwK0`.
