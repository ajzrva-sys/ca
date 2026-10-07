# CFG-124 — One-Turn Models and Route Presets

Status: COMPLETE
Owner/session: Codex / current reliability continuation
Last updated: 2026-08-16

## Goal

Add a process-scoped model override that applies to exactly one complete
top-level turn and backward-compatible named provider/model/reasoning presets,
with reliable restoration and comment-preserving persistence.

## Non-Goals

- No endpoint, credential, prompt, profile-inheritance, session-format, or
  provider-wire change.
- No model-generated routes, automatic preset activation, branch/worktree,
  intermediate commit, installation, or oracle/reference access.

## Evidence and Current Behavior

- `src/main.rs::execute_command_action` applies `/model <id>` immediately and
  persistently for the process; arguments have no `--once` interpretation.
- `src/app.rs::AppState` owns `current_model`, `reasoning_effort`, the
  top-level `turn_lifecycle`, and the queued-input state, but has no typed
  restoration record or preset manager.
- The TUI loop consumes `turn_lifecycle.take_completion()` before handling the
  next event, providing the required restore-before-queued-input boundary.
- `src/config.rs::CaConfig` has no route-preset field. Existing UI/config
  mutations establish the comment-preserving and bare-profile persistence
  conventions.
- Inferred: the safest implementation stores the original route once in a
  process-only typed state and restores it at the existing terminal lifecycle
  boundary, before notification emission or input draining.

## Scope

Allowed files: app/config/command/input/render/orchestration source, focused
tests and snapshots, this plan, and reconciled harness documents.

Forbidden files/areas: `oracles/`, `reference/`, session/provider wire formats,
credentials/endpoints/prompts, branches, worktrees, stashes, preservation
directories, intermediate commits, and installed binaries.

Runtime state affected: process-only current route and unused one-turn
override; named preset persistence only after explicit `/preset-save` in a
non-bare profile.

## Design and Invariants

- `/model <id> --once` validates one exact configured model and captures the
  original provider/model/effort only when no baseline exists. Replacing an
  unused override changes only the selected override.
- Exactly one submitted top-level turn consumes the override. Provider
  continuations, tools, and subagents remain within that turn. Completed,
  Failed, and Cancelled all restore before queued input begins.
- `/clear`, successful `/resume`, ordinary `/model`, and `/preset` restore and
  cancel an unused override before applying their own behavior.
- Presets contain only provider, model, and optional configured reasoning
  effort. Names are 1–64 lowercase ASCII letters, digits, hyphens, or
  underscores.
- `/preset` is a picker; `/preset <name>` applies process-only. `/preset-save
  <name>` uses the existing config store, refuses bare persistence, and asks a
  Cancel-first overwrite confirmation.
- Backward-compatible config loading defaults to no presets; failed validation
  or save leaves active route and config bytes unchanged.

## Milestones

### M1 — Characterize lifecycle and configuration

- Add failing tests for argument parsing, original-baseline retention,
  consumption/restoration across all terminal outcomes, cancellation commands,
  preset validation/defaults, and comment-preserving persistence.
- Expected result: failures identify only the absent typed state, commands,
  config field, and modal.
- Validation: `cargo test one_turn --no-fail-fast` and `cargo test route_preset
  --no-fail-fast`.

### M2 — Implement one-turn route ownership

- Add typed process state, exact `/model ... --once` routing, turn-start
  consumption, and restore-before-next-input lifecycle handling.
- Validate continuations/tools/subagents stay under the override and all
  terminal outcomes restore exactly once.

### M3 — Implement presets and TUI surfaces

- Add backward-compatible config types/store mutation, `/preset` picker,
  process-only apply, `/preset-save`, and Cancel-first overwrite confirmation.
- Add reviewed wide/narrow empty/list/confirmation/error/bare snapshots.

### M4 — Validate and reconcile

- Run focused, complete local, disposable loopback TUI, and exact-source
  physical-FreeBSD gates.
- Reconcile harness docs, archive this plan, and activate OUT-125.

## Acceptance Criteria

