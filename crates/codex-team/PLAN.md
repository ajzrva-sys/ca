# ORCH-CA-001 — Production-Safe `codex-team` and `ca` Integration

Status: ACTIVE — artifact learning loop validated; clean commits and production-isolation MCP pilot pending
Owner/session: Codex `/root`
Last updated: 2026-08-22

## Purpose / Big Picture

Integrate `codex-team` with `ca` while keeping Rust in the orchestrator authoritative for task admission, scope guards, verification, evidence, acceptance, and commits. `ca` becomes an optional role engine and operator cockpit. V1 remains supervised: typed task import, one cycle, explicit pending-commit approval, and explicit human disposition.

Existing users remain on Codex-backed roles and automatic commits unless they opt into the new configuration. The integration must not take over `ca` sessions, profiles, credentials, native task graphs, or control documents.

## Progress

- [x] Record both repository baselines and create an isolated orchestrator implementation worktree.
- [x] Add characterization tests for current commit lanes.
- [x] Centralize commit authority and implement manual pending commits.
- [x] Add clean-start enforcement and ignored `.orchestrator` runtime state.
- [x] Add digest-bound typed task import.
- [x] Add exact typed verification recipes required by `ca`.
- [x] Add public `ca role-run` and offline preflight contract in the clean target integration branch.
- [x] Add first-class orchestrator-side `RoleProvider::Ca` support for every role.
- [x] Complete fake and real-process CLI integration tests, including all four role schemas and allowed/denied Worker writes.
- [x] Replace and harden MCP for observe-only use, including actual `ca::McpManager` discovery.
- [x] Add asynchronous, cancellable MCP operation tools (orchestrator fake-process and SDK tests pass).
- [x] Add the enrolled-profile autonomous artifact loop: sanitized Dream/Distill
      bridge, one-stage active-binary worker, frozen replay, immutable promotion,
      foreground yielding, quarantine/cooldown, and cockpit visibility.
- [x] Refresh both complete matrices and run one disposable fake-provider
      artifact global-activation pilot.
- [ ] Complete clean CLI and MCP pilots.
- [x] Reconcile documentation, tracking files, validation evidence, and the pilot-only completion record.

## Surprises & Discoveries

- Since the 2026-08-22 consolidation, the canonical workspace is
  `/Users/aji/project/ca`, with this orchestrator embedded at
  `crates/codex-team`. The prior target and orchestrator integration tips are
  preserved as `archive/ca-tui-polish` and
  `archive/codex-team-ca-integration` in the root repository.
- The formerly separate dirty checkout bytes and untracked plan were preserved
  in source-only archives before the redundant folders were retired.
- At baseline, the custom MCP source conflicted with documents that described MCP as deferred. The implementation and operator docs now agree on the SDK-backed pilot surface, and actual `ca::McpManager` observe/operate compatibility passes hermetically.
- `ca` unconditionally discovers both MCP tools and resources; the baseline custom server omitted `resources/list`, which the SDK-backed server now supplies as an empty fixed list.
- Automatic commits are spread across Worker, role-request, Rust-patch, deterministic, queue, and control-document lanes.
- All characterized Git lanes now route through one coordinator. Legacy auto mode remains the default; manual mode records one digest-bound intent with exact dirty paths/fingerprints and a deferred accepted-task transition.
- Imported task IDs are retained across Markdown queue reconciliation, and accepted imported work does not write `NEXT_STEPS.md`. The pending intent binds the immutable import digest.
- The orchestrator-side `CaRunner` is wired for CEO, Project Manager, Senior Dev, and Worker. It has fixed authority construction, bounded process/JSONL handling, securely created sanitized artifacts, and child/grandchild termination tests. A real clean `ca` build passes all four role schemas, one allowed Worker write, and one denied outside-scope write through this boundary.
- The custom MCP loop has been replaced by `rmcp`. Observe exposes exactly three tools and zero resources. Operate adds six tools and durable asynchronous step jobs with revision, pause, active-job, manual-mode, clean-start, and provider preconditions.
- The refreshed orchestrator completion matrix passes strict all-feature
  Clippy, 539 library tests with one documented ignore, ten integration tests,
  all-feature release, diff hygiene, snapshot hygiene, and protected-tree
  checks. The disposable process pilot exercises fake Codex/DeepSeek/MiMo
  routes and immutable skill activation without source or deployment mutation.
