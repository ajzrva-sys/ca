# SAN-042 — Sandbox Hub

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-10

## Goal

Provide an idle-only `/sandbox` manager, also reachable from `/config`, that truthfully explains the saved and effective process sandbox, backend readiness, writable workspace roots, and network enforcement, and safely persists a mode for the next launch.

## Non-Goals

- Do not hot-switch the sandbox used by already-constructed tool, MCP, job, or subagent managers.
- Do not add a global network toggle or imply policy-only/off modes enforce network denial.
- Do not change approval rules, workspace roots, provider behavior, or bare-profile persistence.
- Do not edit or copy oracle/reference source.

## Evidence and Current Behavior

- `src/sandbox.rs::SandboxMode` supports `required`, `policy-only`, and `off`; unknown runtime values fail closed to `required`.
- `src/sandbox.rs::SandboxBackend` invokes `/usr/bin/sandbox-exec` only in required mode. Policy-only and off execute processes directly, so their network restrictions are not OS-enforced.
- `src/main.rs::run_tui` captures the configured sandbox mode while constructing MCP and subagent managers. A live config change cannot safely replace all already-constructed boundaries.
- `src/config.rs::ConfigStore` already has comment-preserving TOML edits and atomic replacement, but no sandbox-only mutation.
- `/config` is a searchable hub, but it has no sandbox row; there is no `/sandbox` command or manager state.
- Ante and clean Cursor materials motivate an inspectable sandbox/settings surface, but provide no verified generic `ca` fixture. This work is an oracle-informed `ca` extension, not a parity claim.

## Scope

Allowed files:
- `src/sandbox.rs`, `src/config.rs`, `src/app.rs`, `src/main.rs`
- sandbox-specific changes in `src/tui/{commands,input,render}.rs` and focused tests/snapshots
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`
- this ExecPlan

Forbidden files/areas:
- `oracles/`, `reference/`, real runtime configuration, Keychain, saved sessions, installed binaries
- unrelated formatting/refactors and live manager reconstruction

Runtime state affected: only temporary config files created by tests and the explicit saved sandbox field when a real user invokes the new action. Repository validation uses temporary homes only.

## Design and Invariants

- Store the effective startup mode separately from the saved config value. A successful save changes only the saved value and clearly states that a restart is required.
- Manager state contains only typed modes and sanitized paths/readiness summaries. It contains no resolved credentials or provider runtime config.
- `required` is the only mode described as OS-enforcing workspace write and per-command network policy. `policy-only` and `off` explicitly disclose direct process execution.
- Selecting a weaker mode opens a confirmation with Cancel selected. Selecting required may save directly.
- Bare mode is fully inspectable but view-only and explains how to rerun with a persistent profile.
- Save through one blocking mutation worker. While busy, further mode changes are locked. A failure keeps the manager open with the same selection and an inline error.
- TOML mutation validates the exact canonical strings, preserves unrelated values/comments, and remains atomic. Runtime parsing of unknown legacy values stays fail-closed.

## Milestones

### M1 — Characterization and typed boundaries
- Add failing tests for command visibility/idle availability, strict mode parsing for edits, comment-preserving config mutation, state transitions, and safe readiness summaries.
- Add typed display/storage helpers without changing process execution semantics.
- Validation: focused sandbox/config/command tests.

### M2 — Manager state, input, and rendering
- Add `/sandbox`, `/config` routing, summary/mode rows, bounded navigation, bare behavior, Cancel-first downgrade confirmation, busy/error states, and wide/narrow snapshots.
- Validation: focused input and render tests with snapshot review.

### M3 — Background persistence and orchestration
- Add one blocking save path, reload config on success, preserve effective mode, and apply inline success/failure state.
- Validation: orchestration tests including successful save, no-write failure, and current-process immutability.

### M4 — Completion gates and reconciliation
- Run focused tests, repository-wide tests, formatting, strict Clippy, release build, CLI smoke tests, and a temporary-home TUI/config walkthrough where supported.
- Reconcile the five harness documents and move this plan to completed.

## Acceptance Criteria

- `/sandbox` is discoverable only while idle and `/config` opens the same manager.
- The manager shows saved/effective modes, backend readiness, writable roots, and truthful network behavior without secrets.
- Required can be saved directly; policy-only/off require explicit confirmation with Cancel selected.
- A successful save preserves comments/unrelated TOML and becomes effective only after restart; failure stays inline.
- Bare mode performs no write and gives actionable guidance.
- Wide and narrow views remain usable, including confirmation, busy, and failure states.

## Validation

Focused: `cargo test sandbox_` passed 16 tests; the config-hub routing regression also passed.
Completion: `cargo test --no-fail-fast` passed 446 unit and 25 CLI tests; format, strict Clippy, release, help/version/profile smoke all passed.
Manual/visual: ten wide/narrow snapshots were reviewed. The rebuilt TUI opened `/sandbox`, displayed the real backend/root state, canceled an `off` downgrade without writing, and refused the same change under `--profile bare` in an isolated temporary home.
Blocked checks: none. One recursive cleanup command was rejected before execution; explicit file deletion and empty-directory removal completed successfully.

## Progress

- [x] Inspected sandbox, config, runtime construction, command, input, and render seams.
- [x] Add characterization tests and typed state/config boundaries.
- [x] Implement manager and background save orchestration.
- [x] Review snapshots and run full completion gates.
- [x] Reconcile docs and archive SAN-042 before starting CTX-043.

## Decisions

- Decision: sandbox changes are next-launch only.
  Evidence: MCP and subagent managers capture `SandboxMode` during `run_tui` construction.
  Consequence: the UI retains both saved and effective values and never claims a live switch.
- Decision: no standalone network toggle.
  Evidence: only required mode converts `SandboxPolicy.network_allowed` into an OS sandbox profile.
  Consequence: weaker modes disclose that network behavior depends on approval/policy rather than OS enforcement.

## Surprises and Discoveries

- The completed APR-003 plan was still stored under `active/`; it was archived before SAN-042 began so only one plan is active.
- The repository has no tracked Git baseline in this checkout, so validation and file-scope review must use focused inspection rather than a blanket diff.

## Recovery and Rollback

Revert only SAN-042 additions with targeted patches. Automated state lives under temporary directories. If a config mutation test fails, retain the original temporary file for assertion; never use destructive Git commands or touch the user profile.

## Completion Record

Changed files: `src/{app,config,main,sandbox}.rs`, `src/tui/{commands,input,input_tests,render,render_tests}.rs`, ten sandbox snapshots, this plan, and the five harness documents.
Commands and results: focused sandbox/config/command tests PASS; 446 unit plus 25 CLI tests PASS; format PASS; strict Clippy PASS; release PASS; smoke and isolated live TUI PASS.
Remaining risks: backend readiness is a local capability check, not proof that every future macOS release accepts every generated profile. Saved changes intentionally require restart.
Harness updates: architecture, continuity, testing, next steps, and parity now record SAN-042 as an oracle-informed `ca` extension and route the next handoff to CTX-043.
