# KEY-047 — Keymaps and History Search

Status: COMPLETE
Owner/session: current Codex session
Last updated: 2026-08-10

## Goal

Add a safe, profile-scoped keymap for global composer shortcuts plus a process-only searchable input-history overlay, while permanently preserving Ctrl+R for Diff and keeping cancellation, approvals, confirmations, and modal navigation outside the configurable surface.

## Non-Goals

- Do not make ordinary text editing, Enter submission, Escape, Ctrl+C, approval decisions, destructive confirmations, workspace trust, or modal navigation configurable.
- Do not rebind Ctrl+R, remove the existing Up/Down history traversal, persist input history, add shell-history search, or change session formats.
- Do not allow runtime profile switching, edit bare-profile configuration, or claim Cursor parity.
- Do not modify oracle/reference trees, real user configuration, Keychain data, saved sessions, or the installed binary.

## Evidence and Current Behavior

- `src/tui/input.rs::handle_event` hard-codes global composer shortcuts before normal text editing; modal handlers already have higher precedence and therefore provide a clean non-configurable safety boundary.
- Ctrl+R queues `CommandId::Diff`; focused coverage already protects that mapping.
- `AppState.input_history` is a process-only, bounded list of submitted prompts and `history_idx` supports direct Up/Down traversal. There is no searchable history view or `/history` command.
- Startup and status help strings in `src/tui/render.rs` embed shortcut labels directly.
- `ConfigStore` already supports profile-aware, comment-preserving atomic mutations and disables writes in bare mode.
- No clean Cursor fixture establishes a configurable keymap or `/history`; this is an intentional oracle-informed `ca` extension.

## Scope

Allowed files:

- `src/keymap.rs`, `src/{app,config,main}.rs`
- `src/tui/{commands,input,input_tests,render,render_tests}.rs`
- source-owned focused tests and snapshots
- `docs/exec-plans/{active,completed}/`, `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`

Forbidden files/areas:

- `oracles/`, `reference/`, real config/Keychain/session roots, installed binaries
- provider requests, session schema, approval/confirmation/workspace-trust key handling, and Ctrl+R behavior

Runtime state affected:

- One validated effective keymap, one pending two-stroke prefix, one keymap manager/capture overlay, and one process-only history-search overlay.
- Optional profile-scoped `[keymap]` configuration; no persistent history.

## Design and Invariants

1. `KeyAction`, `KeyStroke`, `KeyChord`, and `Keymap` are typed. Configuration accepts canonical, case-insensitive chord strings, then normalizes display. Unknown actions, malformed strokes, duplicate chords, prefix ambiguity, and reserved safety chords fail validation before the keymap becomes active.
2. The configurable surface is deliberately small: model picker, external editor, model cycle, reasoning cycle, queue input, save/steer, expand/collapse, and history search. Diff remains a visible fixed `Ctrl+R` row. Ctrl+C, Enter, Escape, approvals, confirmations, and modal keys never enter the resolver.
3. Defaults preserve every existing configurable shortcut and add history search on `Ctrl+X Ctrl+R`. A first stroke is intercepted only when it is an exact configured prefix; the second stroke either dispatches one action or clears the prefix with safe feedback. Ctrl+C remains globally authoritative while a prefix is pending.
4. `/keymap [filter]` opens a searchable manager. Enter captures the highlighted configurable action; capture records one stroke and waits for either Enter to save it as one-stroke or a second stroke to save a two-stroke chord. Escape cancels. Shift+R restores the highlighted default. Writes use the blocking mutation worker, lock the editor, preserve unrelated TOML/comments, reload the effective map on success, and retain the draft/error on failure. Bare is view-only.
5. `/history [filter]` opens newest-first process input history with exact original entries, bounded filtering/navigation, paste isolation, and no session/shell-history access. Enter copies the selected entry into the composer for editing; Escape closes without changing it. Duplicate entries remain visible in submission order.
6. Help/title/status surfaces obtain labels from the effective keymap. Fixed safety labels are explicit and cannot be shadowed by configuration.

## Milestones

### M1 — Typed map and configuration boundary

- Add failing parser/default/conflict/reserved/prefix tests and characterize all currently configurable shortcuts.
- Add typed keymap configuration and comment-preserving binding/reset mutations with named/default/bare coverage.
- Load one immutable effective map into `AppState` before TUI input begins.

Expected result: invalid keymaps fail safely and valid profile-scoped bindings are deterministic without changing any input behavior yet.

Validation: focused keymap/config tests.

### M2 — Dispatch, managers, and history search

- Replace only the allowed hard-coded global shortcut branches with the typed resolver; leave Ctrl+R and safety/modal handlers intact.
- Add `/keymap` inspection/filter/capture/reset with blocking-save result handling and stale/busy safety.
- Add `/history` filtering/navigation/paste/selection and preserve the existing Up/Down traversal.
- Render dynamic labels and reviewed wide/narrow manager/history/capture/bare/error states.

Expected result: users can inspect or safely change global shortcuts and search process input history without hidden composer or safety-key effects.

Validation: focused command/input/orchestration/render suites and snapshot review.

### M3 — Completion gates and reconciliation

