# INT-190 — CA ↔ Codex-Team Mechanistic Cooperation

Status: COMPLETE
Owner/session: current Codex session
Last updated: 2026-08-26

## Goal

Make `codex-team` the bounded mechanical co-processor for ordinary interactive
and headless CA agents while preserving its governor role over `ca role-run`.
An opted-in CA profile advertises one first-turn `delegate_mechanistic` tool;
CA supplies the repository revision, calls the exact local delegate MCP server,
polls durable jobs without provider turns, and renders model-free job controls.

## Non-Goals

- No natural-language host classifier before the model responds.
- No facade in Plan, review, checkpoint, scheduled, service, subagent, bare, or
  `role-run` surfaces.
- No product edits, task import, commit, install, network, operate tools, live
  provider traffic, runtime-profile mutation, or background service activation.
- No second baseline commit and no edits under `oracles/` or `reference/`.

## Evidence and Current Behavior

- `crates/codex-team/src/mechanistic/contracts.rs` owns the v1 action/request/
  receipt vocabulary, but CA cannot reuse it without coupling to the complete
  orchestrator crate.
- `src/mcp.rs::call_host_tool_structured` already performs exact non-activating
  host calls; `/recipes` and `/self-improve` use it without model requests.
- Ordinary CA agents currently need `search_mcp_tools` before the new remote
  tools become visible. `ca role-run` deliberately omits MCP.
- The ignored real-client fixture discovers the new observe catalog and then
  fails its stale ten-tool expectation.
- Initial SHA-256 fingerprints for every pre-existing implementation path were
  recorded in the initiating turn. Re-read and compare a path immediately
  before patching it; stop only if that exact path changed concurrently.

## Scope

Allowed files: workspace manifests/lockfile; a new lightweight protocol crate;
CA config, tool registry/selection, MCP, main/headless dispatch, app/TUI/job
presentation and adjacent tests; codex-team mechanistic/MCP/app compatibility
code and tests; current integration/control documentation.

Forbidden: oracle/reference trees, credentials, profiles, sessions, installed
binaries, live providers, services, deployment, Git cleanup/reset/stash, and
unrelated dirty bytes.

## Design and Invariants

- A protocol-only crate contains serde/schemars contracts and structural
  validation; execution authority remains in codex-team.
- `[mcp.mechanistic]` is absent/disabled by default. Enabled targets must be
  local stdio, network-free, resource-free, and exact delegate catalog servers.
- `delegate_mechanistic` accepts actions only. CA obtains the revision through
  host-owned status and always sends remote policy `prefer`.
- Query batches are synchronous. Evidence or one observation starts once and
  is polled by CA in bounded 20-second waits. Dropped/cancelled/timed-out work
  requests remote cancellation once and never signals an observed PID.
- Remote raw tool names remain hidden from model MCP search for the configured
  facade server. Operate tools are never proxied.
- Current-session job state is capped at 50 metadata summaries. Full bounded
  receipts remain canonical tool results, not duplicated telemetry.
- `ca role-run` remains MCP/facade-free; codex-team performs pre-Worker routing
  and supplies compact context before launching CA.

## Milestones

### M1 — Shared protocol and compatibility
- Add the protocol crate, migrate/re-export codex-team contracts, and prove
  schema/serde parity without changing the 11/15/28 MCP catalogs.

### M2 — Profile opt-in and first-turn facade
- Add config validation, tool selection, exact local-server validation, host
  revision binding, query execution, start/poll/cancel, and explicit fallback
  receipts for ordinary TUI/headless surfaces only.

### M3 — Cockpit and activity
- Add `/delegate [JOB_ID]`, bounded process-local snapshots, unified `/jobs`
  rows/details, toasts, cancellation, fixed categorical feedback, and exact
  human correction confirmation without provider/token activity.

### M4 — Bidirectional proof and completion
- Add real-client and combined codex-team→CA tests, reconcile docs, and run the
  focused and repository completion matrix.

## Acceptance Criteria

- Enabled normal TUI/headless requests advertise the facade on turn one without
  starting MCP; every excluded surface omits it.
- One model call selects a typed action batch; CA performs status plus query or
  start/poll internally and returns one bounded terminal receipt.
- Unavailable/declined work explicitly permits prefer fallback; stale, failed,
  timed-out, and cancelled work never silently falls back.
- Cockpit operations make zero provider calls and do not change token usage.
- Query context reaches a CA Worker, Rust-only evidence launches no CA child,
  edit work launches exactly one, and required-unavailable work pauses.
- Exact real CA client discovery passes for 11/15/28 tools and zero resources.

## Validation

Focused protocol/config/tool/MCP/job/TUI and combined integration tests first,
then:

    cargo test --workspace --no-fail-fast -- --test-threads=1
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo build --workspace --release --all-features
    git diff --check

Also run release help/version, snapshot/diff/protected-tree checks, the actual
CA/codex-team compatibility tests, and the production isolation probe without
the debug override. Record environment or dirty-baseline blockers honestly.

## Progress

- [x] Inspect both integration directions and record initial path fingerprints.
- [x] Complete M1 shared protocol extraction and compatibility.
- [x] Complete M2 profile opt-in and first-turn facade.
- [x] Complete M3 cockpit and unified activity.
- [x] Complete M4 bidirectional proof, documentation, and validation.

## Decisions

- First-turn facade, not MCP-search-only or manual-only.
- Profile opt-in with default-off prefer behavior.
- Normal interactive plus ordinary headless only.
- Cockpit plus unified jobs/activity presentation.
- Digest-guarded narrow edits in the current dirty tree; no baseline commit.

## Recovery and Rollback

All changes are additive or re-export compatible. Disable the profile table to
remove the facade. Never reset or clean the shared tree; recover by reverting
only INT-190 hunks after comparing the recorded pre-edit path fingerprints.

## Completion Record

Completed 2026-08-26. The shared protocol crate, default-off profile table,
first-turn CA facade, host-owned revision/query/job polling and exactly-once
drop cancellation, process-local activity, `/delegate`, unified `/jobs`, fixed
feedback, restored durable IDs, and combined reverse integration are present.

The final serial workspace run passed 2,070 CA library tests with two
documented ignores, 38 CLI tests, 17 role-run tests, 15 ca-watch library tests,
three ca-watch CLI tests, 580 codex-team library tests with one documented
external-binary ignore, all eleven codex-team integration tests, and three
protocol tests. Formatting, strict all-target/all-feature Clippy, and the
all-feature release build pass. The explicitly invoked release-binary fixture
passes the exact 11/15/28 catalogs and zero-resource contract. Release help and
version checks pass for all three binaries, and the release `ca role-run`
preflight/read/write boundary fixture passes. Production release doctor reports
the verification isolation subcheck healthy as
`macos-seatbelt-source-projection-v1`; doctor as a whole exits one solely
because the root is intentionally not initialized as a codex-team target.

`git diff --check`, direct protected-tree Git inspection, and built-in snapshot
hygiene pass. The built-in protected-tree check fails closed because ignored
`oracles/` and `reference/` have no trusted content baseline; direct status and
diff inspection names no protected-tree change.

No profile/session/runtime state, installed binary, provider, network service,
oracle, or reference tree was changed. No implementation commit was created.
