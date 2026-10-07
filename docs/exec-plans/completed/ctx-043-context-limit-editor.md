# CTX-043 — Context-Limit Editor

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-10

## Goal

Provide an idle-only `/context-limit [provider/model]` editor for exact model context-window and automatic-checkpoint thresholds, with empty values inheriting lower-precedence settings and successful changes applying before the next turn.

## Non-Goals

- Do not add provider-specific tokenizers or change estimate units.
- Do not mutate provider catalogs, model defaults, canonical messages, checkpoints, or session schemas.
- Do not allow active-turn edits or persistent writes in bare mode.
- Do not edit oracle/reference files or claim Cursor parity.

## Evidence and Current Behavior

- `CaConfig::context_for_model` currently gives a whole `model_profiles[provider/model]` context pair precedence over exact and legacy `model_context` entries. It does not support field-by-field inheritance.
- `context::automatic_threshold` uses an explicit threshold or 80% of a known window; `context_limits` is read by `/context` and before automatic checkpoint decisions.
- No config mutation or TUI state exists for context limits. `/context` is view-only.
- Slash-command availability already distinguishes idle-only actions, and config mutations can run on a blocking worker with atomic TOML replacement.
- Ante documents a settings surface and category-level context reporting, but no clean Cursor fixture establishes this editor. It is an oracle-informed `ca` extension.

## Scope

Allowed: `src/{app,config,context,main}.rs`, context-limit-specific `src/tui/{commands,input,render}.rs`, focused tests/snapshots, this plan, and the five harness documents.
Forbidden: oracle/reference edits, real config/session/Keychain state, provider traffic, unrelated refactors.
Runtime state affected: explicit user saves update only the selected profile's raw config. Validation uses temporary configs/homes.

## Design and Invariants

- Target identity is an exact, validated `provider/model`; the target is fixed while the editor is open.
- Resolve context fields independently in this order: exact model-profile field, exact `model_context` field, legacy model-only field. Empty exact values inherit the next source.
- If an exact model profile exists, edit only its two context keys and preserve all capability/reasoning fields. Otherwise edit the exact `model_context."provider/model"` table. Never rewrite legacy model-only entries.
- Drafts contain only decimal strings. Empty means inherit; nonempty values must be positive integers. A configured checkpoint threshold must resolve against a known effective window and be strictly smaller.
- The UI shows configured versus effective values and identifies inherited/default-80% behavior. Bare is view-only.
- `/context-limit` is idle-only. Persistence runs on one blocking worker; the current turn cannot observe a partial setting. Success reloads config, closes the editor, clears the cached context breakdown, and affects subsequent turns/checkpoint checks only.
- TOML mutation is atomic and preserves unrelated tables, comments, provider data, model capabilities, and defaults.

## Milestones

### M1 — Resolution and persistence characterization
- Add failing tests for field-wise precedence, empty inheritance, validation, exact-table selection, comment/capability preservation, no-op clears, named-profile first save, and bare rejection.
- Validation: focused config/context tests.

### M2 — Command and editor state machine
- Add command discovery, target parsing, `/config` delegation, field editing, cursor-safe input/paste, validation-in-place, bare state, and cancel/save actions.
- Validation: focused command/input/orchestration tests.

### M3 — Background save and rendering
- Add blocking persistence/reload, between-turn cache invalidation, inline failure recovery, and reviewed wide/narrow populated/inherited/bare/busy/error snapshots.
- Validation: focused render/orchestration tests and snapshot review.

### M4 — Completion gates and reconciliation
- Run full tests, formatting, strict Clippy, release, smoke, and an isolated temporary-home walkthrough; reconcile docs and archive before USG-044.

## Acceptance Criteria

- `/context-limit` targets the current model; an optional exact configured `provider/model` target is supported and invalid/ambiguous targets fail locally.
- Empty fields inherit independently. Effective threshold remains explicit when configured, otherwise 80% of the resolved window.
- Zero, malformed, unknown-window thresholds, and threshold-at/above-window drafts remain in the editor with actionable errors and no write.
- Saves preserve comments, unrelated TOML, model capabilities, providers/defaults, and lower-precedence entries.
- Changes are unavailable during active turns and in bare; successful idle saves affect `/context` and automatic checkpoint policy on subsequent work.

## Validation

Focused: PASS — `cargo test context --no-fail-fast` ran 35 tests; the context-limit slice contains 15 config/command/input/orchestration/render tests.
Completion: PASS — 461 unit tests and 25 CLI tests; formatting, strict Clippy, release, and help/version/profile smoke all passed.
Manual/visual: PASS — ten wide/narrow populated/inherited/bare/busy/failure snapshots were reviewed. In an isolated temporary home, the rebuilt TUI opened and canceled the editor, reopened it, changed a threshold from 24000 to 20000, saved successfully, and the temporary home was removed.
Blocked checks: none.

## Progress

- [x] Inspected context precedence, automatic-threshold, dashboard, config, command, input, render, and mutation-worker seams.
- [x] Add field-wise resolution and persistence tests.
- [x] Implement the editor and blocking save path.
- [x] Review snapshots and run all completion gates.
- [x] Reconcile docs and archive CTX-043 before USG-044.

## Decisions

- Decision: inheritance is field-wise rather than whole-record.
  Evidence: the approved plan says empty values inherit, while the current whole-record lookup cannot honor one empty field independently.
  Consequence: fully specified existing entries behave unchanged; partial entries gain deterministic lower-precedence fallback.
- Decision: existing exact model profiles own edited context fields.
  Evidence: model profiles already outrank `model_context` and contain capability/reasoning data that must remain intact.
  Consequence: the mutation patches only two keys in that table; models without a profile use exact `model_context`.

## Surprises and Discoveries

- The existing lookup treats any model-profile context field as replacing the entire pair, so implementing true empty inheritance requires a small resolution correction before adding UI.
- At 60 columns, the initial descriptive field rows clipped both their state and failure copy. The reviewed narrow layout now uses compact `Window`/`Checkpoint` rows and a two-line error instead.

## Recovery and Rollback

Revert only CTX-043 patches. Config tests retain originals under temporary directories and assert no write on validation failure. Never use destructive Git operations or real runtime state.

## Completion Record

Changed files: `src/{app,config,main}.rs`, `src/tui/{commands,input,render,render_tests}.rs`, ten context-limit snapshots, this plan, and the five harness documents.
Commands and results: focused context PASS (35); full tests PASS (461 unit + 25 CLI); format PASS; strict Clippy PASS; release PASS; smoke and isolated live TUI PASS.
Remaining risks: estimates remain tokenizer-free by design; models without any inherited or configured window cannot accept an explicit checkpoint threshold.
Harness updates: architecture, continuity, testing, next steps, and parity record CTX-043 as an oracle-informed `ca` extension and route the next handoff to USG-044.
