# PLG-208 — Plugin Discovery UX and Ephemeral Plugins

Status: COMPLETE (2026-08-28)
Owner/session: CUR-204 goal session
Last updated: 2026-08-28

## Goal

Expose CA's existing verified plugin/catalog lifecycle in a TUI cockpit and add
bounded, collision-safe, non-persistent local `--plugin-dir` loading.

## Non-Goals

No arbitrary Git marketplace, unsigned remote discovery, hosted installation,
or weakening of CLI digest/collision checks.

## Evidence and Current Behavior

Before this plan, `src/plugin.rs` supported verified stores, signed catalogs,
staging, and rollback, but lacked a TUI manager and process-only plugin roots.

## Scope

Allowed: plugin/runtime/main/TUI code, focused tests/snapshots, docs. Bare
profile, evidence trees, and persistent registry writes from ephemeral loading
are forbidden.

## Design and Invariants

`/plugin` browses/searches/inspects/adds/enables/disables/removes, operates signed
catalog staging/update/rollback, and reuses current confirmation/digest/drift/
collision enforcement. Each `--plugin-dir` is canonical, bounded, contains
exactly one `plugin.toml` or `plugin.json`, contributes for one process only,
and never touches the registry. Bare rejects it.

## Milestones

### M1 — Ephemeral loading

Add non-persistence, bounds, manifest ambiguity, collision, drift, and bare
failure tests; integrate process-local contributions.

### M2 — Management cockpit

Add deterministic list/detail/search/confirmation/catalog/rollback states and
wide/narrow snapshots.

### M3 — Validation

Run existing plugin/catalog suites, new CLI/TUI coverage, and child gates.

## Acceptance Criteria

Ephemeral plugins vanish on the next process and cannot bypass trust; all TUI
mutations have the same enforcement as CLI operations.

## Validation

Focused plugin, registry, CLI, input/render tests and standard child gates.

## Progress

- [x] Failure-first ephemeral tests.
- [x] Process-local loading.
- [x] Plugin cockpit.
- [x] Focused and child gates.

## Decisions

Trusted local catalogs remain the only discovery source.

## Surprises and Discoveries

- macOS temp paths commonly enter through the system `/var` symlink. Ephemeral
  roots therefore reject a symlink as the selected directory itself, then
  canonicalize before applying the existing component/resource checks; this
  preserves platform-canonical paths without allowing plugin resources to
  escape their canonical bundle.
- The signed-catalog runner originally wrote directly to stdout. It now has one
  shared result-producing core: the CLI prints that result, while the TUI shows
  it in the pager, so catalog verification/staging/rollback enforcement is not
  duplicated or weakened.
- One unchanged capability-handle test from the active agent-lifecycle work
  failed during the first full run and passed immediately in isolation; the
  subsequent complete serial workspace run passed.

## Recovery and Rollback

Tests use disposable registries; reverse additive flags and cockpit state only.

## Completion Record

Added repeatable `--plugin-dir` launch roots with a 32-directory bound, exact
single-manifest selection, canonical-directory deduplication, existing bundle
size/resource/digest validation, persistent/ephemeral collision checks, and
zero registry writes. Bare profiles reject the flag before launch.

Added `/plugin` and `/plugins` for list, search, safe detail, add, enable,
disable, remove, and the complete trusted local catalog trust/untrust/sync/list/
install/update/rollback lifecycle. Mutating operations use explicit preview and
exact confirmation where the CLI does, then reuse the same digest, drift,
collision, signature, staging, and rollback code. No remote marketplace or
repository loading was added.

Focused plugin/catalog/CLI/command/render coverage passes (21 filtered tests,
including the existing end-to-end local plugin case). Wide/narrow cockpit and
the command-count snapshot were reviewed. The final workspace run passes CA
2,182 tests with four documented ignores, CLI 38, role-run 17, ca-watch 15
library plus three CLI, codex-team 647 library with one documented ignore plus
16 integration, and protocol 12. Formatting, strict all-feature Clippy,
all-feature release build, release help/plugin help, diff/snapshot hygiene,
exact protected-status comparison, and protected-tree verification pass. No
runtime registry, installed binary, oracle, or reference state was changed.
