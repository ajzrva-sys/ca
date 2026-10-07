# ENV-209 — Direnv and Shell Integration

Status: COMPLETE (2026-08-28)
Owner/session: CUR-204 goal session
Last updated: 2026-08-28

## Goal

Add an immutable redacted run-environment overlay, explicit trusted direnv
evaluation, and recoverable CA-native zsh integration management.

## Non-Goals

No automatic `.envrc` execution, secret persistence/model context/logging,
network authority inference, or claimed Cursor shell parity beyond evidence.

## Evidence and Current Behavior

Before this plan, CA launched providers and multiple child-process surfaces from
ambient/config environment without a single explicit overlay and had no direnv
or zsh integration manager.

## Scope

Allowed: new environment/shell modules, provider/process launch plumbing, CLI/
TUI commands, focused tests/docs. Runtime mutation remained confined to
disposable tests; no user shell installation was performed.

## Design and Invariants

`RunEnvironment` is immutable-by-snapshot, non-serializable, redacted in Debug,
and used by providers, jobs, terminals, hooks, MCP, LSP, setup, tools, editors,
and environment-dependent discovery. `.envrc` is detected only. `--direnv`
requires trusted launch authority; `/direnv` previews the workspace, fixed
command, sandbox/network policy, and current source digest, then requires exact
confirmation. `direnv export json` is bounded, sandbox-required, network-denied,
and process-group-cancelled on timeout. Re-evaluation requires a new digest and
approval. A loaded overlay is bound to its primary workspace so it cannot
silently follow `/workspace` into another root. Zsh integration has exact
markers, completions and terminal keys only, preview, regular-file checks,
timestamped SHA-labeled backup, atomic mode-preserving writes, idempotence, and
exact-block uninstall.

## Milestones

### M1 — Overlay contract

Failure-first secret sentinel, bounds, propagation, serialization/debug, and
provider/child surface tests; implement immutable overlay.

### M2 — Direnv

Add trusted CLI/TUI approval/digest/network/drift flow and hermetic fake-direnv
tests.

### M3 — Shell integration

Add generator, print/install/uninstall commands with backup/symlink/idempotence/
exact-removal tests and validate.

## Acceptance Criteria

Sentinel values reach only authorized child/provider environments and never
sessions/logs/model/debug; direnv cannot execute implicitly; shell removal
touches only CA markers.

## Validation

Focused environment/provider/jobs/MCP/LSP/CLI/TUI/shell tests and child gates.

## Progress

- [x] Failure-first overlay tests.
- [x] Overlay propagation.
- [x] Direnv flow.
- [x] Zsh integration and child gates.

## Decisions

- Environment authority is process-local and exposed through immutable
  snapshots; approved re-evaluation atomically replaces the global snapshot.
- `--direnv` conflicts with resume/continue selectors because launch-time
  workspace authority is not yet safely resolvable for those paths.
- The overlay is bound to one canonical primary workspace. Changing the primary
  root requires a fresh process rather than carrying possibly secret values to
  another project.
- Browser discovery consumes overlay HOME/PATH, but the browser process keeps
  its intentionally narrow isolated environment instead of inheriting the full
  development overlay.

## Surprises and Discoveries

- Provider construction occurs after an initial config load, so startup now
  performs a trust-only load, resolves and evaluates the authorized overlay,
  then reloads configuration before constructing providers and registries.
- TUI re-evaluation must rebuild environment-sensitive plugin, MCP, LSP, skill,
  and agent routing state; failure rolls the overlay back to its prior snapshot.
- macOS command cleanup needed process-group ownership to guarantee that a
  timed-out direnv evaluation cannot leave descendants behind.

## Recovery and Rollback

Tests use disposable homes and fake direnv executables. Real zsh installation
always retains a timestamped SHA-labeled recovery copy; uninstall removes exact
CA markers only. Reverse only ENV-209 hunks if rollback is required.

## Completion Record

Added `src/run_environment.rs` and `src/shell_integration.rs`; wired the overlay
through config/provider credential resolution and the authorized child-process
surfaces; added `--direnv`, `/direnv`, `ca shell-integration zsh`, and guarded
install/uninstall commands. Added deterministic CLI, overlay, drift, propagation,
shell-file, and 100x30/60x24 TUI coverage.

Focused results: eight run-environment tests, four shell-integration tests,
three direnv-focused CLI/TUI tests, and all 203 render tests pass. The final
serial workspace run passes CA 2,198 tests with four documented ignores; CLI
38, role-run 17, ca-watch 15 library plus three CLI, codex-team 647 library with
one documented ignore plus 16 integration tests, protocol 12, and doc tests all
pass. Formatting, strict all-feature Clippy, all-feature release build, release
help/version/shell smokes, diff/snapshot hygiene, exact protected-status
comparison, and protected-tree verification pass. No user `.zshrc`, runtime
profile, provider, session, installed binary, oracle, or reference state was
changed.
