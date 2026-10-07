# HED-210 — Rich Launch and Worktree Controls

Status: COMPLETE (2026-08-28)
Owner/session: CUR-204 goal session
Last updated: 2026-08-28

## Goal

Add fresh-run mode, sandbox/trust, and digest-bound worktree launch/setup controls
without persistent authority changes.

## Non-Goals

No persistent `--trust`, silent sandbox weakening, automatic unapproved setup,
or deletion of a failed setup worktree.

## Evidence and Current Behavior

Plan/ask and sandbox already existed inside CA but were not complete one-shot
launch controls. Worktrees had a deliberate registry lifecycle but no
pre-registry launch/setup path.

## Scope

Allowed: main/runtime/worktree/sandbox/tool-selection/repo/TUI/test/docs code.
Preserve worktree ownership and SEC-158 authority boundaries.

## Design and Invariants

`--mode plan|ask` and `--plan` reuse existing restrictions. `--sandbox` is
process-only; `policy-only|off` require process-only `--trust`. All of these
fresh-run authority controls conflict with resume selectors. Worktree creation
precedes workspace registries, replaces only the primary root, and retains
supplemental roots. Native `.ca/worktrees.toml` wins over compatibility
`.cursor/worktrees.json`. Setup digest binds repo, source bytes, worktree, steps,
cwd, sandbox, and network. The TUI uses a Cancel-first asynchronous preview;
headless setup requires the exact digest or skips with a structured warning.
Failure retains registered state. `ca repo worktree-setup` is preview-only.

## Milestones

### M1 — Launch flags and authority

Failure-first flag combination, trust, sandbox, and mode tests; implement process
overrides.

### M2 — Worktree setup contract

Add native/compat parsers, precedence, name/base validation, digest binding,
preview, failure retention, and no-setup cases.

### M3 — Integration and validation

Construct worktree before registries, add TUI/headless coverage, and run gates.

## Acceptance Criteria

No setup runs without the matching authority/digest; failed trees remain
inspectable; launch restrictions match existing modes and persistence is intact.

## Validation

Focused CLI/worktree/repo/sandbox/TUI/headless tests and child gates.

## Progress

- [x] Failure-first launch tests.
- [x] Mode/sandbox/trust controls.
- [x] Setup parsing/digest/integration.
- [x] Focused and child gates.

## Decisions

Repository setup is typed data with exact approval, not arbitrary startup shell.
TUI confirmation is an in-app asynchronous modal so setup cannot block input and
wide/narrow behavior is deterministic; headless authority remains digest-only.

## Surprises and Discoveries

- The config hub's saved/effective sandbox display intentionally still parses
  persistent configuration; all execution consumers use the process-effective
  override.
- A pre-TUI stdin digest prompt would have met the mutation boundary but not the
  required TUI or snapshot behavior, so it was replaced before completion.
- macOS debug test links continue to emit the repository's known oversized
  `__eh_frame` compact-unwind warning; strict Clippy and release builds are clean.

## Recovery and Rollback

Tests own disposable worktrees; setup failure never triggers automatic deletion.
Interactive exit cancels an in-flight setup task while retaining the registered
worktree for inspection.

## Completion Record

Changed behavior: fresh launches accept plan/ask selection, process sandbox and
headless trust, registered worktree creation, typed digest-bound setup, and the
local preview-only repository command. Ask mode advertises no tools and receives
the same explicit guidance in context preview and provider prompts. Structured
headless startup warnings report every skipped setup without changing ordinary
output.

Focused results: eight setup-contract tests, 36 worktree-matching tests, CLI
mode/sandbox/worktree/repo tests, headless warning/tool-profile tests, primary
root replacement, and TUI setup input tests pass. Six 100x30/60x24 setup
preview/busy/failure snapshots were reviewed and accepted.

Completion gates: `cargo test --workspace --no-fail-fast -- --test-threads=1`
passed CA 2,216 tests with four documented ignores, CLI 38, role-run 17,
ca-watch 15 plus three CLI, codex-team 647 with one documented ignore plus 16
integration tests, protocol 12, and doc tests. `cargo fmt --all -- --check`,
strict all-target/all-feature workspace Clippy, and the all-feature workspace
release build passed. Release help/version and repository preview smokes,
`git diff --check`, zero `.snap.new`, direct protected-tree comparison, and
`codex-team verify protected-trees --repo . --json` passed.

No runtime profile, persistent trust, session, provider, installed binary,
oracle, reference, or protected-tree byte was changed. Installation remains
reserved for REL-215.