- The override affects exactly one top-level turn and restores its original
  route/effort after Completed, Failed, or Cancelled before queued input.
- Replacing an unused override retains the first baseline; explicit route
  commands cancel it deterministically.
- Preset validation, application, persistence, overwrite confirmation, bare
  refusal, and failure atomicity satisfy the stated bounds.
- No session/provider schema, credential, endpoint, prompt, user runtime, or
  oracle/reference file changes outside explicitly confirmed disposable tests.

## Validation

Focused: PASS. Characterization initially failed with 21 expected missing
state, config, command, input, and render symbols. The completed focused set
covers one-turn baseline retention, replacement, consumption, all terminal
outcomes, unused cancellation, prepared-route identity, ordinary-model
cancellation, preset name/config validation, comment-preserving atomic saves,
bare refusal, failed-save route stability, command metadata, input behavior,
and eight reviewed wide/narrow picker/confirmation states.

Completion: PASS. Local default passes 1,437 main tests plus 37 CLI tests;
local all-feature passes 1,466 main, 118 experiment, and 37 CLI tests. The two
documented ignores remain. Formatting, strict all-feature Clippy, all-feature
release, help/version, diff check, and snapshot hygiene pass. A post-gate
Rust-1.96 lint-only simplification was rechecked with its focused test, local
and native formatting/Clippy, and both release builds.

Manual/visual: PASS. A disposable 80x24 no-session TUI and credentialless
loopback provider observed `once` for the selected next turn and `base` for
the following turn. It saved process-route presets without sessions, applied
a preset to cancel an unused override, and proved default Enter left an
existing preset byte-identical. The owned provider stopped and the fixture
moved recoverably to Trash.

Physical FreeBSD: PASS on 15.1-RELEASE-p2 amd64 as unprivileged `aji`.
Focused tests pass; default passes 1,424 main plus 37 CLI; all features pass
1,453 main, 116 experiment, and 37 CLI, with the two documented ignores. The
official-price experiment took 490 seconds. Native formatting, strict Clippy,
release, help/version, snapshot hygiene, x86-64 ELF identity, linkage, and
key-file source hashes pass. The disposable source was removed; the shared
build cache remains for later tasks.

## Progress

- [x] Plan activated after CTX-123 completion.
- [x] Failing characterization tests added.
- [x] One-turn route lifecycle implemented.
- [x] Preset persistence and TUI surfaces implemented.
- [x] Focused/local/manual/FreeBSD gates complete.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: restoration will attach to the existing top-level terminal
  completion boundary, not individual provider/tool requests.
  Evidence: the lifecycle already aggregates continuations, tools, and
  subagents and is consumed before subsequent event handling.
  Consequence: one-turn semantics do not fork provider orchestration.

## Surprises and Discoveries

- The new preset commands initially displaced `/provider` as the first
  command-palette Down selection. Keeping `/provider` immediately after
  `/model` preserved the established navigation contract.
- A newly named preset is requested by a slash-command action that runs inside
  the generic event arm. The asynchronous save must be started in that same
  event cycle rather than waiting for unrelated input.
- Rust 1.96 on the physical host applies one strict boolean-simplification
  lint that the local Rust 1.97 run did not emit; the equivalent predicate was
  simplified and revalidated on both systems.

## Recovery and Rollback

CFG-124 adds one backward-compatible optional config table and process-only UI
state. Source rollback removes its commands, config/store field, modal, tests,
snapshots, and docs without rewriting existing sessions or provider data.

## Completion Record

Changed files: `src/app.rs`, `src/config.rs`, `src/main.rs`,
`src/tui/commands.rs`, `src/tui/input.rs`, `src/tui/render.rs`, focused render
tests and eight snapshots, plus reconciled harness documents.

The completed behavior is process-scoped: exactly one top-level turn consumes
an override and the existing terminal lifecycle restores the original
provider/model/effort before queued input. Presets persist only the three
allowed route fields through the comment-preserving config store. No endpoint,
credential, prompt, provider/session wire shape, oracle/reference content,
branch, worktree, stash, installed binary, or intermediate commit changed.

OUT-125 is the only active next task.
