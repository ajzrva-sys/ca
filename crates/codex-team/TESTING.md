# TESTING

## PRT-230 Finder metadata exclusion and REL-160 repair (2026-08-29)

- Failure-first protected-tree test: FAIL as expected, four entries versus two.
  After the exact-component v2 filter, protected-tree unit tests pass 6/6 and
  real CLI tests pass 3/3, including nested changes and a similarly named
  protected control.
- Full package passes 652 active tests/1 documented ignore, MCP stdio 8/8, and
  all integrations. The digest-bound v2 manifest apply succeeds without
  touching protected content.
- REL-160's corrected clean-filter invariant passes ten consecutive focused
  runs. The first workspace run records its historical one-versus-two failure;
  both complete replacement serial matrices pass.
- Default workspace matrix: CA 2,371/4 ignored, codex-team 652/1 ignored, MCP
  stdio 8/8. All-target/all-feature: CA 2,400/4 ignored, experiment 118/1
  ignored, codex-team 652/1 ignored, MCP stdio 8/8. All other targets pass.
- Formatting, strict workspace all-target/all-feature Clippy, all-feature
  release, candidate signature, diff/snapshot hygiene, and direct protected
  status/diff pass. The exact manifest-only commit was separately authorized,
  and the HEAD-trusted protected verifier passes.

## TST-217 broker cache-publication collision repair (2026-08-29)

- The four deterministic fixed-clock atomic-publication tests pass, covering
  16 concurrent publishers, complete same-target payloads, a preserved foreign
  first-candidate collision, fail-closed exhaustion of all 16 candidates, and
  cleanup of only writer-owned temporary files.
- `cargo test -p codex-team broker::tests:: --lib -- --test-threads=1`: PASS,
  all 11 broker tests, including private-mode, hardlink, tamper, pruning, and
  cache-size coverage.
- The shared-query stdio regression passed three consecutive focused runs.
  `cargo test -p codex-team mcp:: -- --test-threads=1` passed 36 tests, and
  `cargo test -p codex-team --test mcp_sdk -- --test-threads=1` passed 8/8.
- Both requested serial workspace matrices passed cleanly. The default matrix
  passed CA 2,267/4 ignored and codex-team 651/1 ignored; the all-target,
  all-feature matrix passed CA 2,296/4 ignored, experiment 118/1 ignored, and
  codex-team 651/1 ignored, with every integration target passing in both.
- `cargo fmt --all -- --check`, strict workspace all-target/all-feature Clippy,
  and the workspace all-feature release build: PASS. No runtime/profile state,
  provider request, installation, oracle, or reference mutation occurred.

## TST-216 deterministic shared-evidence cancellation barrier (2026-08-29)

- The repaired
  `five_stdio_clients_share_evidence_cache_and_last_consumer_cancellation`
  case passed once during implementation and three immediate repetitions. Each
  run observed exactly five consumers through `delegation_status` before
  releasing the fixture; the child remained active through four detachments
  and became cancelled only after the last consumer request.
- `cargo test -p codex-team mcp:: -- --test-threads=1`: PASS, 36 MCP unit
  tests.
- `cargo test -p codex-team --test mcp_sdk -- --test-threads=1`: PASS on the
  clean rerun, 8/8. The preceding attempt passed the repaired cancellation case
  but hit the separate existing shared-query cache-publication `EEXIST` race;
  that exact query test passed immediately alone. TST-217 later repaired the
  publication race.
- `cargo test -p codex-team --no-fail-fast -- --test-threads=1`: the 647 active
  library tests passed with one documented ignore, and every integration target
  except the MCP SDK target passed. The repaired cancellation case passed in
  that target; the same unrelated cache-publication collision was its sole
  failure. That historical run is not a clean aggregate pass; TST-217's later
  default and all-feature workspace matrices both passed cleanly.
- `cargo fmt --all -- --check` and strict all-target/all-feature package
  Clippy: PASS. No production source, provider, profile, session, installed
  binary, oracle, or reference path changed.

## INT-194 dependency-aware verification and diagnosis (2026-08-27)

- Shared protocol/schema suite: PASS, six tests including the optional,
  default-false full-escalation policy bit and legacy decoding.
- Mechanistic library filter: PASS, 17 tests covering Rust/Node/Python/Swift
  graph ownership, transitive reverse dependencies, graph bounds/fail-closed
  behavior, package-scoped typed recipes, diagnosis, cache binding, query
  safety, and non-signalling observation cancellation.
- Real stdio SDK: PASS, six tests; observe/delegate/operate remain exactly
  11/15/28 tools with zero resources.
- Root CA focused mechanistic/session/event/evidence/quality-history and atomic
  mutation-batch suites: PASS.
- The one serial workspace run reached every target. Its sole failure was a
  stale CA command-palette count after adding `/evidence`; the corrected exact
  snapshot test passes. Codex-team passed 595 library tests with one
  external-binary ignore plus all 13 integration tests.
- Formatting, strict all-target/all-feature Clippy, all-feature release,
  release help/version, diff/snapshot/protected-tree checks, production
  Seatbelt, and the real release 11/15/28 zero-resource CA fixture: PASS.
  Completion details are recorded in root `TESTING.md` and the completed
  INT-194 ExecPlan.

## INT-193 automatic preflight and governed verification (2026-08-27)

- Shared protocol/schema suite: PASS, six tests.
- Codex-team composite query/evidence suite: PASS, twelve tests covering
  tracked-file enforcement, bounded expansion, changed-file selection,
  affected-input cache invalidation, recipe parity, and non-signalling watcher
  cancellation.
- MCP server and real stdio SDK suites: PASS, 27 plus six tests; public
  catalogs remain exactly 11/15/28 with zero resources.
- Learned delegation lifecycle: PASS, four focused tests; unapproved manifests
  remain inert, executable members are rejected, exact promotion thresholds
  hold, and failures/corrections override positive evidence.
- Root CA focused tests pass for deterministic preflight, request-only bounded
  operands, exact mutation capture, schedule V2 purity, Main-only facade
  selection, event deduplication, and role-run recursion exclusion.
- The one broad serial workspace run reached every target. The codex-team
  library's one real schema regression was fixed and its full failed target
  then passed 590 tests with one external-binary ignore; all 13 integration
  and six protocol tests passed. The unrelated clean-filter failure did not
  reproduce in isolation or in the full failed-target rerun.