- The strict cycle/status/disposition receipt schemas were reconciled with the additive manual/revision/pending DTO fields, and their focused schema-contract test passes within the full suite.
- Adversarial audits found and closed same-cycle fall-through after pending intent creation, dirty/staged revision replay, symlink and case-alias Worker scope escape, custom guard-ignore weakening, inherited-pipe process hangs, unbounded doctor probes, raw provider-output leakage, MCP nested-child hard-cancel gaps, missing partial-state approval latching, commit-mode/hook ambiguity, Git-success/state-save recovery, incomplete import receipts, insecure run-artifact publication, and live orphan recovery after MCP restart. The refreshed aggregate matrix covers these closures.
- The clean `ca` implementation now has pinned read calls, bounded cancellable `blame`, malformed-Deny rejection, selected-route-only credential loading, output credential redaction, and a role-only hardlink/symlink-safe mutation boundary. Its refreshed all-feature matrix passes.
- `run --auto-continue` now fails closed whenever manual commit mode or any `ca`-backed role is selected. The legacy Codex/auto-commit workflow remains compatible.

## Decision Log

- This root `PLAN.md` is the source of truth by explicit user request.
- `codex-team` is the only scheduler during an orchestrated cycle.
- V1 is supervised and single-cycle; no production `run --auto-continue`.
- Tasks enter through a strict digest-bound JSON import, never by rewriting `ca/NEXT_STEPS.md`.
- `ca` receives exact process-scoped grants through a versioned `ca role-run` contract; no broad `--yolo`.
- Every orchestrator role may use the `ca` backend, with one configured internal `ca` provider and per-role model/effort.
- MCP defaults to observe-only and pins its repository at launch.
- Mutating MCP requires manual commit mode, clean-start enforcement, stale-state preconditions, and cancellable engines.
- Existing configuration remains Codex-backed and auto-committing by default.

## Context and Orientation

Orchestrator root: `/Users/aji/project/ca/crates/codex-team`.

Primary boundaries are `src/app.rs` (conductor), `src/git.rs` (commit lanes), `src/config.rs`, `src/state.rs`, `src/domain/task.rs`, `src/verification/`, `src/codex/run.rs`, `src/mcp/`, and `src/app/receipts.rs`.

The target `ca` boundaries are rooted at `/Users/aji/project/ca` and include
`src/main.rs`, `src/role_run.rs`, `src/schedule.rs`, `src/headless.rs`,
`src/headless_contract.rs`, `src/permission.rs`, `src/runtime.rs`, and
`src/mcp.rs`.

Forbidden areas are both repositories' `oracles/` and `reference/` trees, real runtime profiles/configuration, credentials, sessions, installed binaries, deployment, and destructive Git/filesystem operations.

## Plan of Work

### M0 — Characterization and tracking

Keep this plan and `NEXT_STEPS.md` current. Characterize every current commit lane and the existing MCP handshake defects before changing behavior.

### M1 — Manual commit authority

Add backward-compatible `commit_mode = "auto" | "manual"` and `require_clean_start = false`. Centralize commit behavior behind one coordinator. In manual mode, verified work creates one fingerprinted `PendingCommit` in state without staging, committing, popping the task, or applying final tracking transitions. `step` and `run` refuse while an intent exists.

Add `commit-pending --id --expected-digest --expected-revision --json`. It must verify base HEAD, empty index, exact dirty paths and fingerprints, evidence, and the current revision. Failure leaves Worker bytes and pending state unchanged. No discard/reset command is added.

### M2 — Clean adoption, revision, task import, and verification

Add idempotent `.git/info/exclude` markers for `/.orchestrator/`, and make doctor report whether runtime state is ignored. When `require_clean_start` is enabled, check tracked, staged, deleted, renamed, and untracked paths before recovery or role calls, excluding only `.orchestrator/**` and configured generated roots.

Add a stable repository revision over canonical root, HEAD, sorted non-runtime porcelain state, and bounded authoritative config/state/control-document bytes.

Add `task-import --input FILE|- --json` preview and digest-bound apply with `--accept-digest` and `--expected-revision`. Accept exactly one strict `codex-team.task_import.v1` task, reject unknown fields and unsafe paths, write immutable import receipts under `.orchestrator/imports/`, and enqueue without model calls, document edits, staging, or commits. Imported completion remains in orchestrator state/evidence and never rewrites `ca` documents.

Extend typed no-shell verification to exact `ca` gates: complete default/all-feature tests, safe focused tests, all-format check, strict all-target/all-feature Clippy, release all-features build, released help/version, diff check, snapshot hygiene, and protected-tree checks. Reject arbitrary Cargo flags, environment prefixes, shells, redirects, selectors, and network commands.

