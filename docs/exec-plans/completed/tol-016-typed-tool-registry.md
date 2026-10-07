# TOL-016 — Typed Tool Registry

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-09

## Goal

Make one registry authoritative for tool discovery, schema/argument validation, dispatch, approval mutation classification, origin/capabilities/resources, and runner profiles, with typed execution outcomes.

## Non-Goals

- Dynamic MCP transport (`MCP-022`) or asynchronous agent redesign (`AGT-024`).
- Changing existing built-in names, required arguments, approval-rule strings, or successful model-visible output.
- Workspace/sandbox enforcement (`WSP-020`).

## Evidence and Current Behavior

- `src/agent/tools.rs::default_tool_defs` manually defined 27 schemas while `execute_tool` separately dispatched 27 string arms.
- `src/main.rs::tool_needs_approval` maintains a third name list.
- `execute_pending_tools_async` deduplicates by name/arguments, causing distinct call IDs not to execute independently.
- Handler argument access defaults missing required values to empty strings.
- Strict Clippy retains one tool lint and two approval lints.

## Scope

Allowed files: `Cargo.toml`, `Cargo.lock`, new `src/agent/registry.rs`, `src/agent/tools.rs`, `src/agent/mod.rs`, narrow orchestration/approval/test callers, and harness docs.

Forbidden: oracle/reference trees, credentials, user runtime state, installed binary, live TUI, workspace/sandbox policy expansion.

Runtime state affected: focused execution tests use temporary roots and short-lived local jobs only.

## Design and Invariants

- `ToolRegistry` stores immutable built-in `ToolSpec`s keyed by stable `ToolName`.
- Each spec holds a generated `JsonSchema`, typed serde validator, `ToolHandler`, `ToolOrigin`, mutation, capabilities, resource fields, and profile membership.
- Built-in schemas and validation come from the same argument type.
- `ToolContext` carries cwd, timeout, and profile; handlers receive only validated JSON until the later prepared-resource boundary.
- `ToolExecutionResult` is success/failure/denial/cancellation and exposes unchanged model text.
- Approval `AutoReview` uses registry mutation metadata; missing/unknown tools require approval and fail closed on execution.
- No name/argument cache exists; call IDs remain the only identity.

## Milestones

### M1 — Characterization and registry invariants
- Add tests for unique names, schema/validator agreement, required fields, profile selection, capabilities/resources, dynamic collision, unknown tools, and typed outcomes.

### M2 — Registry and typed execution
- Add argument types, registry/spec/profile/context/result types, generated schemas, and enum dispatch.
- Migrate tool definition and execution callers.

### M3 — Approval and independent calls
- Replace approval name list with mutation metadata and remove name/argument deduplication.
- Test two identical invocations with distinct IDs both execute.

### M4 — Validation
- Clear all strict-Clippy findings, run focused/full tests, format, release, help/version, and reconcile harness docs.

## Acceptance Criteria

- Exactly one built-in spec exists per preserved name and every spec validates its generated required arguments.
- Profiles expose only intended tools; checkpoint/plan profiles are empty and dynamic tools cannot shadow built-ins.
- Missing required fields fail before handler side effects.
- Unknown tools produce typed failure.
- Duplicate invocations execute independently and preserve call IDs/order.
- Strict Clippy passes with `-D warnings`.

## Validation

Focused: PASS — registry invariants (3), job/tool typed execution (3), independent identical calls (1), and duplicate persistent denial (1).

Completion: PASS — 180 unit tests plus 3 CLI integration tests; format, strict Clippy, release, help, and version all pass.

Manual/visual: no intended visual change; no live TUI.

Blocked checks: none.

## Progress

- [x] Inventoried schemas, string dispatch, approval classification, and deduplication.
- [x] Added red registry invariant tests.
- [x] Implemented registry and migrated execution/discovery.
- [x] Removed deduplication and centralized approval mutation metadata.
- [x] Completed validation and harness updates.

## Decisions

- Decision: preserve handler output strings behind typed outcomes.
  Evidence: provider-visible compatibility depends on existing ordinary output.
  Consequence: orchestration can classify failure without rewriting successful text.

## Surprises and Discoveries

- Generated optional schemas initially admitted JSON `null`; normalization now preserves the previous omitted-or-concrete wire schema without changing optional serde validation.

## Recovery and Rollback

Inverse-patch only listed source/docs files; temporary execution-test roots are self-cleaning.

## Completion Record

Changed files: `Cargo.toml`, `Cargo.lock`, `src/agent/mod.rs`, new `src/agent/registry.rs`, `src/agent/tools.rs`, `src/main.rs`, `src/test.rs`, `src/agent/subagent.rs`, and harness docs.

Commands and results: focused suites PASS; `cargo test` PASS (180 + 3); `cargo fmt --check` PASS; `cargo clippy --all-targets -- -D warnings` PASS; `cargo build --release` PASS; local help/version PASS.

Remaining risks: handler internals still consume validated JSON values; prepared resources and origin-aware authorization intentionally remain for `WSP-020`/`MCP-022`.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, and `TESTING.md` record the registry boundary and actual validation.
