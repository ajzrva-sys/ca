# ENV-009 — Runtime Paths and Secret-Safe Configuration

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-09

## Goal

All production config, data, session, and history paths come from an injected `RuntimePaths`; config mutations atomically patch the raw TOML so environment placeholders are never rewritten as resolved secrets.

## Non-Goals

- Workspace-root policy and process sandboxing (`WSP-020`).
- MCP or skill configuration.
- Changing provider credentials or the user's live configuration.

## Evidence and Current Behavior

- `src/config.rs::source_hermes_env` implicitly reads `~/.hermes/.env` and falls back to `/Users/andrewzimmerman`.
- `src/config.rs::CaConfig::load` expands the entire TOML and `CaConfig::save` serializes that resolved value, which can persist a resolved secret.
- `src/session.rs::SessionStore::production` and `src/app.rs::ShellHistory::new` independently discover `HOME` and fall back to the launch directory.
- `src/main.rs` and `src/agent/tools.rs` contain additional developer-specific home fallbacks.
- Inferred: one injected boundary prevents path-policy drift and makes tests hermetic.

## Scope

Allowed files: `Cargo.toml`, `Cargo.lock`, `src/runtime.rs`, `src/config.rs`, `src/session.rs`, `src/app.rs`, narrowly required callers in `src/main.rs`, `src/test.rs`, `src/agent/tools.rs`, focused tests, and harness docs.

Forbidden files/areas: `oracles/`, `reference/`, user config/data/session/history, credentials, installed binaries, and the running TUI process.

Runtime state affected: tests use temporary directories only; smoke tests use local-exit flags only.

## Design and Invariants

- `RuntimePaths` is constructed from an explicit home/root in tests and discovered once at production startup.
- Missing or non-Unicode `HOME` returns a deterministic setup error; it never selects cwd or a developer path.
- `ConfigStore` owns the raw file. Loading resolves placeholders only in the returned runtime copy.
- Approval/default mutations edit the raw TOML document and atomically replace it in the same directory.
- Default config auto-creation remains compatible and atomic.
- Ordinary default `AppState` remains in-memory; production explicitly injects the history path.

## Milestones

### M1 — Characterization
- Add temporary-root tests for path derivation, missing home, default creation, and placeholder-preserving patches.
- Validation: focused runtime/config tests fail against current behavior.

### M2 — Boundaries and callers
- Implement `RuntimePaths` and `ConfigStore`; remove implicit Hermes environment loading and developer fallbacks.
- Inject session/history/config paths at startup and patch raw config at mutation sites.
- Validation: focused tests pass and `rg` finds no developer fallback or Hermes env sourcing.

### M3 — Completion gates
- Run focused tests, full tests, fmt, strict clippy, release build, and local help/version smoke tests.
- Record actual results in this plan, `TESTING.md`, `CONTINUITY.md`, and `NEXT_STEPS.md` before moving the plan to completed.

## Acceptance Criteria

- Missing `HOME` cannot read or write another user's path.
- Config, sessions, and history share injected runtime paths.
- Patching approvals/defaults preserves `${SECRET}` text and never writes its resolved value.
- Config creation and legacy TOML parsing remain supported.
- All automated tests use temporary state.

## Validation

Focused: PASS — 2 runtime-path tests, 2 config-store tests, and 1 injected-history test.

Completion: `cargo test` PASS (162 unit + 3 CLI); `cargo fmt --check` PASS; release PASS; strict Clippy reports only the existing 3 findings in `tools.rs`/`approval.rs`.

Manual/visual: no visible TUI change; local `--help` and `--version` PASS.

Blocked checks: none.

## Progress

- [x] Inspected current path/config boundaries and claimed ENV-009.
- [x] Added red characterization tests.
- [x] Implemented runtime/config boundaries and migrated callers.
- [x] Completed validation and harness reconciliation.

## Decisions

- Decision: runtime values and raw config syntax have separate owners.
  Evidence: serializing `CaConfig` after environment expansion can persist plaintext secrets.
  Consequence: mutations use explicit `ConfigStore` patch operations, not a general resolved-config save.

## Surprises and Discoveries

- `toml_edit` can serialize just the approval-rule item, allowing the rest of the raw document (including comments and placeholders) to remain untouched.

## Recovery and Rollback

Revert only the files listed in Scope using an inverse patch. No runtime files are touched by this plan.

## Completion Record

Changed files: `Cargo.toml`, `Cargo.lock`, `src/runtime.rs`, `src/config.rs`, `src/main.rs`, `src/context.rs`, `src/session.rs`, `src/test.rs`, `src/agent/tools.rs`, `src/approval.rs`, and harness docs.

Commands and results: focused tests PASS; full test/format/release/help/version PASS; strict Clippy retains the 3 documented pre-existing errors.

Remaining risks: helper/subagent code still discovers the same runtime boundary on demand; `TOL-016` will centralize its injected execution context. `sandbox-exec` policy remains `WSP-020`.

Harness updates: `ARCHITECTURE.md`, `ENVIRONMENT.md`, `TESTING.md`, `CONTINUITY.md`, and `NEXT_STEPS.md` reconciled.
