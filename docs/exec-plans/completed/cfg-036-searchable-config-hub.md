# CFG-036 — Searchable Config Hub

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-10

## Goal

Replace the path-only `/config` command with a searchable keyboard-first hub that exposes current model, provider setup, reasoning, review model, MCP, skills, and config location while delegating complex edits to their existing safe managers.

## Non-Goals

- Do not build a generic TOML editor or display resolved credentials.
- Do not edit hooks, approval path rules, workspace trust, or raw MCP commands inline.
- Do not add named profiles in this slice.
- Do not claim Ante or Cursor parity.

## Evidence and Current Behavior

- `/config` currently shows only `ConfigStore::path()` (`src/main.rs`).
- Model/provider/reasoning/MCP/skills already have typed command paths and dedicated state.
- Ante documents a searchable settings dialog; it is secondary descriptive evidence only.

## Scope

Allowed files: `src/app.rs`, `src/tui/{commands,input,render,input_tests,render_tests}.rs`, `src/main.rs`, typed config persistence where required, snapshots, and harness docs.
Forbidden: oracle/reference trees, secret resolution/display, user runtime state, installed binary.

## Design and Invariants

- `/config [filter]` opens an idle-only overlay; typing filters and arrows/Home/End/Page keys navigate, Enter activates, Esc closes.
- Rows show safe current values and route to existing model/provider/MCP/skills surfaces. Config path is displayed but never opened or rewritten automatically.
- Reasoning uses the existing configured effort cycle and persists through the existing model selection/session behavior only; unsupported models remain read-only.
- Review-model editing receives a dedicated typed provider/model selection path that atomically preserves unrelated TOML.
- Returning from delegated surfaces reopens the hub with the originating row selected when practical; cancel never mutates config.

## Milestones

- [x] Existing command/state/persistence seams inspected.
- [x] Failing state/input/render tests added.
- [x] Searchable hub and delegated routes implemented.
- [x] Typed review-model persistence implemented.
- [x] Focused and completion checks passed.
- [x] Harness reconciled and plan archived.

## Acceptance Criteria

- `/config` opens the hub and `/config <filter>` applies the initial filter.
- Current model, reasoning, review model, provider setup, MCP, skills, and config file are visible without secrets.
- Navigation/filter/paste affect only the hub; Enter routes deterministically; Esc returns without mutation.
- Scalar saves preserve comments, providers, credentials, and unrelated TOML.
- Wide/narrow populated and no-match snapshots are reviewed.

## Validation

Focused command/input/render/config/orchestration tests, then repository completion gates and an isolated rebuilt-TUI walkthrough.

## Decisions

- Complex settings remain owned by their existing managers.
- Security-sensitive rules are not one-key toggles.

## Surprises and Discoveries

- None yet.

## Recovery and Rollback

Targeted patches to files in Scope; no runtime data migration is introduced.

## Completion Record

Changed files: shared app state, command/input/render paths, config persistence, orchestration, tests/snapshots, and harness documents.
Commands/results: focused suites PASS; full 387 unit + 23 CLI PASS; format, strict Clippy, and release PASS.
Remaining risks: delegated managers close back to their normal owning surface rather than reconstructing a suspended config-hub filter; live terminal walkthrough was not run.
Harness updates: `CONTINUITY.md`, `ARCHITECTURE.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` reconciled.