- Formatting, strict workspace Clippy, all-feature release, snapshot/protected
  hygiene, production Seatbelt, release/installed 11/15/28 compatibility, and
  reverse release-CA compatibility: PASS. The 14 focused release-readiness
  tests confirm dirty/untracked and plan bookkeeping are informational while
  real contract/gate/source-stability failures remain blocking.
- Installed hashes are
  `ca=e68c2bfb434c3c9ad936b12f3e523666cd1000e35a7bca9d2dabf418197df870`
  and
  `codex-team=841eff1f53dd98372d014e0f42416ec9f00503f30427d3bf856a2a5432cb881a`.
  Installed `/delegate` reports 15 tools and zero resources; `/jobs` is empty;
  both ran without provider traffic. Exact details and backups are in root
  `TESTING.md`.

## INT-191 protected-tree baseline and activation (2026-08-26)

- Protected-tree scanner/manifest/preview/apply focused tests: PASS, including
  HEAD-only trust, exact digest/revision binding, drift, symlinks, hardlinks,
  specials, bounds, races, and zero protected-root mutation.
- Manifest-only commit: PASS; commit
  `1530d660916ecb8d6f5befa68d1a850e9912c674` adds only
  `.codex-team-protected-trees.json` and preserved the unrelated index and
  worktree digests.
- Real protected-tree receipt and direct protected Git status/diff: PASS;
  `oracles/` and `reference/` are unchanged.
- Root serial workspace matrix: PASS; codex-team contributes 585 unit tests
  with one documented external-binary ignore and 13 integration tests.
- Formatting, strict all-target/all-feature Clippy, optimized all-feature
  workspace build, diff/snapshot hygiene, release help/version: PASS.
- Real CA MCP fixture: PASS for exact 11/15/28 catalogs and zero resources.
  Reverse release-CA role-run fixture: PASS.
- Production Seatbelt isolation: PASS without debug override.
- Publication release readiness retains dirty/untracked source diagnostics but
  no longer vetoes local activation. Exact validated artifact hashes,
  protected-tree verification, recoverable backups, and installed
  compatibility were used instead.
- Installed hashes match release artifacts:
  `ca=b966d7fd02a5f97a58f0a42ec99b08d98ec8dbd1a4d3054bdf7786810b1a98f9`,
  `codex-team=770a000fb296be68732278cc06fa1e3157491de7bf24e5396bd6bf07f6a502d9`.
- Named-profile doctor starts no MCP process. The installed TUI `/delegate`
  receipt reports 15 tools and zero resources; `/jobs` renders the unified
  activity view; exit leaves no server process. No provider call occurred.
- Focused installed-executable sandbox and repository-only uninitialized
  status regressions pass, as do strict package Clippy checks.
- The automatic-selection contract test passes: eligible mechanical requests
  prefer codex-team without naming the tool and may fall back only when the
  receipt explicitly allows it.
- Action-only facade and hidden host-owned version regressions pass. The real
  installed-codex-team compatibility fixture now invokes the facade without a
  model-supplied schema and still passes exact 11/15/28 catalogs and query flow.

## INT-190 CA reciprocal delegation completion (2026-08-26)

- Shared protocol round trips and generated schemas: PASS, three tests.
- Exact 11/15/28 MCP catalog, zero-resource, durable job, bounded repository
  query, and non-signalling observation tests: PASS.
- Combined pre-Worker integration: PASS; query context reaches the CA-backed
  Worker, admitted Rust-only evidence makes zero CA calls, and edit work makes
  exactly one CA call.
- Root serial workspace matrix: PASS; codex-team contributes 580 library tests
  with one documented external-binary ignore and all eleven integration tests.
- Strict workspace all-target/all-feature Clippy, formatting, and all-feature
  release build: PASS.
- Explicit `CODEX_TEAM_BIN=target/release/codex-team` root CA compatibility
  fixture: PASS for exact observe/delegate/operate catalogs and the facade
  query.
- Explicit `CA_BIN=target/release/ca` compatibility fixture: PASS for offline
  role preflight and exact allowed/denied Worker writes.
- Release doctor production isolation subcheck: PASS,
  `macos-seatbelt-source-projection-v1`, without
  `CODEX_TEAM_TEST_ISOLATION_READY`. The overall doctor exit one is expected
  because the monorepo root is intentionally not initialized as a governed
  target.
- Direct protected-tree Git inspection reports no changes. The built-in check
  remains fail-closed because ignored `oracles/` and `reference/` have no
  trusted tracked-content baseline; snapshot hygiene and `git diff --check`
  pass.

## ORCH-DELEGATION-FABRIC-003 focused checkpoint (2026-08-26)

The versioned read/observe/verify contract, exact MCP tiers, internal task
policy routing, and inert learned-manifest lifecycle are implemented. Focused
checks currently pass:

- `cargo test -p codex-team mechanistic:: --lib -- --test-threads=1`: PASS —
  seven tests covering all typed evidence recipes, query bounds/redaction,
  traversal, private/secret/binary/oversized inputs, symlinks/hardlinks, target
  immutability, and non-signalling observation cancellation.
- Learned delegation/artifact focused suites: PASS — seven tests covering
  inert/namespaced manifests, prompt exclusion, canonical digests, shadow
  thresholds, exact approval, drift/ambiguity, quarantine, retirement, and
  human correction.
- Exact MCP observe/delegate/operate catalog tests and six real stdio SDK tests:
  PASS. The catalogs contain 11, 15, and 28 tools respectively and advertise
  zero resources.
- Typed query-context injection, eligible Worker skip, edit-task non-bypass,
  and `prefer`/`require` fallback tests: PASS.

Final results:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS. The root
  app passed 2,059 library tests with two documented ignores plus 38 CLI and 17
  role-run tests; `ca-watch` passed 15 library and three CLI tests;
  `codex-team` passed 579 library tests with one documented external-binary
  ignore plus all eleven integration tests.
- `cargo fmt --all -- --check`, strict workspace all-target/all-feature Clippy,
  and `cargo build --workspace --release --all-features`: PASS.
- Release `codex-team --help`/`--version`, all new schema parses,
  `git diff --check`, the real release-`ca` preflight/Worker compatibility test,
  and snapshot hygiene: PASS.
- The real production Seatbelt subcheck in release `doctor` is PASS:
  `macos-seatbelt-source-projection-v1`. No test isolation override was used.
