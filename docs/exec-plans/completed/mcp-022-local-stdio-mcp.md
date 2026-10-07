# MCP-022 — Sandboxed Local-stdio MCP

Status: COMPLETE (2026-08-09)
Owner/session: Codex primary task
Last updated: 2026-08-09

## Goal

Connect lazily to explicitly configured local stdio MCP servers, expose collision-safe dynamic tools and bounded text resources through the existing registry, approval, workspace, and sandbox boundaries, and terminate every child on shutdown.

## Non-Goals

- Remote transports, OAuth, prompts, sampling, elicitation, binary content, detached servers, or server-provided authorization.
- Cursor parity while `PAR-005` remains blocked.
- Oracle/reference changes, runtime credential/config changes, installed binaries, user sessions, or interaction with the live TUI.

## Evidence and Current Behavior

- `ToolRegistry::register_dynamic` already rejects built-in and dynamic name collisions, but dynamic specs cannot yet execute through a distinct handler.
- `ToolOrigin::Mcp` and `ToolProfile::Mcp` exist as reserved typed variants; `ApprovalOrigin::Mcp` exists but has no runtime consumer.
- `WorkspaceContext`, `ApprovalBroker`, and `SandboxBackend` already provide root, prepared-resource, origin, process, and network policy boundaries.
- `ConfigStore` resolves environment placeholders only in its runtime copy while preserving the raw TOML document for atomic patches.
- `rmcp` 3.1.2 is available from crates.io with `client` and `transport-child-process`; default server/macros features can remain disabled.
- Inferred: the existing required macOS sandbox accepts shell command text only, so MCP needs a direct executable/argument preparation method before child transport construction.

## Scope

Allowed files: `Cargo.toml`, `Cargo.lock`, `src/`, focused tests/fixtures/snapshots, this plan, `NEXT_STEPS.md`, `TESTING.md`, `CONTINUITY.md`, and `ARCHITECTURE.md`.

Forbidden: `oracles/`, `reference/`, credentials, user runtime config/session files, installed binaries, and live processes other than hermetic test children.

Runtime state: tests use injected temporary config/workspace roots and hermetic fake stdio servers only.

## Design and Invariants

- Pin `rmcp = 3.1.2` with default features disabled and only client/child-process stdio support.
- User MCP config is part of raw `config.toml`; project config is exactly `<primary>/.ca/mcp.toml` and is inactive until a host trust record matches canonical workspace path and SHA-256.
- Config commands use executable plus argument arrays, never a shell. Environment starts from a fixed allowlist, overlays configured values after runtime placeholder resolution, and never appears in status/errors.
- Enabled servers start only on catalog/resource/call demand. Local CLI help/version and ordinary startup create no child.
- Each server is initialized with timeouts; tools and text resources are paged to bounded totals. Calls, reads, stderr, and displayed output are capped and sanitized.
- Names are mapped deterministically to `mcp__<server>__<tool>` identifiers, with digest suffixes when sanitization would collide. Dynamic tools cannot shadow built-ins.
- MCP annotations are descriptive only. Every dynamic call is mutating/prompt-by-default and carries `ToolOrigin::Mcp(server)` through the approval broker.
- The exact executable/arguments/environment/network decision is prepared once under workspace/sandbox policy before spawn. Network requires trusted configuration plus explicit approval; default is denied.
- Tool-list/resource-list change notifications mark the catalog stale and refresh before the next provider turn.
- Timeout, cancellation, malformed protocol, child exit, and bounded-output violations return typed failures. `/mcp reconnect` closes the old service before a new lazy start. Manager drop/shutdown cancels all services.

## Milestones

### M1 — Characterization and config/trust
- Add failing tests for backward-compatible user config, project hash trust, placeholder redaction, environment filtering, name mapping, and lazy startup.
- Implement config/trust parsing and atomic project-trust patching.

### M2 — Sandboxed transport and manager
- Add a hermetic fake stdio server covering initialization, pagination, tool calls, text resources, malformed output, timeout, cancellation, stderr bounds, environment filtering, and cleanup.
- Implement direct sandbox preparation, `McpManager`, lazy lifecycle, reconnect, and shutdown.

### M3 — Registry, approval, provider, and TUI integration
- Register discovered tools dynamically with origin/schema validation and route calls through approval and the manager.
- Add `/mcp`, `/mcp verbose`, `/mcp reconnect <server>` plus reviewed wide/narrow status snapshots.
- Refresh stale list-change catalogs before the next provider request.

### M4 — Completion
- Run focused and repository-wide gates, local help/version, reconcile harness docs, and move this plan only when every required gate passes.

## Acceptance Criteria

- Help/version/local semantic exits start no MCP process.
- Project config is inactive without a path-and-hash host trust record; repositories cannot modify their own trust.
- Fake servers prove lazy startup, initialization, discovery/call/resource behavior, timeout/cancel/reconnect, environment filtering, output caps, and cleanup.
- Dynamic names are collision-safe, cannot shadow built-ins, and preserve server origin through prompt-by-default approval.
- Text resources are bounded and binary content fails closed.
- Sandboxing and explicit network policy apply to MCP children; unsupported required mode fails closed.
- Visible status includes server origin/state without resolved secrets or unsafe terminal sequences.

## Validation

Focused: PASS — 10 MCP manager/protocol tests, config/registry/headless/command suites, 2 loopback CLI tests, and 28 render tests.
Completion: PASS — 257 unit tests plus 18 CLI integration tests; format, strict Clippy, release, local help, and version all pass.
Manual/visual: PASS — MCP status snapshots reviewed at 100×30 and 60×24; 52 accepted snapshots total and zero pending. Live TUI remained forbidden.
Blocked checks: clean Cursor comparison remains blocked under `PAR-005`.

## Progress

- [x] Existing registry/config/workspace/sandbox/approval/command boundaries traced.
- [x] Characterization tests added.
- [x] Config and trust boundary complete.
- [x] Sandboxed transport and manager complete.
- [x] Registry/approval/provider/TUI integration complete.
- [x] Completion gates and documentation complete.

## Decisions

- Decision: use exact crates.io `rmcp` 3.1.2 with defaults off and only client plus child-process transport features.
  Evidence: `cargo info rmcp@3.1.2` reports the requested version and feature names.
  Consequence: server macros, HTTP transports, OAuth, and unrelated protocol surfaces are not linked.
- Decision: treat all MCP tools as mutating/prompt-by-default in v1.
  Evidence: server annotations cannot authorize execution and arbitrary remote behavior cannot be proven read-only.
  Consequence: exact user/project rules may allow a namespaced tool, but server metadata alone never bypasses approval.

## Surprises and Discoveries

- The requested 3.1.2 release is present on crates.io even though the indexed GitHub release page initially surfaced 3.0.1.

## Recovery and Rollback

Use milestone-scoped `apply_patch` reversals; never use destructive Git commands. New config fields remain optional/defaulted, and no manager construction occurs on local CLI exits.

## Completion Record

Changed files: `Cargo.toml`, `Cargo.lock`, `src/{config,sandbox,mcp,approval,app,main,headless}.rs`, `src/agent/{registry,tools}.rs`, `tests/cli.rs`, MCP render snapshots, and harness documentation.
Commands and results: focused suites PASS; `cargo test` PASS (257 unit, 18 CLI); `cargo fmt --check`, strict Clippy, release build, local help, and version PASS.
Remaining risks: remote transports and every v1 exclusion remain intentionally absent; clean Cursor comparison remains blocked under `PAR-005`; deprecated `sandbox-exec` remains isolated behind runtime capability checking.
Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, and `TESTING.md` now record the ownership, invariants, results, and next handoff.
