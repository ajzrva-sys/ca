# WSP-205 — Named Multi-Root Workspaces

Status: COMPLETE (2026-08-28)
Owner/session: CUR-204 goal session
Last updated: 2026-08-28

## Goal

Provide a safe profile-private named workspace catalog and fresh-launch
multi-root selection through `--workspace`, repeatable `--add-dir`, and matching
slash commands while preserving resumed-session root authority.

## Non-Goals

No hosted workspace service, bare-profile catalog, symlink escape, session root
override, or independent root model per registry.

## Evidence and Current Behavior

`src/workspace.rs` already models multiple roots and sessions remember them.
`src/tui/commands.rs` exposes `/add-dir` and `/workspace`, but no named catalog
exists and `src/main.rs` lacks fresh-launch workspace flags.

## Scope

Allowed: workspace/runtime/session/main/TUI command code, focused tests and CUR
docs. Forbidden: evidence trees, user runtime state during tests, unrelated
session semantics.

## Design and Invariants

Persist `ca.workspace-catalog.v1` atomically at mode 0600 per profile: at most
128 bounded ASCII names, 32 canonical directory roots each. Exact catalog name
wins over ambiguous relative path; `./` and absolute inputs force path mode.
Loading establishes the primary saved root; canonical CLI additions append in
order after deduplication. Missing, duplicate, non-directory, and escaped roots
fail closed. Resume selectors conflict with both launch flags. All workspace-
dependent constructors receive one `WorkspaceContext`.

## Milestones

### M1 — Catalog contract

Add failing format, atomicity, permission, bound, path, symlink, ambiguity, and
deduplication tests; implement the private catalog.

### M2 — Launch and commands

Add CLI conflicts/resolution and `/save-workspace`, `/load-workspace`, and
`/workspace list|save|load|delete|cd|remove` with deterministic states.

### M3 — Propagation

Prove commands, rules, skills, MCP, LSP, attachments, sandbox, and provider
launch share roots; run focused and child completion checks.

## Acceptance Criteria

All catalog and launch scenarios in CUR-204 pass; bare rejects named operations
but accepts process-only additions; resumes retain recorded roots; no registry
sees a divergent root set.

## Validation

Focused workspace/catalog/CLI/session/TUI tests, then default tests, format,
strict Clippy, release build, wide/narrow review, and protected-tree check.

## Progress

- [x] Failure-first catalog tests.
- [x] Catalog and launch resolution.
- [x] Slash commands and multi-root propagation.
- [x] Focused and child gates.

## Decisions

One shared workspace object is authoritative; catalog resolution occurs before
workspace-dependent construction.

## Surprises and Discoveries

- Catalog loads must require the exact owner-only `0600` file mode, not merely
  absence of group/world bits.
- Runtime workspace replacement must construct the next command, skill, memory,
  MCP, LSP, scratch, and trust state before committing the new root set.

## Recovery and Rollback

Remove only the additive catalog module/fields and command hunks; disposable
tests own catalog roots.

## Completion Record

Implemented `ca.workspace-catalog.v1`, fresh-launch `--workspace` and repeatable
`--add-dir`, named workspace commands, and ordered multi-root propagation across
prompt commands, instructions/rules, skills, MCP, LSP, attachments, sandboxing,
provider launch, system prompts, and restored sessions. Exact catalog names win
over ambiguous relative paths; explicit relative or absolute paths force path
resolution; bare profiles keep process-only roots but no catalog.

Focused catalog, workspace, CLI, command, instruction, skill, and wide/narrow
snapshot tests pass. `cargo test --workspace --no-fail-fast --
--test-threads=1` passed (CA 2,152 passed/4 ignored, all other workspace targets
passed), as did `cargo fmt --all -- --check`, strict all-target/all-feature
Clippy, and `cargo build --workspace --release --all-features`. `git diff
--check`, zero `.snap.new`, direct protected-tree status, and
`codex-team verify protected-trees --repo . --json` passed. No runtime profile,
session, provider, installed binary, oracle, or reference state was changed.
