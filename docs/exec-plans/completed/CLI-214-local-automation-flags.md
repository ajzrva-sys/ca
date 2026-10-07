# CLI-214 — Local Automation Flags

Status: COMPLETE (2026-08-29)
Owner/session: CUR-204 goal session
Last updated: 2026-08-29

## Goal

Add bounded opt-in local automation inputs, repository schemas/commands, version
gating, caller IDs, diff context, and redacted environment events while keeping
existing output byte-compatible by default.

## Non-Goals

No arbitrary transcript import, persistent imported history, environment
feedback, network/account queries, or unflagged output changes.

## Evidence and Current Behavior

`src/main.rs` lacks the requested local flags and repo/shell subcommands;
sessions already have export and Git/context primitives that can be reused
through stricter bounded adapters.

## Scope

Allowed: CLI/main/session portability/context/git/headless contracts/repo code,
focused fixtures/tests/docs. No provider/user state or remote queries.

## Design and Invariants

`--min-version` validates strict SemVer before config/session/provider work.
History accepts bounded `ca.session-export.v1`, verifies tool consistency,
conflicts resume, implies no-session, and is never stored. Excluding workspace
context retains user/global and explicit attachments/skills. New session ID is
a fresh collision-checked UUID. `--with-diffs` injects bounded host Git summaries
only. `--printenv` requires JSON/JSONL and emits redacted added/changed/removed
post-command observations without state feedback. `ca repo info|status` emit
stable `ca.repo-info.v1`/`ca.repo-status.v1` text/JSON with local metadata only.

## Milestones

### M1 — Preflight flags and history

Failure-first SemVer/order, bounded export/tool consistency, conflicts, session
ID collision, and context-exclusion cases; implement parsing/resolution.

### M2 — Diffs and environment output

Add bounded binary/truncation Git snapshot and secret-safe structured
environment-event tests/implementation.

### M3 — Repository commands

Add local schemas/text/JSON, readiness/worktree states, smokes, and child gates.

## Acceptance Criteria

Every flag combination is validated; malformed/oversized inputs fail before
side effects; existing outputs are byte-identical without opt-ins; repo commands
perform no network/account action.

## Validation

Focused CLI/session/headless/git/context/repo schema tests and child gates.

## Progress

- [x] Failure-first flag/contract tests.
- [x] Version/history/context/session ID.
- [x] Diff/printenv/repo commands.
- [x] Focused tests, formatting, strict Clippy, release build, and local smokes.
- [x] One complete all-feature workspace matrix passed after the isolated
  protected-tree timing checks.

## Decisions

Automation inputs are ephemeral launch evidence, not implicit persistence.

## Surprises and Discoveries

- Workspace-context exclusion could not simply omit workspace skill discovery:
  the contract retains explicit skills. `SkillRegistry` therefore hides native
  workspace instances from automatic catalogs, suggestions, and generated
  prompt commands while preserving exact user-directed activation.
- A workspace selected below a repository root must not permit `--with-diffs`
  to read its parent repository. Git capture now requires the repository root
  to be contained by the selected primary workspace.
- The exact all-feature workspace gate reached the protected
  `crates/codex-team` subtree twice and encountered pre-existing timing races:
  first `five_stdio_clients_share_one_query_and_persistent_receipt` failed one
  cache publication with `EEXIST`; its exact rerun passed. The next full rerun
  instead raced `stateful_clean_filter_cannot_replace_prevalidated_commit_bytes`
  and `five_stdio_clients_share_evidence_cache_and_last_consumer_cancellation`.
  The stateful-filter exact rerun passed; the cancellation test remains timing
  sensitive. No protected-tree source was changed. A third exact complete run
  then passed the entire matrix, including both previously failing cases.
  Post-completion TST-216 later replaced the cancellation test's fixed delay
  with a deterministic five-consumer marker barrier under explicit ownership;
  post-completion TST-217 then repaired the cache-publication `EEXIST` race with
  monotonic temporary-name sequences and bounded collision retry. Both complete
  workspace matrices now pass.

## Recovery and Rollback

Reverse opt-in parser/contract fields and additive repo module only.

## Completion Record

Implemented interfaces:

- strict pre-initialization `--min-version`;
- bounded ephemeral `ca.session-export.v1` history and collision-checked fresh
  UUID session identity;
- automatic workspace-context exclusion that retains user/global context plus
  explicit attachments and exact skills;
- bounded host-captured Git context and opt-in key-only environment deltas;
- local-only versioned `ca repo info|status` text/JSON surfaces.

Validation observed:

- focused parser, history, session collision, Git, context, environment,
  repository schema, and workspace-skill tests: passed;
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: passed;
- `cargo fmt --all -- --check`: passed;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed;
- `cargo build --workspace --release --all-features`: passed;
- release help/version and `ca repo info|status|worktree-setup` JSON smokes:
  passed;
- `cargo test --workspace --all-targets --all-features --no-fail-fast --
  --test-threads=1`: passed on the clean completion run. CA passed 2,296 tests
  with four documented ignores; the experiment target passed 118 with one
  documented ignore; all CLI, role-run, watcher, scheduler, MCP integration,
  protocol, example, and remaining workspace targets passed.
