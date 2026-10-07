# KNL-206 — Conditional Rules and Command Management

Status: COMPLETE (2026-08-28)
Owner/session: CUR-204 goal session
Last updated: 2026-08-28

## Goal

Add bounded always/glob/manual rules across all workspace roots and guarded TUI
management for native rules and custom commands.

## Non-Goals

No prompt-prose path inference, model-controlled activation, built-in override,
or mutation of plugin, compatibility, or auto-distilled commands.

## Evidence and Current Behavior

Rules are loaded unconditionally from the primary `.ca/rules`; the existing
prompt-command registry is multi-scope and collision-aware but has no manager.

## Scope

Allowed: rule/command/session/workspace/TUI modules, focused tests/snapshots and
CUR docs. Foreign sources are read-only; evidence trees are forbidden.

## Design and Invariants

Legacy Markdown remains always-on. Version-compatible frontmatter supports
description, `always|glob|manual`, and bounded globs. Glob activation uses only
explicit attachments, `@` paths, or `--with-diffs`; manual IDs are defaulted in
saved sessions. `/rule` and `/command` browse/filter/view/create/edit/delete/
reload with native-root, preview, confirmation, digest, and drift enforcement.

## Milestones

### M1 — Rule model and matching

Failure-first metadata, legacy, glob-source, manual persistence, malformed, and
multi-root tests; implement deterministic discovery and activation.

### M2 — Rule and command cockpits

Add guarded CRUD/reload flows, read-only foreign visibility, collision/built-in
precedence, editor/delete drift coverage, and wide/narrow snapshots.

### M3 — Validation

Run focused session/context/command/TUI suites and child completion gates.

## Acceptance Criteria

Only explicit file evidence activates globs; session manual activation survives
save/load; non-native sources cannot be edited; all legacy behavior remains.

## Validation

Focused rule, prompt-command, session, attachment, diff-context, input/render
tests; standard child gates and snapshot/protected-tree hygiene.

## Progress

- [x] Failure-first rule tests.
- [x] Conditional discovery and persistence.
- [x] Rule and command cockpits.
- [x] Focused and child gates.

## Decisions

Rule activation is host/user state, never a model tool.

## Surprises and Discoveries

- The legacy project-instruction contract included the visible `Rule: name`
  heading. The new prompt renderer retains that prefix while also including the
  stable scoped rule ID.
- One unchanged five-client `codex-team` MCP timing test failed during an
  intermediate full run, passed immediately in isolation, and passed in the
  subsequent clean full workspace rerun.

## Recovery and Rollback

Reverse additive metadata/defaulted fields and manager hunks only.

## Completion Record

Implemented bounded conditional rules from user scope and every workspace
root, with legacy/always/glob/manual activation, explicit host-path matching,
defaulted saved-session manual IDs, and stable scoped IDs. Added `/rule` and
`/command` list/filter/view/create/edit/delete/reload flows. Native user/project
assets use preview, exact digest confirmation, atomic owner-only replacement,
and drift/symlink/hardlink checks; compatibility, plugin, generated, skill, and
auto-distilled command sources remain read-only, and built-ins retain
precedence.

Focused rule, mutation, session, instruction, prompt-command, manager, and
wide/narrow snapshot tests pass. `cargo test --workspace --no-fail-fast --
--test-threads=1` passed on the clean completion run (CA 2,164 passed/4
ignored; every other workspace target passed). `cargo fmt --all -- --check`,
strict all-target/all-feature Clippy, and `cargo build --workspace --release
--all-features` passed. `git diff --check`, zero `.snap.new`, direct protected-
tree status, and `codex-team verify protected-trees --repo . --json` passed.
No runtime profile, provider, installed binary, oracle, or reference state was
changed.
