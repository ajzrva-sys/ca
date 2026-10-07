# ACP-213 — Stable ACP Interoperability

Status: COMPLETE (2026-08-28)
Owner/session: CUR-204 goal session
Last updated: 2026-08-28

## Goal

Expose CA as a stable ACP v1 stdio agent through `ca acp` using official Rust
SDK 2.0.0 while keeping CA service protocols and authority boundaries separate.

## Non-Goals

No ACP v2 experimental features, TCP/WebSocket listener, hosted auth/logout,
privileged-command secret path, or dependency on client filesystem/terminal.

## Evidence and Current Behavior

CA exposes its own `ca.serve.v1`; the official `agent-client-protocol=2.0.0`
retains stable v1 wire schema while v2 is experimental. Before this plan, no
ACP subcommand existed.

## Scope

Allowed: pinned SDK dependency, new ACP module/subcommand, session/provider/tool/
permission adapters, official-SDK fixtures, docs. Keep service v1/v2 bytes and
SEC-158 secret flow untouched.

## Design and Invariants

Newline JSON-RPC 2.0 stdio implements initialize, session/new, load, prompt,
cancel, and set_mode. Advertise agent/plan/ask; stream text, thought, tool, plan,
commands, usage, and mode updates. Translate approvals to
`session/request_permission`; absent permission support closes mutating/network
work. Use CA filesystem/tool enforcement and credentials. Paths are absolute
and locations one-based. Disconnect cancels owned work and emits no secret.

## Milestones

### M1 — Wire shell

Pin stable SDK with v2 disabled; add malformed JSON-RPC, init, capability, and
disconnect fixtures while proving service bytes unchanged.

### M2 — Session bridge

Implement new/load/prompt/cancel/mode and streaming updates with official SDK
client fixtures.

### M3 — Permissions and validation

Add allow/deny/unanswerable cases, paths/lines, usage/tools/plans/commands, then
run child gates and smokes.

## Acceptance Criteria

All required methods interoperate with official fixtures; malformed/disconnected
clients cleanly fail; mutating/network operations never run without permission;
service v1/v2 goldens remain byte-independent.

## Validation

Focused ACP SDK/service/session/permission/tool tests, stdio smokes, and child
gates.

## Progress

- [x] SDK pin and failure-first wire tests.
- [x] Session/mode/stream bridge.
- [x] Permission closure and fixtures.
- [x] Focused and child gates.

## Decisions

Stable wire v1 is the only advertised ACP dialect.

## Surprises and Discoveries

- The official SDK enables `serde_json/preserve_order` for the unified Cargo
  graph. That changed deterministic object ordering in five existing CA byte
  contracts and enlarged `rmcp::model::CallToolResult` to 200 bytes in the
  combined all-feature build, without changing protected codex-team source.
- CA now sorts JSON objects explicitly at the affected compatibility boundary,
  preserving the prior mechanistic, request, recipe, runtime-log, and portable-
  session bytes. Root `clippy.toml` keeps `result_large_err` enabled with a
  narrow 256-byte ceiling above the dependency-determined 200-byte type.
- ACP disconnect and failed transport cleanup needed a shared inner outcome so
  every prompt path cancels, joins, clears active ownership, and cleans local
  resources exactly once.

## Recovery and Rollback

ACP is additive; remove the subcommand/module/dependency without touching CA
service protocols.

## Completion Record

Implemented `ca acp` as stable ACP v1 newline JSON-RPC stdio through the pinned
official Rust SDK. It supports initialize, new/load/prompt/cancel/set-mode;
agent/plan/ask modes; bounded prompt blocks; text/thought/tool/plan/command/
usage/mode updates; fail-closed permission requests; absolute contained tool
locations with one-based lines; and owned disconnect cleanup. Client MCP,
filesystem, terminal, hosted auth/logout, WebSocket/TCP, privileged secrets,
and experimental v2 behavior are not advertised.

Changed implementation paths: `Cargo.toml`, `Cargo.lock`, `clippy.toml`,
`src/acp.rs`, `src/main.rs`, `src/service.rs`, `src/json_compat.rs`,
`src/request.rs`, `src/mechanistic_delegation.rs`,
`src/runtime_diagnostics.rs`, and `src/session_portability.rs`.

Validation:

- `cargo test acp::tests --bin ca -- --test-threads=1`: PASS, 9 tests using
  official SDK clients plus raw malformed/EOF transport fixtures.
- `cargo test service::tests --bin ca -- --test-threads=1`: PASS, 24 tests;
  the existing `ca.serve.v1` behavior remains independent.
- `cargo test -p ca --bin ca --no-fail-fast -- --test-threads=1`: PASS, 2,255
  tests with four documented ignores after compatibility-order regressions
  were corrected.
- Both exact workspace matrices pass. The default CA target passed 2,255 tests
  plus four documented ignores; the all-feature CA target passed 2,284 tests
  plus four documented ignores. Every workspace and integration target passed.
- `cargo fmt --all -- --check`, strict workspace/all-target/all-feature Clippy,
  and `cargo build --workspace --release --all-features`: PASS.
- Release help/version/ACP-help smokes and one credentialless stable v1
  initialize/EOF smoke pass.
- `git diff --check`, zero `.snap.new`, exact protected-baseline comparison,
  and protected-tree verifier: PASS.

No user configuration, session, provider, installed binary, oracle, reference,
or protected codex-team source was changed. Installation remains reserved for
REL-215.
