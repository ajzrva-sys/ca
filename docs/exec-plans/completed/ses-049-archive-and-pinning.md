# SES-049 — Archive and Pinning

Status: COMPLETE
Owner/session: `/root`
Last updated: 2026-08-10

## Goal

Make saved-session organization safe and predictable in `/resume`: users can pin or unpin saved sessions, browse active and archived sessions, and archive or restore exactly one inactive session without changing descendants or the current session.

## Non-Goals

- Deleting archived sessions, bulk archive operations, automatic retention, import/export, or session-format redesign.
- Rewriting branch parent metadata or cascading an archive operation to descendants.
- Runtime profile switching or persistent-session mutations in `bare` or `--no-session` runs.
- Cursor parity. This is an Ante-informed `ca` extension.

## Evidence and Current Behavior

- `src/session.rs` already stores a defaulted `SessionV4::pinned` field, but `SessionStore::list` still orders only by update time and no mutation API exposes pinning.
- `SessionStore::delete` validates a one-component storage key, re-reads the file, checks the expected UUID, and unlinks exactly one JSON file. Archive moves need the same identity checks without rewriting session contents.
- `/resume` state and rendering live in `src/app.rs`, `src/tui/input.rs`, and `src/tui/render.rs`. It currently supports filtering, resuming, and Cancel-first permanent deletion. The current session is visibly marked and protected from deletion.
- `src/runtime.rs::RuntimePaths::for_profile` already isolates named-profile session roots; no archive root exists yet.
- Session deletion runs on a blocking worker in `src/main.rs`, locks confirmation input while busy, preserves the filter and clamped row, and reports failures inline. Archive/pin mutations should reuse that orchestration grammar.
- The approved roadmap requires pin/unpin and archive/unarchive in `/resume`, pinned-first ordering, exact UUID-validated moves into a profile-scoped archive, no descendant cascade, and current-session protection.

## Scope

Allowed files:

- `src/session.rs`, `src/runtime.rs`, `src/app.rs`, `src/main.rs`
- `src/tui/{commands,input,render,snapshots}.rs` and focused test modules/fixtures
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`
- This ExecPlan

Forbidden files/areas:

- `oracles/`, `reference/`, real user configuration, Keychain entries, real saved sessions, and the installed binary
- Session import/export, retention, or lazy-history work reserved for later plans

Runtime state affected:

- Only the selected profile's saved-session and archive directories. Tests and walkthroughs use temporary homes and generated fixture sessions only.

## Design and Invariants

- Add a profile-scoped archive path: default profile uses `<data>/archive`; named profiles use `<data>/profiles/<name>/archive`. Bare resolves the default path for display consistency but exposes no persistent session manager.
- `SessionStore` gains UUID-checked `set_pinned` and exact move operations. Every storage key is one filename component; source UUID is re-read immediately before mutation; a destination collision fails without overwrite; move uses a same-filesystem no-overwrite hard-link/unlink transfer and does not deserialize/reserialize session bytes.
- Archiving and restoring move exactly one JSON file. Descendant files and historical `parent` metadata are untouched.
- Pinning updates exactly one active saved session atomically and may lazily upgrade older session JSON through the existing V4 save contract. Archived pin state is preserved but not editable until restored.
- `/resume` has explicit Active and Archive views. `Tab`/`BackTab` switches views, preserves the current filter, refreshes the relevant list, and clamps selection. Enter resumes only active rows; archived rows must be restored first.
- `Shift+P` toggles pin on the highlighted active row. `Shift+A` opens Cancel-first archive confirmation for an inactive active row. `Shift+U` restores the highlighted archived row. Mutation work runs on one blocking session-management worker and locks all picker mutations while busy.
- The current session may be pinned/unpinned because pinning is non-destructive, but it cannot be archived or deleted. Current-session protection is checked both in input and immediately before the worker starts.
- Active sessions sort pinned first, then `updated_at` descending, then UUID. Archive rows use the same stable ordering using their preserved pin field.
- A successful mutation refreshes both lists, keeps the filter, retains the affected row when it still exists, or selects the row that occupies its former visible position. Empty-view behavior remains explicit and does not discard the other view.
- Failures remain inline and preserve view/filter/selection/confirmation. No action overwrites an existing destination file.
- Bare and `--no-session` keep `/resume` unavailable under their existing guidance, so no archive directory is read or created.

## Milestones

### M1 — Characterize storage and picker behavior

- Add failing tests for pinned-first ordering, UUID mismatch/traversal/collision, byte-preserving exact archive/restore, and no descendant cascade.
- Add input/render tests for Active/Archive views, dynamic controls, current protection, Cancel-first confirmation, busy locking, and empty/narrow states.
- Validation: focused `session`, session-picker input, and render tests fail only on the missing behavior.

### M2 — Implement storage and runtime paths

- Add the profile-scoped archive path and focused default/named/bare path tests.
- Add safe exact-file move and pin APIs with collision and UUID protection.
- Implement pinned-first stable listing without breaking legacy loading or stable storage keys.
- Validation: focused storage/runtime tests pass.

### M3 — Implement `/resume` organization UI

- Add typed view, confirmation, busy, and action state.
- Add view switching, pin/unpin, archive, restore, current protection, deterministic refresh, and inline errors.
- Run filesystem work on the blocking worker and discard stale results that no longer match the selected action.
- Add reviewed wide/narrow snapshots for active pinned/current rows, archive rows, confirmation, busy, empty, and failure states.
- Validation: focused input/orchestration/render tests and snapshot review pass.

### M4 — Complete gates and isolated walkthrough

- Run `cargo test --no-fail-fast`, formatting, strict Clippy, release build, and help/version/profile smoke tests.
- Exercise a default and named profile under a temporary home: pin, archive, inspect exact files/descendants, restore, verify current protection and collision failure, and confirm no real runtime path changed.
- Reconcile harness documentation, move this plan to completed, and mark SES-050 next without starting it.

## Acceptance Criteria

- Pin/unpin persists one UUID-validated active session and immediately changes pinned-first list ordering.
- Active and archive views are searchable and navigable; Enter never resumes an archived session.
- Archive/restore moves one exact file between profile-scoped roots without rewriting its bytes, overwriting collisions, or touching descendants/parent metadata.
- Current-session archive is rejected before filesystem mutation; pinning it remains allowed.
- Archive confirmation defaults to Cancel; busy state ignores duplicate actions; failure keeps confirmation and selection with a sanitized inline error.
- Named profiles cannot see or mutate another profile's active/archive sessions.
- Bare and `--no-session` do not read or create either session root.
- Wide/narrow deterministic snapshots cover populated, empty, confirmation, busy, and error states.
- All focused and repository gates pass, and the temporary-home walkthrough proves isolation.

## Validation

Focused:

- PASS: `cargo test session::tests --no-fail-fast` (14), `cargo test tui::input::session_picker_tests --no-fail-fast` (8), `cargo test tui::render_tests::session_ --no-fail-fast` (5), `cargo test tests::session_ --no-fail-fast` (10), `cargo test runtime::tests --no-fail-fast` (5), plus two active-session orchestration regressions.

Completion:

- PASS: `cargo test --no-fail-fast` (539 unit plus 25 CLI), `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo build --release`.
- PASS: rebuilt help/version/bare-resume/profile-doctor smoke. The named-profile doctor emitted valid JSON, returned its expected non-ready status for an intentionally absent runtime home, and did not create the missing profile.

Manual/visual:

- PASS: reviewed populated, empty, busy, confirmation, and failure snapshots at wide and narrow widths; zero pending snapshots.
- PASS: isolated rebuilt-binary walkthrough under temporary roots. Pinning persisted, one default-profile session archived/restored with an identical SHA-256, its child remained active with unchanged parent metadata, current-session archive was rejected, and a named profile archived only its own fixture.

Blocked checks:

- None currently.

## Progress

- [x] Verified the existing V4 pin field, session-store safety boundary, `/resume` deletion flow, and profile path isolation.
- [x] Created the active ExecPlan before implementation.
- [x] Added failing storage and state/input characterization tests before implementation, then added orchestration and render regression coverage.
- [x] Implemented profile-scoped archive paths and exact pin/archive/restore storage operations.
- [x] Implemented the `/resume` Active/Archive manager and blocking mutation orchestration.
- [x] Reviewed populated, empty, busy, confirmation, and failure wide/narrow snapshots.
- [x] Passed focused and repository-wide validation gates.
- [x] Completed the isolated temporary-home walkthrough and reconciled documentation.

## Decisions

- Decision: use `Tab`/`BackTab` for Active/Archive views, `Shift+P` for pinning, `Shift+A` for archive, and `Shift+U` for restore.
  Evidence: `/resume` currently has no tabbed content or these modified bindings, and the operations remain discoverable in a dynamic footer.
  Consequence: Enter keeps its unambiguous resume meaning only in the Active view.
- Decision: allow pinning the current session while prohibiting its archive.
  Evidence: pinning is an organizational metadata update; moving the active file would break autosave identity and persistence assumptions.
  Consequence: “protect current session” applies to removal from the active store, consistent with existing deletion protection.
- Decision: archive via exact no-overwrite file transfer rather than V4 reserialization.
  Evidence: the requirement is an exact one-file move and historical sessions must remain backward compatible and unmigrated until a successful content mutation.
  Consequence: archive/restore preserves bytes, unknown compatible fields, timestamps, and parent metadata.

## Surprises and Discoveries

- A plain filesystem rename can overwrite an existing destination on supported platforms. The implementation therefore uses a same-filesystem hard link followed by source unlink: creation fails atomically on collision, source-removal failure rolls the new link back, and the session bytes/inode contents remain exact.
- Applying pinned-first ordering to `SessionStore::list` would make `-c` resume an older pinned session. Startup continuation now explicitly selects maximum `updated_at`, while `/resume` retains pinned-first organization.

## Recovery and Rollback

- Code rollback is limited to SES-049-owned files and uses patch-based edits; no destructive Git operation is needed.
- Tests create disposable data roots. A failed move leaves the source file in place, and destination collisions never overwrite.
- Walkthrough fixtures are removed only after their exact temporary root is recorded and verified; real user paths are never targets.

## Completion Record

Changed files: session storage/runtime/state/orchestration in `src/{session,runtime,app,main}.rs`; `/resume` actions/input/rendering and deterministic snapshots under `src/tui/`; and the five required harness documents plus this plan.

Commands and results: all focused suites, 539 unit tests, 25 CLI tests, formatting, strict Clippy, release build, help/version/profile smoke, snapshot review, and isolated default/named-profile TUI walkthrough passed. The initial doctor smoke used an unsupported `--json` spelling, exited during argument parsing, and was rerun successfully with `--output-format json`; it changed no runtime state.

Remaining risks: archive/restore intentionally requires source and destination on the same filesystem. Destination-collision behavior is covered hermetically rather than by the interactive walkthrough. Ctrl+C handling while a modal owns input remains a broader pre-existing key-routing concern, outside this storage-organizational slice.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` now record the exact storage boundary, validation evidence, extension status, and SES-050 handoff.