### M3 — Public `ca role-run`

Add `ca role-run --profile NAME` and `--preflight`. Reuse restricted schedule-worker preparation but expose a new strict `ca.role_run.v1` stdin contract with canonical workspace, provider/model/effort, prompt, contained output schema, exact tools, and process grants. Named profiles only; required sandbox; persistent Deny rules remain ceilings while persistent positive rules are removed. No sessions, writes to runtime config, skills, plugins, MCP, LSP, hooks, agents, workflows, jobs, browser, email, communication, or model-visible network tools.

Stdout is only `ca.headless.v2` JSONL. Success requires exactly one object-valued structured output followed by completion. Cancellation reaps descendants. Add `ca/docs/ROLE_RUN_PROTOCOL.md` and `/.orchestrator/` to the clean integration branch's `.gitignore`.

### M4 — Orchestrator `CaRunner`

Add disabled-by-default `[ca]` config (`bin`, named `profile`, internal `route_provider`), `RoleProvider::Ca`, a separate `CaRunner`, provider-neutral role dispatch, and explicit read-only versus Worker authority. All roles may use `ca`; no fallback or engine substitution.

Read-only roles receive only repository read tools. Worker adds `apply_patch` and `write_file`, with path grants derived from the same normalized allowed patterns used by the Rust guard. Shell, Git, delete, agents, jobs, MCP, browser, web, communication, and network tasks are refused.

The runner launches only the absolute `ca role-run` binary, uses a process group, bounded concurrent stdout/stderr readers, timeout/cancellation cleanup, JSONL sequence/run validation, and existing typed role parsing. Guard, verification, and commit never run after failure or malformed output. Doctor uses `ca --version` plus offline role-run preflight.

### M5 — MCP observe mode

Replace the custom loop with the same pinned `rmcp` release used by `ca`, or otherwise prove exact protocol compatibility if dependency evidence requires a different implementation. Canonicalize and pin `--repo`; remove repository and rendering switches from tool arguments; implement proper notifications, tools, and empty resources; return typed structured receipts without global stdout capture. Default catalog is `status`, `doctor`, and `step_status` only.

### M6 — MCP operate mode

Add `--access operate`, stable revisions/pause fingerprints, and durable atomic jobs under `.orchestrator/mcp/jobs/`. `step_start` validates and returns immediately; `step_status` supports bounded polling; `step_cancel` gracefully then forcibly reaps process groups. Add typed task preview/apply, commit-pending, and disposition tools. Operate requires manual commits, clean-start, and cancellable Codex/`ca` engines; Ollama/MLX remain rejected.

### M7 — Integration tests, dogfood, and documentation

Use fake and loopback providers for automated cross-process tests. Add ignored explicitly invoked real-binary tests for `CA_BIN` and the real `ca` MCP manager. Live provider dogfood requires separate authorization and a clean disposable target worktree. Use one task, one step, one explicit commit; never auto-continue or infer live success from fake tests.

Update README, architecture, continuity, testing, MCP/Hermes docs, `docs/CA_INTEGRATION.md`, and a repository-contained supervisor skill. Do not install it automatically.

## Concrete Interfaces

Configuration defaults:

```toml
commit_mode = "auto"
require_clean_start = false

[ca]
enabled = false
bin = "/absolute/path/to/ca"
profile = "codex-team"
route_provider = "openai"
```

Manual approval:

```text
codex-team commit-pending --repo PATH --id ID \
  --expected-digest SHA256 --expected-revision SHA256 --json
```

Task import:

```text
codex-team task-import --repo PATH --input FILE_OR_DASH --json
codex-team task-import --repo PATH --input FILE_OR_DASH \
  --accept-digest SHA256 --expected-revision SHA256 --json
```

MCP:

```text
codex-team mcp --repo CANONICAL_GIT_ROOT \
  --access observe|operate --job-timeout-seconds 7200
```

Observe tools are `status`, `doctor`, and `step_status`. Operate additionally exposes task import preview/apply, step start/cancel, commit pending, and human disposition. All schemas deny unknown fields.

## Validation and Acceptance

Focused coverage must prove configuration migration, every commit lane, pending fingerprints/rollback, clean-start and ignore behavior, task import idempotency/conflicts, exact verification recipes, `CaRunner` grants/output/cancellation/all-role dispatch, MCP negotiation/resources/structured results/jobs/stale preconditions, and temporary-Git end-to-end cycles.