- Run full tests, formatting, strict Clippy, release build, and help/version/profile smoke tests.
- Exercise the rebuilt TUI with a temporary home: submit several distinct prompts to a loopback provider, search and restore one through `/history`, capture a one-stroke and a two-stroke keymap entry, verify dynamic labels and restart persistence, reset the entry, and verify Ctrl+R still opens Diff. Exercise bare view-only behavior separately.
- Reconcile architecture, continuity, testing, next steps, and parity; record actual results and move this plan to completed before starting SES-048.

Expected result: KEY-047 is independently releasable and later session work remains inactive until all gates pass.

## Acceptance Criteria

- Existing default global shortcuts behave identically, Ctrl+R always opens Diff, and emergency/modal keys remain non-configurable.
- Valid one- and two-stroke bindings dispatch exactly one action; conflicts, prefix ambiguity, malformed inputs, unknown actions, and reserved strokes are rejected before persistence/runtime activation.
- `/keymap` is searchable, truthful about fixed versus configurable rows, supports capture/reset, preserves comments/unrelated config, reloads after successful saves, and is view-only in bare mode.
- `/history` is process-only, newest-first, filterable, paste-isolated, and restores the exact highlighted input as an editable composer draft without submitting it.
- Dynamic visible shortcut labels follow the effective map and remain bounded on narrow terminals.
- All focused, repository-wide, release/smoke, snapshot, and temporary-home walkthrough gates pass without real runtime side effects.

## Validation

Focused: PASS. Seventeen parser/config/command/input/orchestration/render tests cover defaults, one/two-stroke dispatch, malformed/reserved/conflicting bindings, atomic save/reset, bare refusal, filtering, paste isolation, process-only draft restoration, pending-prefix timeout/emergency controls, fixed Ctrl+R, and dynamic labels. Twelve new wide/narrow snapshots were reviewed with no pending snapshot files.

Completion: PASS. `cargo test --no-fail-fast` passed 515 unit and 25 CLI integration tests; `cargo fmt --check`, strict Clippy, `cargo build --release`, rebuilt help/version, configured-model listing, bare, and missing named-profile doctor smoke checks passed.

Manual/visual: PASS. A rebuilt-binary temporary-home walkthrough submitted `alpha marker` and `beta marker` to a credentialless loopback provider, filtered/restored `alpha marker` without submission, captured `Ctrl+B`, replaced it with `Ctrl+X Ctrl+B`, restarted and invoked the persisted chord into an empty process-history view, reset to `Ctrl+X Ctrl+R`, opened Diff with fixed Ctrl+R, and verified bare showed and enforced view-only mode.

Blocked checks: none.

## Progress

- [x] Reviewed current command, composer input, process-history, renderer, profile, and atomic config seams.
- [x] Add typed keymap and persistence characterization.
- [x] Add safe dispatch and two-stroke state.
- [x] Add `/keymap` and `/history` UI/orchestration/snapshots.
- [x] Run completion gates and isolated walkthroughs.
- [x] Reconcile harness and archive the plan.

## Decisions

- Decision: configurable bindings apply only in the base composer/global-shortcut layer.
  Evidence: every sensitive approval, confirmation, trust, and modal handler already has higher input precedence.
  Consequence: customization cannot alter safety decisions or overlay escape paths.
- Decision: Ctrl+R is a fixed, displayed row rather than a configurable action.
  Evidence: the approved program explicitly requires Ctrl+R to remain Diff.
  Consequence: reverse history receives a distinct configurable default chord and `/history` remains universally available.
- Decision: history stays process-only and duplicate-preserving.
  Evidence: current `input_history` is process-only and the roadmap does not authorize session/shell-history persistence changes.
  Consequence: KEY-047 does not alter privacy or session compatibility boundaries.

## Surprises and Discoveries

- The existing model picker has two defaults (`Ctrl+K` and `Ctrl+L`), so one action supports a bounded list of chords while a captured override deliberately replaces that action's default list.
- A pending two-stroke prefix must be resolved before the fixed standalone Ctrl+R branch so the default `Ctrl+X Ctrl+R` history binding can work. An unmatched prefix followed by Ctrl+R falls back to Diff, keeping the fixed command reachable.
- Adding another welcome-screen row displaced unrelated narrow overlays. Dynamic labels now replace existing model/reasoning/steer labels without changing the default layout; the full searchable binding list remains in `/keymap`.
- The live restart walkthrough exposed a stale first-stroke status after successful dispatch. The resolver now clears that hint before opening the target action, with a focused regression; final repository gates passed afterward.

## Recovery and Rollback

- Remove only the typed keymap/history state, command/UI integration, config patch, focused tests, and snapshots introduced here. Existing fixed input branches can be restored locally without Git-destructive operations. No runtime migration is required because absent `[keymap]` means defaults.

## Completion Record

Changed files: `src/keymap.rs`, `src/{app,config,main}.rs`, `src/tui/{commands,input,input_tests,render,render_tests}.rs`, 12 keymap/history snapshots, this plan, and the five repository harness documents.

Commands and results: focused keymap/history tests PASS; 12 reviewed snapshots PASS; final `cargo test --no-fail-fast` PASS (515 + 25); format PASS; strict Clippy PASS; release PASS; help/version/profile smoke PASS; isolated loopback default/restart/bare walkthrough PASS.

Remaining risks: key chords are terminal events, so some host terminal applications may reserve or translate particular control sequences before `ca` receives them. Capture shows exactly what `ca` received, rejects unsafe/ambiguous values, and leaves `/history` as an always-available command.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` now record the resolver boundary, process-only history, actual gates, intentional-extension status, and SES-048 handoff.
