# SES-048 — Session V4, No-Session, and Queue Persistence

Status: COMPLETE
Owner/session: current Codex session
Last updated: 2026-08-10

## Goal

Persist queued drafts safely in a backward-readable Session V4 while adding an explicit `--no-session` run mode that keeps all non-session capabilities and in-memory branching available without reading or writing saved sessions.

## Non-Goals

- Do not add archive, pin/unpin UI, retention, import/export, lazy history storage, or runtime profile switching; later session plans own those surfaces.
- Do not auto-submit restored queued drafts, migrate existing files eagerly, rewrite V1–V3 files on read, persist composer/history/kill-ring state, or change queue success-only drain semantics.
- Do not disable MCP, skills, hooks, config writes, retry, rewind, or in-memory forks under `--no-session`; only bare mode retains the broader extension restrictions.
- Do not modify oracle/reference trees, real user config, Keychain data, saved sessions, or the installed binary.

## Evidence and Current Behavior

- `src/session.rs::SessionV3` writes version 3, reads V1–V3, and has no `pinned` or queued-input fields. V1 conversion and V2/V3 deserialization already provide a lazy compatibility seam.
- `src/queue.rs::InputQueue` owns stable UUIDs, text, timestamps, order, editing, deletion, and success-only draining, but has no serialization boundary.
- `src/main.rs::save_current_session` persists canonical/session work state but not `AppState::input_queue`; `apply_loaded_session` cannot restore queued drafts.
- `RunCapabilities` currently derives only from `ActiveProfile`; bare disables all extensions/persistence, while no independent session-only capability selector exists.
- CLI parsing already rejects bare with continue/resume and preserves positional prompt/attachment ordering, providing the characterization boundary for `--no-session`.
- No clean Cursor fixture establishes Session V4, queued-draft persistence, or `--no-session`; these are intentional oracle-informed `ca` extensions.

## Scope

Allowed files:

- `src/{session,queue,runtime,app,main}.rs`
- `src/tui/{commands,input,input_tests,render,render_tests}.rs` only where truthful no-session/queue status or tests require it
- `tests/cli.rs`, source-owned snapshots/fixtures
- `docs/exec-plans/{active,completed}/`, `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, and runtime-path documentation if behavior changes

Forbidden files/areas:

- `oracles/`, `reference/`, real runtime roots, Keychain, installed binaries
- provider/tool/MCP/hook authority, Session archive/pin UI, and automatic restored-queue submission

Runtime state affected:

- One immutable session-persistence capability, Session V4 metadata, and the existing process input queue. No new background service or external authority.

## Design and Invariants

1. `SessionV4` adds defaulted `pinned: bool` and an ordered queued-input field containing only stable ID, text, and creation time. V1–V3 load in memory with `false`/empty defaults; reads never write; the next successful save writes version 4 in the same exact file.
2. Queue serialization is explicit and bounded to queued drafts. Selection, edit state, composer text, input history, and kill ring are process-only. Restoring a session places drafts visibly in `/queue`/preview state but never initiates a provider request.
3. Every queue content/order mutation updates the current in-memory session and, when persistence is enabled, the same profile-scoped session file. Removing an item for steering or success-only drain is persisted before it could be restored as a duplicate after a crash.
4. `--no-session` is a CLI run capability, not a profile. It conflicts with `--continue`, `--resume-list`, and `--session`; it leaves provider config, MCP, skills, hooks, sandbox, Keychain, shell history, retry, rewind, and in-memory forks unchanged.
5. With session persistence disabled, no session directory is read or created. `/save` and `/resume` remain discoverable but return guidance specific to `--no-session`; current-session identity and branches exist only in memory. Bare keeps its existing broader guidance and restrictions.
6. Queue restoration and mutation never auto-start provider traffic. Existing success-only drain still applies only to drafts queued during the current process after an explicitly active turn boundary; restored drafts remain user-visible until the user explicitly edits/submits/removes them or a separately specified future behavior changes that policy.

## Milestones

### M1 — V4 compatibility and queue boundary

- Add failing V1–V4 lazy-read/save tests, defaulted pin/queue characterization, and queue serialization/restore tests.
- Add Session V4 fields, explicit queue snapshots, and load/save integration without changing provider or UI behavior.

Expected result: old sessions load without writes, the next save upgrades in place, and current queue order/identity round-trips without process-only editor state.

Validation: focused `session` and `queue` tests.

### M2 — No-session capability and orchestration

- Add failing CLI conflict/positional tests and runtime-capability tests.
- Add `--no-session`, session-only capability resolution, no-read/no-write orchestration, truthful command guidance, queue mutation persistence, and restored-draft non-submission coverage.
- Add or update wide/narrow queue/resume/status snapshots only where visible behavior changes.

Expected result: ordinary profiles can opt out of persistence without losing extensions or in-memory history operations; queued drafts persist safely only when enabled.

Validation: focused CLI/runtime/orchestration/input/render suites and snapshot review.

### M3 — Completion gates and reconciliation

- Run full tests, formatting, strict Clippy, release build, and help/version/profile/no-session smoke tests.
- Exercise the rebuilt TUI with a temporary home and delayed loopback provider: persist multiple queued drafts, cancel/exit, resume and observe them without a request, mutate the queue, then run `--no-session` with MCP/skills/hooks config present and prove no session file is read or created while retry/rewind/fork remain in memory.
- Reconcile architecture, continuity, testing, next steps, parity, and runtime-path docs; move the plan before starting SES-049.

Expected result: SES-048 is independently releasable and SES-049 remains inactive until all gates pass.

## Acceptance Criteria

- V1, V2, and V3 files load as V4-compatible state without mutation; V4 round-trips `pinned` and ordered queued drafts; the next save upgrades the same file.
- Queue IDs/text/timestamps/order persist, while selection/edit/composer/history state does not. Restoring queued items produces no provider request and no automatic submission.
- Queue add/edit/delete/reorder/steer/drain transitions cannot leave a successfully persisted session with a stale duplicate queued item.
- `--no-session` preserves positional prompts and attachments, conflicts with every startup resume path, leaves non-session capabilities enabled, and causes no session-root read/create/write.
- `/save` and `/resume` explain the active no-session or bare reason; retry, rewind, and fork remain functional in memory.
- Focused, repository-wide, release/smoke, snapshot, and temporary-home walkthrough gates pass without real runtime side effects.

## Validation

Focused: PASS. Session 11/11, queue 6/6, runtime capabilities 5/5, session-picker 3/3, queue-filtered 20/20, parser/no-session, persistence, restored non-submission, failed-drain rollback, and command-guidance tests all passed.

Completion: PASS. `cargo test --no-fail-fast` passed 524 unit tests and 25 CLI integration tests; `cargo fmt --check`, strict Clippy, release build, help/version/conflict smoke, and zero-pending-snapshot checks passed.

Manual/visual: PASS. Two no-session title snapshots were reviewed at 100x30 and 60x24. A rebuilt-binary disposable-home walkthrough resumed a V4 queued draft without provider traffic, persisted its deletion, and proved no-session save/resume guidance plus in-memory 401 retry, fork, and rewind without changing the existing session/config hashes or file count.

Blocked checks: none.

## Progress

- [x] Traced current V1–V3 schema, store, queue, profile capability, CLI, autosave, restore, retry, rewind, and fork seams.
- [x] Add Session V4 and explicit queue snapshots with lazy compatibility tests.
- [x] Add `--no-session` capability/CLI/orchestration and queue mutation persistence.
- [x] Review visible states and run completion/isolated walkthrough gates.
- [x] Reconcile harness and archive the plan.

## Decisions

- Decision: model `--no-session` as a run capability layered on the immutable active profile.
  Evidence: bare currently conflates extension and persistence disablement, while the approved behavior disables only session storage.
  Consequence: ordinary profile config, credentials, MCP, skills, hooks, approvals, and sandbox behavior remain unchanged.
- Decision: persist only queue data that already has stable semantic identity.
  Evidence: `PendingInput` already owns UUID/text/creation time; selection and edit/composer state are transient UI concerns.
  Consequence: queue round-trips are deterministic without expanding session privacy scope.
- Decision: new branches start with an empty queue.
  Evidence: queued drafts belong to the source turn schedule and silently copying them into retry/fork branches risks duplicate provider submission.
  Consequence: source saves retain their queue, while retry/fork branches are clean process/session timelines.

## Surprises and Discoveries

- The completion path currently saves before popping the next queued draft. V4 persistence must record the pop before the continuation request starts or a crash can restore and later duplicate that draft.
- Restored drafts needed a process-only eligibility set: restoring queue bytes alone would otherwise cause the oldest saved draft to drain after any later successful turn. Explicit edit/submit/remove releases or removes that guard; merely resuming or reordering does not.
- The live walkthrough exercised the ordinary pre-output provider retry policy on its delayed first request. Queue persistence remained correct, and the dedicated no-session 401 path still proved explicit branch retry without session writes.

## Recovery and Rollback

- Remove only the defaulted V4 fields, queue snapshot conversion, run-capability/CLI flag, orchestration persistence hooks, and focused tests/snapshots. V1–V3 files remain untouched until a successful V4 save, so rollback requires no migration or destructive Git operation.

## Completion Record

Changed files: `src/{app,main,queue,runtime,session}.rs`, `src/tui/{commands,input,render,render_tests}.rs`, two no-session title snapshots, this plan, and the five repository harness documents.

Commands and results: focused suites PASS; two reviewed snapshots PASS; final full tests PASS (524 + 25); format PASS; strict Clippy PASS; release PASS; help/version/conflict smoke PASS; isolated loopback resume/mutation/no-session walkthrough PASS.

Remaining risks: queue persistence intentionally does not include composer/history/editor selection state. Restored drafts are protected from automatic drain only for the current process; after an explicit edit they rejoin ordinary FIFO behavior by design.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` now record V4 compatibility, queue persistence/crash ordering, no-session capability isolation, actual gates, intentional-extension status, and the SES-049 handoff.