Orchestrator completion:

```text
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
git diff --check
```

`ca` completion:

```text
cargo test role_run --no-fail-fast
cargo test --no-fail-fast
cargo test --all-targets --all-features --no-fail-fast --quiet
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release --all-features
target/release/ca --help
target/release/ca --version
git diff --check
```

Acceptance requires legacy auto/Codex compatibility; no implicit commit in manual mode; no work while pending; stale approvals are no-ops; task import never rewrites `ca`; typed shell-free verification; exact role-run authority; no engine fallback; pinned MCP repository; real `ca` tool/resource discovery; asynchronous long cycles; descendant cleanup; and no MCP commit without a separate exact approval.

## Idempotence and Recovery

New fields default to legacy behavior. Import preview is mutation-free and repeated identical apply is idempotent. Ignore insertion is marker-based. Failed pending validation preserves Worker files and state. Failed bookkeeping restores only Rust-written bytes and unstages only selected paths. There is no automated reset/clean/discard. MCP job writes are atomic and corrupt records fail closed. Interrupted work remains durable partial-state evidence. Never delete `.orchestrator` automatically.

## Artifacts and Notes

Durable artifacts include task/pending/MCP schemas, `.orchestrator/imports/*.json`, `.orchestrator/mcp/jobs/*.json`, bounded `.ca` run logs, `docs/CA_INTEGRATION.md`, `ca/docs/ROLE_RUN_PROTOCOL.md`, and a repository-contained supervisor skill. They must not contain credentials, resolved headers, full provider requests, or unredacted tool arguments.

## Outcomes & Retrospective

Changed files:

- Orchestrator: implementation is present across configuration/state/receipts,
  centralized Git commits, cancellation-aware process boundaries, typed
  import/revision/verification, restricted `CaRunner`, SDK MCP/jobs, schemas,
  focused/integration tests, supervisor skill, and operator/control docs. These
  paths are included in the owner-authorized clean integration checkpoint after
  the final safety review.
- `ca`: the clean integration clone contains the public role protocol,
  offline preflight, restricted authority and execution preparation,
  credential/output redaction, process cleanup, exact mutation boundary,
  public CLI tests, real MCP-client compatibility test, and harness updates.
  The original dirty checkout, runtime profiles, credentials, sessions,
  installed binaries, `oracles/`, and `reference/` remain untouched.

Commands and results:

- Baseline/worktree safety inspection: PASS.
- Documentation reconciliation: PASS. A doc-scoped `git diff --check` exited
  0; `--no-index --check` on untracked `PLAN.md` and
  `docs/CA_INTEGRATION.md` emitted no whitespace diagnostics (its expected
  exit 1 only reports that the file differs from `/dev/null`).
- Orchestrator focused/completion: PASS — 539 library tests with one documented
  ignore, ten integration tests, strict all-target/all-feature Clippy,
  formatting, all-feature release, help/version, diff, snapshot, and protected
  tree hygiene.
- `ca` focused/completion: PASS — 1,757 main tests with two documented ignores,
  118 experiment tests with one documented ignore, 37 CLI tests, 17 role-run
  CLI tests, strict all-target/all-feature Clippy, formatting, all-feature
  release, help/version, diff, snapshot, and protected-tree hygiene.
- Cross-repository: PASS for the final release `ca role-run` all-role boundary,
  generic stdio SDK coverage, actual `ca::McpManager` observe/operate against
  the final debug binary, and the disposable process-level fake
  Codex/DeepSeek/MiMo global skill-activation pilot.
- CLI/MCP pilots: the release MCP operate gate is environmentally blocked: the
  release orchestrator correctly refuses to start because production macOS
  Seatbelt cannot self-test inside this managed app. The debug-only isolation
  double passes the exact client/server contract. No live-provider pilot was
  authorized or run.

Remaining risks:

- Release MCP operate compatibility must be rerun on a host where production
  verification isolation can self-test before real enrollment.
- The public contract, refreshed matrices, and disposable fake-process
  activation are proven, but operator review still gates production use.
- Live-provider behavior is deliberately unproven because no such request was authorized.
- The receipt schema contract and actual `ca` client boundary are validated;
  external reliance still depends on the production-isolation host check.

Final decision:

- [ ] Accepted for supervised production use.
- [x] Pilot-only pending production-isolation host confirmation and supervised dogfood.
- [ ] Rejected and rolled back.