- The ignored root `ca::McpManager` compatibility fixture discovers the new
  eleven-tool observe catalog, then FAILS its stale hard-coded ten-tool
  assertion. `src/mcp.rs` contains unrelated pre-existing user edits, so the
  plan's overlap guard leaves it untouched. The six real stdio SDK tests cover
  the exact 11/15/28 catalogs and zero resources.
- The built-in protected-tree check FAILS CLOSED because ignored `oracles/` and
  `reference/` roots have no trusted tracked-content baseline. The exact Git
  diff from baseline `c1513f5` names no protected paths, and release readiness
  confirms they are absent from tracked release source.
- Release readiness is NOT READY as expected for this intentionally dirty,
  uncommitted workspace: active-plan consistency, working-tree cleanliness, and
  tracked release-control checks block it. No cleanup, commit, or bypass was
  attempted.

## MCP restart fixture orphan cleanup (2026-08-22)

Four orphan processes from an August 20 failing restart fixture were stopped.
The test harness now owns every simulated orphan through a drop guard that
signals the exact process group and waits for the direct child on every early
exit path.

- `cargo test restart_fixture_reaper_kills_and_waits_when_a_test_exits_early -- --nocapture`: PASS.
- `cargo test restart_terminates_and_records_a_still_alive_orphaned_step -- --nocapture`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --all-targets --all-features --no-fail-fast --quiet`: PASS — 540
  library tests with one documented ignore, plus ten integration tests.
- Post-focused and post-matrix process scans for `yes` and temporary
  `codex-team` MCP/learning/verification children: empty.

## ORCH-SELF-IMPROVE-002 autonomous artifact loop (2026-08-22)

The enrolled-profile artifact loop is implementation- and fake-pilot complete,
disabled by default, and still requires an isolation-capable host check before
real enrollment.

- `cargo test self_improvement --lib -- --nocapture`: PASS — 30/30 focused
  tests before the final process pilot was added.
- Process-level fake Codex/DeepSeek/MiMo pilot: PASS. An exact enrolled intent
  was exported and acknowledged through a child `ca` process, replayed across
  all three contained routes, and promoted as one immutable skill; both source
  HEADs and deployment records stayed unchanged.
- `cargo test --all-targets --all-features --no-fail-fast --quiet`: PASS — 539
  library tests with one documented ignore, plus 1 import foundation, 1 manual
  commit CLI, 5 MCP SDK, and 3 step-cycle tests.
- `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo fmt --all -- --check`, and `cargo build --release --all-features`:
  PASS.
- Release help/version, `git diff --check`, snapshot hygiene, and protected
  trees: PASS. Release SHA-256 is
  `06fbd6864dcf73d8d14ae3a96b1604b187443c27db1d1985f9f1d02bfdf97cb9`;
  supervisor SHA-256 is
  `3b4043d184bcd9ee635e7641587d14761e98d93e39aaacece287fdcc07cd18dd`.
- Final release orchestrator to final release `ca role-run`: PASS for all-role
  loopback compatibility.
- Actual `ca::McpManager` observe/operate compatibility: PASS against the final
  debug binary with the debug-only isolation double. The release server fails
  closed before MCP initialization because macOS Seatbelt cannot self-test from
  this managed app environment. No release bypass was added; rerun on an
  isolation-capable host before real enrollment.

No provider credential, real profile/session, installed binary, service,
source transition, dependency change, commit, deployment, oracle, or reference
tree was used or changed.

## ORCH-LEARN-COMPILER-001 final automated matrix (2026-08-21)

The controlled learning implementation is automated-gate complete and remains
pilot-only pending one clean global-activation pilot with the production
isolation backend. Coverage includes schema/digest tampering, hardened global
storage, legacy observation migration, first-success proposal creation, exact
approval, structural matching and ambiguity, drift deactivation, local
quarantine, two-repository retirement, cancellation, output/privacy bounds,
crash recovery, zero-AI execution, CLI/MCP contracts, and shared-projection
build-to-release command chains.

- `cargo test --no-fail-fast`: PASS — 508 library tests with one documented
  ignore, plus 1 import foundation, 1 manual commit CLI, 5 MCP SDK, and 3
  step-cycle integration tests.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- `git diff --check`, `.snap.new`, and protected-tree checks: PASS.
- Real orchestrator-to-`ca` role/preflight compatibility: PASS against the
  clean final debug binary, covering all four roles and allowed/denied Worker
  writes.
- Real `ca::McpManager`-to-orchestrator observe/operate compatibility: PASS,
  including exact recipe list/preview/mutation catalogs with no model tool
  activation.

The test-only `CODEX_TEAM_TEST_ISOLATION_READY=1` path is debug-only and never
counts as a supervised pilot. No live provider, user credential/profile,
installed binary, or protected tree was used.

## ORCH-CA-001 pre-final-hardening matrix

The pre-final-hardening ORCH-CA-001 completion matrix passed on 2026-08-20.
Focused behaviors below were covered by that library/integration suite; rows
say explicitly when the evidence came from that run rather than a separately
invoked filter. Commit-recovery, task-import publication, run-artifact, and MCP
restart fixes landed afterward, so the aggregate matrix was pending at this
historical checkpoint; the final results above supersede it.
Hermetic real-binary role and actual `ca` MCP-client compatibility are recorded
separately and do not substitute for supervised pilots.

The two real-binary commands below were recorded before the 2026-08-22
monorepo consolidation. Their current binary paths, after a workspace build,
are `/Users/aji/project/ca/target/debug/ca` and
`/Users/aji/project/ca/target/debug/codex-team`.

| Command | Current status | What it must prove |
|---|---|---|
| `cargo test config::tests` | Covered by passing 465-test library suite | Legacy serde defaults, disabled `ca` fail-closed behavior, reserved-profile rejection, required Worker guard, and rejection of custom ignored paths for `ca` Workers |
| `cargo test git::tests` | Covered by passing 465-test library suite | Every characterized legacy auto-commit lane; manual intent path/digest/fingerprint binding; stale rejection; exact approval; deferred files/recovery evidence; imported-task completion; idempotent runtime exclude |
| `cargo test task_import` | Covered by passing 465-test library suite | Strict schema/bounds/path validation, case-insensitive protected-tree rejection, canonical digest, preview/apply/idempotency/conflict behavior, and immutable receipt handling |
| `cargo test revision` | Covered by passing 465-test library suite | Dirty worktree bytes, staged blob identities/modes, symlink and size fail-closed cases, partial-state latch, volatile MCP observation exclusion, and pause-fingerprint staleness |
| `cargo test verification::ca` | Covered by passing 465-test library suite | Acceptance/normalization of the exact shell-free `ca` recipes and rejection of shells, authority-widening flags, network commands, snapshot residue, and protected-tree changes |
| `cargo test --test ca_integration_foundations` | Passed: 1 test in full run | Cross-process digest-bound import with unchanged HEAD/index, stale rejection, idempotency, and Rust-owned hygiene commands |
| `cargo test --test manual_commit_cli` | Passed: 1 test in full run | Public-CLI manual cycle pauses without staging/committing, stale approval is mutation-free, exact approval creates one commit, and replay fails |
| `cargo test ca::run::tests` | Covered by passing 465-test library suite | Fixed tools/grants, symlink/case/ignored-root rejection, JSONL contract, sanitized artifacts, exact argv/stdin, bounded preflight, and child/grandchild cleanup on cancellation, timeout, and normal direct-child exit |
| `CA_BIN=/Users/aji/project/ca/target/debug/ca cargo test ca::run::tests::real_ca_binary_preflight_read_only_and_worker_compatibility -- --ignored --exact --nocapture` | Passed before consolidation: 1 ignored-by-default test | Actual `ca` offline preflight; CEO, Project Manager, Senior Dev, and Worker schemas; one exact-grant Worker write; one denied outside-scope write with unchanged canary; accepted/redacted JSONL; fixed tool catalogs; and zero profile/session persistence through `RealCaRunner`; used a disposable HOME, empty child environment, loopback provider, and test-only credential |
| `CODEX_TEAM_BIN=/Users/aji/project/ca/target/debug/codex-team cargo test mcp::tests::real_codex_team_mcp_manager_observe_and_operate_compatibility -- --ignored --nocapture` (run from `crates/codex-team`) | Passed before consolidation: 1 ignored-by-default test | Actual `ca::McpManager` discovered the exact observe/operate catalogs and zero resources, read status, previewed/applied one strict task, rejected a stale apply without mutation, and polled one credentialless asynchronous no-work step with unchanged HEAD/index |
| `cargo test cancellation` | Covered by passing 465-test library suite | Shared cancellation-token behavior used by role and verification subprocesses |
| `cargo test codex` | Covered by passing 465-test library suite | Bounded/sanitized provider failures plus descendant reaping after cancellation, timeout, and a successful direct-child exit with inherited pipe holders |
| `cargo test mcp::` | Covered by passing 465-test library suite | Exact catalogs, pinned/symlink-safe Git root, structured errors, durable jobs, actual nested process-group hard fallback, partial-state approval latch, stale-state checks, shutdown, restart recovery, and retention |
| `cargo test --test mcp_sdk` | Passed: 5 tests in full run | Real stdio SDK negotiation, observe/operate discovery, stale disposition/import rejection, credentialless asynchronous no-work step, and noncommitting task import |
| `cargo test manual_` | Covered by passing 465-test library suite | A pending CEO, Project Manager, Senior Dev, Worker, deterministic, or Rust-authored intent terminates that cycle before later role or bookkeeping work |
| `cargo test no_work_cycle_reports_paused_when_control_docs_created_pending_intent` | Passed within 465-test library suite | A no-work cycle that creates a manual control-document intent reports `paused`, never `no_work` |
| `cargo test codex_doctor_commands_are_bounded_and_receipt_redacts_child_output` | Passed within 465-test library suite | Codex version/model probes time out, reap descendants, bound output, and do not expose child canaries in doctor receipts |
| `cargo test markdown_queue_sync_retains_digest_bound_imported_task` | Covered by passing 465-test library suite | Markdown reconciliation cannot remove an imported state-owned task |
| `cargo test imported_worker_acceptance_leaves_next_steps_byte_identical` | Covered by passing 465-test library suite | Imported task acceptance never rewrites the target `NEXT_STEPS.md` |
| Receipt schema contract test for cycle/status/disposition additive fields | Passed within 465-test library suite | Strict schemas with `additionalProperties: false` accept `commit_mode`, `require_clean_start`, `revision`, `pause_fingerprint`, and pending intent summaries emitted by the DTOs |
| `cargo test --no-fail-fast` | Passed before final transaction/recovery fixes; rerun pending | 465 library tests, 1 `ca_integration_foundations`, 1 `manual_commit_cli`, 5 `mcp_sdk`, 3 `step_cycle`, and 0 doctests passed (475 total tests) at the checkpoint |
| `cargo fmt --all -- --check` | Passed at the checkpoint; rerun pending | Rust formatting across the workspace |
| `cargo clippy --all-targets -- -D warnings` | Passed at the checkpoint; rerun pending | Strict lint completion gate |
| `cargo build --release` | Passed at the checkpoint; rerun pending | Optimized orchestrator build |
| `target/release/codex-team --help` | Passed | Built CLI command surface renders successfully |
| `target/release/codex-team --version` | Passed: `0.1.0` | Built CLI version renders successfully |
| `git diff --check` | Passed | Whitespace integrity across the integration diff |
| `target/release/codex-team verify snapshot-hygiene` | Passed: no `.snap.new` files | Built-in snapshot hygiene gate |
| `target/release/codex-team verify protected-trees` | Passed: unchanged | Built-in protected-tree gate |

Not completed by the recorded evidence:

- Final post-fix aggregate matrices for both integration branches. The latest
  pre-final-safety `ca` default suite passed 1,692 main tests, 37 ordinary CLI
  tests, and 10 role CLI tests, but credential-redaction and hardlink-safe
  mutation fixes landed afterward. The post-fix focused checkpoint passes 38
  role-run unit tests and 13 role CLI tests.
- The separately supervised one-step CLI and MCP dogfood sequences. The rows
  above are hermetic compatibility tests, not operator acceptance.
- Live-provider execution. It requires separate authorization and has not run.

Documentation reconciliation commands run on 2026-08-20:

- `git diff --check -- README.md ARCHITECTURE.md CONTINUITY.md TESTING.md ROADMAP.md docs/.INDEX.md docs/AGENT_MAP.md docs/ACCOMPLISHMENTS.md docs/COMPLETED.md docs/HERMES.md docs/HERMES_IMPLEMENTATION_GUIDE.md docs/HERMES_INTEGRATION_CHECKLIST.md`: passed with no output.
- `git diff --no-index --check /dev/null PLAN.md`: no whitespace diagnostics;
  exit `1` is expected because the untracked file differs from `/dev/null`.
- `git diff --no-index --check /dev/null docs/CA_INTEGRATION.md`: no
  whitespace diagnostics; exit `1` is expected because the untracked file
  differs from `/dev/null`.
- Local-link target audit with `test -e` for the touched docs' `PLAN.md`,
  integration/Hermes/operations/agent-map documents, machine catalog, and
  supervisor scripts: passed with no missing path.

## Historical verified commands

| Command | Status | What it validates | Evidence |
|---|---|---|---|
| `cargo fmt -- --check` | Verified during ORCH-INTEGRITY-001 | Rust formatting | Command exited 0 |
| `cargo test` | Verified during ORCH-INTEGRITY-001 | Full Rust unit/doc test suite | 262 lib unit tests passed |
| `cargo test role_schemas_require` | Verified during ORCH-INTEGRITY-001 | Recursive strict property=required for all role/task schemas | 1 test passed |
| `cargo test lease` | Verified during ORCH-INTEGRITY-001 / CHURN-003 | File lease expansion caps and repo-hint TTL | Context lease module tests passed |
| `cargo build --release` | Verified during ORCH-ROUTER-001 route cost controls | Optimized local binary build | Release profile finished successfully for `codex-team` |
| `target/release/codex-team doctor --repo /Users/andrewzimmerman/project/codex_team_orchestrator` | Verified as an environment smoke test | Local Codex binary, Git discovery, control-doc presence, and default model availability | Exited 0; found `codex-cli 0.140.0` and default role models |
| `cargo test verification` | Verified during docs-only verification hint suppression | Verification routing, evidence, retry, report-root policy, observation-hint scoping, and runner-adjacent behavior | 16 tests passed |
| `cargo test codex` | Verified after Codex adapter extraction | Codex runner argument contract, JSONL parser helpers, and existing Codex-log import/live helper tests | 9 tests passed |
| `cargo test parse_json_response` | Verified after Codex adapter extraction | Strict structured role-output parsing fallback behavior | 4 tests passed |
| `cargo test paths` | Verified during Helicon readiness guardrails | Path root canonicalization, sibling-prefix distinction, and uppercase `Docs/` control-doc recognition | 3 tests passed |
| `cargo test dirty_tree` | Verified during ORCH-HEL-005 | Dirty-tree continuation, preserve suppression, pre-Senior policy ordering, and large adoption dirty-set pause behavior | 6 tests passed |
| `cargo test human_disposition` | Verified during Helicon legacy report refresh | Interactive/non-interactive human-disposition parsing, report refresh, and state transitions | 7 tests passed |
| `cargo test hermes_` | Verified during ORCH-HERMES-003 | Hermes receipts including error outcome, hydrate parse, observe status | 9 tests passed |
| `cargo test ollama` | Verified during ORCH-OLLAMA-001 | Ollama request/response parsing, disabled fail-closed, health skip, role dispatch | 9 tests passed |
| `cargo test mlx_lm` | Verified during MLX LM provider linkage | MLX LM request body, disabled fail-closed, health skip, role dispatch | 6 tests passed |
| `cargo test dirty_adoption_report` | Verified during Helicon legacy report refresh | Dirty adoption report ownership classification for init scaffolding and generated roots | 1 test passed |
| `cargo test route_regret` | Verified during ORCH-ROUTER-001 route cost controls | Route-regret classification, redundant Worker read/search detection, task-plus-fingerprint dedupe, and lane-savings accounting | 6 tests passed |
| `cargo test efficiency_watch` | Verified during ORCH-LESSONS-001 others reliability/routing lessons | Watcher detection of existing efficiency alerts plus tool-health cooldown, derived-status, and route-regret reporting | 12 tests passed |
| `cargo test tool_health` | Verified during ORCH-LESSONS-001 others reliability/routing lessons | Tool failure classification, advisory cooldown aggregation, and watcher cooldown alerting | 3 tests passed |
| `cargo test route_learning` | Verified during ORCH-LESSONS-001 others reliability/routing lessons | Route scorecard scoring, stable tie behavior, and positive route-learning duplicate suppression | 3 tests passed |
| `cargo test derived_status` | Verified during ORCH-LESSONS-001 others reliability/routing lessons | Derived status priority and queue-to-worker status derivation | 2 tests passed |
| `cargo test acceptance_contract` | Verified during ORCH-LESSONS-001 others reliability/routing lessons | Acceptance contract derivation, characterization requirement checks, and pre-Worker risky-refactor blocking | 4 tests passed |
| `cargo test route_bench` | Verified during ORCH-LESSONS-001 others reliability/routing lessons | Local route benchmark report generation without role calls | 1 test passed |
| `cargo test self_heal` | Verified during watcher self-heal implementation | Watch self-heal switches full planner context to compact and persists stale verification-hint suppressions | 2 tests passed |
| `cargo test persisted_suppressed_observation_hint` | Verified during watcher self-heal implementation | Persisted stale-hint suppressions are honored by verification planning | 1 test passed |
| `cargo test recent_failed_observation_hint` | Verified during stale verification hint suppression | Failed local/global observation-memory hints are skipped from later verification plans | 1 test passed |
| `cargo test prompt_stays` | Verified during compact role prompt budget tightening | Compact Project Manager and Worker prompts stay bounded with oversized control docs | 2 tests passed |
| `cargo test parity_preflight` | Verified during Helicon readiness guardrails | Renderer/campaign preflight gate behavior | 3 tests passed |
| `cargo test queue::next_steps` | Verified during Helicon adoption readiness | Extracted `NEXT_STEPS` parser contract | 10 tests passed |
| `cargo test worker_schema_requires_every_strict_object_property` | Verified during Worker schema strictness fix | Worker response schema compatibility with strict response-format required/properties rules | 1 test passed |
| `cargo test worker` | Verified during Worker schema strictness fix | Worker schema, handoff, retry, routing, and queue-adjacent behavior | 28 tests passed |
| `cargo test verification_plan_skips_observation_hints_for_docs_only_task` | Verified during docs-only verification hint suppression | Docs/tracking-doc tasks do not inherit broad observation verification hints as acceptance commands | 1 test passed |
| `cargo test deterministic_recipe_excludes_worker_diagnostic_command_failures` | Verified during recipe diagnostic command suppression | Worker diagnostic commands and failed Rust verification items are not harvested into deterministic recipes | 1 test passed |
| `cargo test recipe` | Verified during recipe diagnostic command suppression | Recipe parsing, harvesting, action-cache, and promotion behavior | 6 tests passed |
| `cargo test legacy_intern_support_contract` | Verified during verification runner extraction | Legacy Intern config compatibility and Rust support packet contracts | 5 tests passed |
| `target/release/codex-team disposition --help` | Verified during Helicon legacy report refresh | Non-interactive disposition CLI surface, decision values, and refresh flag | Listed `preserve`, `recheck`, `gate`, `keep-paused`, and `--refresh-report` |
| `cargo test mission_command` | Verified during mission setter implementation | Rust-owned mission update command, roadmap mission text rewrite, and blank mission rejection | 3 tests passed |
| `cargo test planner_work_plan` | Verified during PM work-plan persistence | PM structured work plan is persisted as Rust-owned observation markdown/JSON without extra PM cycles | 1 test passed |
| `cargo test models_command` | Verified during model/speed config CLI implementation | Role model, reasoning effort, and timeout config mutation plus validation errors | 3 tests passed |
| `cargo test planner_task_scope` | Verified during PM control-doc scope normalization | PM-mentioned control docs and active ExecPlans are added to docs/evidence task scopes without broadening plain source tasks | 2 tests passed |
| `cargo test command_only` | Verified during mechanistic handoff implementation | Command-only evidence tasks run in Rust before Worker, and unsafe command-only tasks stop before Worker | 2 tests passed |
| `cargo test mechanistic` | Verified during mechanistic handoff implementation | Watcher self-heal writes mechanistic handoff observations and external feedback asks for recipe/router repair | 2 tests passed |

## Repo-Derived Commands Needing Current Verification

| Command | Status | Evidence | Verification needed |
|---|---|---|---|
| `codex-team doctor --repo /path/to/project` | Inferred, not run in this harness pass | `README.md` and `src/cli.rs` | Run only when a configured target repo and Codex CLI are available |
| `codex-team commands --repo /path/to/project` | Inferred, not run in this harness pass | `README.md` and `src/cli.rs` | Run only against a disposable or intended target repo |

## Minimum Validation

- Docs-only changes: manually check paths/links and record commands run; no Rust tests required unless docs mention generated code behavior.
- Queue parser changes: `cargo test queue::next_steps` and `cargo test`.
- Support packet changes: `cargo test legacy_intern_support_contract` and `cargo test`.
- Routing and route-regret telemetry changes: `cargo test route_regret`, `cargo test efficiency_watch`, then `cargo test`.
- Verification command handling: `cargo test verification`, `cargo test legacy_intern_support_contract`, then `cargo test`.
- Codex adapter changes: `cargo test step_once_routes_planner_through_fake_codex_runner`, `cargo test codex`, then `cargo test`.
- Commit mode or staging changes: `cargo test git::tests`, the two imported-task queue/acceptance tests, then `cargo test`.
- Task import or revision changes: `cargo test task_import`, `cargo test revision`, `cargo test --test ca_integration_foundations`, then `cargo test`.
- Restricted `ca` adapter changes: `cargo test ca::run::tests`, `cargo test cancellation`, then `cargo test`. Also rerun the ignored real-binary gate with the clean target build through `CA_BIN`; this remains loopback evidence rather than a live-provider pilot.
- MCP changes: `cargo test mcp::`, `cargo test --test mcp_sdk`, then `cargo test`. Rerun the separate actual-`ca` `McpManager` compatibility gate against the final binary; its current checkpoint passes.
- Structured role-output parsing changes: `cargo test parse_json_response`, then `cargo test`.
- Structured Worker schema changes: `cargo test worker_schema_requires_every_strict_object_property`, `cargo test worker`, then `cargo test`.
- Path root construction changes: `cargo test paths`, then `cargo test`.
- Human-disposition or dirty-adoption reporting changes: `cargo test human_disposition`, `cargo test dirty_adoption_report`, `cargo test dirty_tree`, then `cargo test`.
- Watcher/efficiency telemetry changes: `cargo test efficiency_watch`, then `cargo test`.
- Release/install claims: `cargo build --release`; install only with explicit approval.

## Focused Test Strategy

- Use one Cargo test filter per invocation while debugging.
- After focused tests pass, run `cargo test` before claiming source behavior is safe.
- Do not rewrite tests to approve new behavior until the intended contract is documented.

## When Tests Fail

- Record the exact command and failure summary.
- Classify failure as compile, test assertion, environment, timeout, schema/serialization, or policy/safety when evidence supports it.
- Prefer fixing the smallest owning subsystem.
- After two failed patch/verification iterations, stop editing and remap the call path.

## Coverage Gaps

- CI exists under `.github/workflows/`; the local ORCH-CA-001 completion matrix above is recorded as passing for this worktree.
- The actual `ca` binary passes the hermetic `RealCaRunner` all-role/Worker loopback gate, and actual `ca::McpManager` observe/operate compatibility passes. These remain distinct from supervised dogfood and live-provider evidence.
- The clean supervised CLI pilot, observe/operate MCP pilot, live-provider run, and real-profile checks are not recorded as complete. Live traffic and real-profile mutation require separate authorization.
- Watch mode now emits advisory efficiency reports and has a small opt-in self-heal lane for compact planner config and stale verification-hint suppressions; deterministic rerun-loop validation remains future work.

## Commands Run During Senior Foreman Progress And Rust Evidence Jobs
- `cargo test foreman_progress`: validated Senior progress action schema, Rust evidence receipt execution, and one-worker-task queueing; 3 tests passed.
- `cargo test pm_control_doc`: validated existing PM control-doc auto-commit fallback behavior; 3 tests passed.
- `cargo test pm_mutated`: validated PM-mutated control-doc zero-task auto-commit behavior; 1 test passed.
- `cargo test pm_source_edits`: validated PM source edits are flagged and stop before Worker; 1 test passed.
- `cargo test approved_backlog_skips_pm`: validated approved `NEXT_STEPS.md` work bypasses stale `planner_due`; 1 test passed.
- `cargo test efficiency_watch`: validated PM edit, PM/Worker doc-bounce, deterministic-command-in-Worker, prompt-bloat, and stale-hint alerts; 9 tests passed.
- `cargo test verification`: validated verification routing and SwiftStacker report-root policy after evidence-job reuse; 22 tests passed.
- `cargo fmt -- --check`: validated Rust formatting; passed with no output.
- `cargo test`: validated full Rust suite; 182 unit tests passed, plus zero-test main/doc targets.
- `cargo build --release`: rebuilt the optimized `target/release/codex-team` binary; succeeded.

## Commands Run During Role-Authorized Rust Commits
- `cargo test rust_commit`: validated Senior Rust commit jobs, selected allowed path commits, verification-failure rejection, and direct `git commit` command rejection; 4 tests passed.
- `cargo test pm_commit_request`: validated PM typed commit requests commit selected files and reject failed verification without creating a commit; 2 tests passed.
- `cargo test foreman_progress`: validated Senior progress schema and Rust progress action routing, including `rust_commit_job`; 6 tests passed.
- `cargo test planner_work_plan`: validated PM planner schema/output still persists structured work plans with `commit_requests`; 1 test passed.
- `cargo test watch_loop`: validated watcher behavior remains intact around non-interactive loops and stop classifications; 12 tests passed.
- `cargo test pm_control_doc`: validated existing PM control-doc auto-commit fallback behavior after adding the new commit lane; 3 tests passed.
- `cargo test pm_mutated`: validated PM-mutated control-doc zero-task auto-commit behavior after adding the new commit lane; 1 test passed.
- `cargo test step_once_routes_planner_through_fake_codex_runner`: validated planner fallback stops cleanly after a no-work PM response while still using the fake Codex runner; 1 test passed.
- `cargo test helicon_adoption_dry_run_pauses_before_role_calls`: validated dirty adoption still pauses before role calls; 1 test passed.
- `cargo fmt -- --check`: validated Rust formatting; passed with no output.
- `cargo test`: validated full Rust suite; 241 unit tests passed, plus zero-test main/doc targets.
- `git diff --check`: validated no whitespace errors; passed.
- `cargo build --release`: rebuilt the optimized `target/release/codex-team` binary after the attached watch-loop process finished; passed.

## Commands Run During Watch-Loop Orchestrator Auto-Gate Repair
- `cargo test watch_loop`: validated target/orchestrator watch-loop behavior, including PM-created safety-gate auto-disposition for orchestrator repair cycles and dirty-adoption pause refusal; 13 tests passed.
- `cargo fmt -- --check`: initially reported a line-wrap formatting diff; passed after `cargo fmt`.
- `cargo test`: validated full Rust suite; 242 unit tests passed, plus zero-test main/doc targets.
- `git diff --check`: validated no whitespace errors; passed.
- `cargo build --release`: rebuilt the optimized `target/release/codex-team` binary; passed.

## Commands Run During Token-Churn Context Pack Receipts
- `cargo test context_pack`: validated pure context-pack section capping and hard-budget rejection; 2 tests passed.
- `cargo test prompt_receipt`: validated Worker prompt receipts record all named sections plus token/time estimates; 1 test passed.
- `cargo test worker_prompt`: validated compact Worker prompt behavior and over-budget rejection before model launch; 3 tests passed.
- `cargo test attempt_capsule`: validated repeated failure fingerprints omit duplicate full logs/diffs in retry context; 1 test passed.
- `cargo fmt -- --check`: validated Rust formatting after `cargo fmt`; passed with no output.
- `cargo test`: validated full Rust suite; 187 unit tests passed, plus zero-test main/doc targets.
- `cargo build --release`: rebuilt the optimized `target/release/codex-team` binary; succeeded.

## Commands Run During Supervised Efficiency Watch Loop
- `cargo test watch_loop`: validated non-interactive pause stop, cross-repo efficiency feedback packet persistence, missing orchestrator-repo initialization, blocked orchestrator-repair stop classification, Planner-due handoff, and loop report rendering; 5 tests passed.
- `cargo test efficiency_watch`: validated existing watcher alerts and Rust-owned self-heal behavior remain intact; 9 tests passed.
- `cargo fmt -- --check`: validated Rust formatting after `cargo fmt`; passed with no output.
- `cargo test`: validated full Rust suite; 192 unit tests passed, plus zero-test main/doc targets.
- `cargo build --release`: rebuilt the optimized `target/release/codex-team` binary; succeeded.

## Commands Run During Watcher-Driven Mechanistic Handoff
- `cargo test efficiency_watch`: validated watcher alerts still detect Worker-run deterministic commands and now include structured handoff metadata; 9 tests passed.
- `cargo test foreman_progress`: validated existing Senior progress Rust evidence execution still persists receipts; 3 tests passed.
- `cargo test verification`: validated typed recipe parsing and SwiftStacker report-root guards; 22 tests passed.
- `cargo test mechanistic`: validated self-heal handoff observation persistence and external recipe/router repair feedback; 2 tests passed.
- `cargo test command_only`: validated command-only evidence tasks run in Rust before Worker and unsafe command-only tasks stop before Worker; 2 tests passed.
- `cargo test source_edit_task_with_validation_commands_still_calls_worker`: validated edit tasks with validation commands still launch Worker; passed.
- `cargo test watch_loop`: validated external efficiency feedback and bounded loop behavior after mechanistic handoff payload changes; 5 tests passed.
- `cargo fmt -- --check`: initially reported formatting drift, then passed after `cargo fmt`.
- `cargo test`: validated full Rust suite; 206 unit tests passed, plus zero-test main/doc targets.
- `cargo build --release`: rebuilt the optimized `target/release/codex-team` binary; succeeded.

## Commands Run During Route Attempt Memory And Active Regret Routing
- `cargo test route_attempt`: validated route lifecycle/domain tests and route attempt/outcome persistence; 2 tests passed.
- `cargo test route_regret`: validated active regret classification, watcher alerting, and Rust evidence substitution accounting; 4 tests passed.
- `cargo test efficiency_watch`: validated watcher reports include route-regret alerts without regressing existing efficiency alerts; 10 tests passed.
- `cargo test foreman_progress`: validated Senior progress Rust evidence jobs still persist receipts and route through the existing evidence path; 3 tests passed.
- `cargo test command_only`: validated command-only evidence tasks still run in Rust before Worker and unsafe command-only tasks stop before Worker; 3 tests passed.
- `cargo test worker`: validated Worker routing, including repeated-fingerprint routing to Senior before another full Worker retry; 47 tests passed.
- `cargo fmt -- --check`: initially reported formatting drift, then passed after `cargo fmt`.
- `cargo test`: validated full Rust suite; 213 unit tests passed, plus zero-test main/doc targets.
- `cargo build --release`: rebuilt the optimized `target/release/codex-team` binary; succeeded.
- `git diff --check`: validated no whitespace errors; passed.

## Commands Run During ORCH-ROUTER-001 Route Cost Controls
- `cargo test route_regret`: validated redundant Worker read/search detection, task-plus-fingerprint dedupe, route-regret reporting, and lane-savings aggregation; 6 tests passed.
- `cargo test efficiency_watch`: validated watcher reporting records redundant exploration observations and emits route-regret alerts without regressing existing efficiency alerts; 11 tests passed.
- `cargo fmt -- --check`: validated Rust formatting; passed with no output.
- `cargo test`: validated full Rust suite; 245 unit tests passed, plus zero-test main/doc targets.
- `cargo build --release`: rebuilt the optimized `target/release/codex-team` binary; succeeded.

## Commands Run During ORCH-LESSONS-001 Others Reliability And Routing Lessons
- `cargo test tool_health`: validated tool failure classification, advisory cooldown aggregation, and watcher cooldown alerting; 3 tests passed.
- `cargo test route_learning`: validated route scorecard scoring, stable tie behavior, and positive route-learning duplicate suppression; 3 tests passed.
- `cargo test derived_status`: validated derived status priority and queue-to-worker derivation; 2 tests passed.
- `cargo test acceptance_contract`: validated acceptance contract derivation, characterization requirement checks, and pre-Worker risky-refactor blocking; 4 tests passed.
- `cargo test route_bench`: validated local route benchmark report generation; 1 test passed.
- `cargo test route_regret`: validated existing route-regret behavior after route-learning sidecars; 6 tests passed.
- `cargo test efficiency_watch`: validated existing watcher alerts plus tool-health cooldown reporting; 12 tests passed.
- `cargo fmt -- --check`: initially reported formatting drift, then passed after `cargo fmt`.
- `cargo test`: initially failed only the support-brief golden expectation for the newly added acceptance contract field; passed after updating the expected definition of done. Final full suite: 258 unit tests passed, plus zero-test main/doc targets.
- `cargo build --release`: rebuilt the optimized `target/release/codex-team` binary; succeeded.
- `git diff --check`: validated no whitespace errors; passed.

## Commands Run During Interactive Watch-Loop Human Disposition
- `cargo test watch_loop`: validated non-interactive pause stop behavior plus prompt-mode recheck/preserve disposition handling; 7 tests passed.
- `cargo test human_disposition`: validated existing human-disposition parse, command, refresh, and mission-summary behavior; 10 tests passed.
- `cargo fmt -- --check`: initially reported formatting drift, then passed after `cargo fmt`.
- `cargo test`: validated full Rust suite; 215 unit tests passed, plus zero-test main/doc targets.
- `cargo build --release`: rebuilt the optimized `target/release/codex-team` binary; succeeded.
- `git diff --check`: validated no whitespace errors; passed.

## Commands Run During Watch-Loop Auto-Gate PM Pauses
- `cargo test watch_loop`: validated `watch-loop --self-heal` auto-gates PM-created gateable pauses and does not auto-gate dirty-adoption pauses; 9 tests passed.
- `cargo test human_disposition`: validated existing human-disposition parse, command, refresh, and mission-summary behavior; 10 tests passed.
- `cargo fmt -- --check`: initially reported formatting drift, then passed after `cargo fmt`.
- `cargo test`: validated full Rust suite; 217 unit tests passed, plus zero-test main/doc targets.
- `cargo build --release`: rebuilt the optimized `target/release/codex-team` binary; succeeded.
- `git diff --check`: validated no whitespace errors; passed.

## Commands Run During Watch-Loop Newly Created Pause Auto-Gate
- `cargo test watch_loop`: validated pre-step and post-step PM-created gateable pause auto-gating plus dirty-adoption no-auto-gate behavior; 11 tests passed.
- `cargo test human_disposition`: validated existing human-disposition parse, command, refresh, and mission-summary behavior; 10 tests passed.
- `cargo fmt -- --check`: initially reported formatting drift, then passed after `cargo fmt`.
- `cargo test`: validated full Rust suite; 219 unit tests passed, plus zero-test main/doc targets.
- `cargo build --release`: rebuilt the optimized `target/release/codex-team` binary; succeeded.
- `git diff --check`: validated no whitespace errors; passed.

## Commands Run During Ignored Queue Bookkeeping Staging
- `cargo test deterministic_queue_bookkeeping_can_commit_ignored_non_ai_queue`: validated an ignored untracked `NEXT_STEPS_non-ai.md` command-only task can be marked done, force-staged, committed, and leave no non-runtime dirty status; passed.
- `cargo test watch_loop`: validated existing watch-loop pause/auto-gate behavior remains intact; 11 tests passed.
- `cargo test command_only`: validated command-only evidence routing still runs safe work in Rust and stops unsafe work before Worker; 3 tests passed.
- `cargo fmt -- --check`: initially reported formatting drift, then passed after `cargo fmt`.
- `cargo test`: validated full Rust suite; 220 unit tests passed, plus zero-test main/doc targets.
- `cargo build --release`: rebuilt the optimized `target/release/codex-team` binary; succeeded.
- `git diff --check`: validated no whitespace errors; passed.

## Commands Run During ORCH-LEARN-001 Repo Lock And Atomic IO
- `cargo test lock`: validated repo lock acquisition/release, live-lock refusal, stale dead-PID recovery, corrupt-lock refusal, token-safe release, sorted multi-repo acquisition, and app-level `step`/`run` live-lock refusal; 15 tests passed.
- `cargo test fs_util`: validated atomic text replacement, parent directory creation, permission preservation, JSONL append validity, large-string truncation, and newline termination; 5 tests passed.
- `cargo test watch_loop`: validated existing watch-loop behavior plus target/orchestrator duplicate-lock handling; 12 tests passed.
- `cargo test human_disposition`: validated human-disposition pause/state paths still pass through the atomic write wrapper safely; 10 tests passed.
- `cargo fmt -- --check`: initially reported rustfmt wrapping drift in new lock/IO helpers, then passed after `cargo fmt`.
- `cargo test`: validated full Rust suite; 234 unit tests passed, plus zero-test main/doc targets.
- `cargo build --release`: deferred while the existing runtime was still running, then rebuilt the optimized `target/release/codex-team` binary after the attached watch-loop finished; passed.
- `git diff --check`: validated no whitespace errors; passed.
