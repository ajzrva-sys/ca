# WSP-020 — Workspace Roots, Approval Broker, and macOS Sandbox

Status: COMPLETE
Owner/session: Codex primary task
Last updated: 2026-08-09

## Goal

Resolve every model-controlled filesystem/process resource once against explicit workspace roots, approve that prepared invocation with origin context, and execute processes under a capability-checked macOS sandbox with network denied by default.

## Non-Goals

- MCP/skills/agents/review implementations owned by later plans.
- Detached processes, remote sandbox services, or GUI root management.
- Oracle edits, credential/runtime-session mutation, installation, or live TUI interaction.
- Cursor parity claims while `PAR-005` is blocked.

## Evidence and Current Behavior

- `src/agent/tools.rs::resolve_path` accepts arbitrary absolute paths, `~`, traversal, and symlink escape; handlers re-resolve raw arguments after approval.
- `src/approval.rs` independently resolves path strings for glob rules.
- `src/jobs.rs::JobManager::start` executes `sh -c` directly with the requested cwd and inherited environment/network.
- `src/agent/registry.rs::ToolSpec::resources` already identifies path, cwd, command, URL, and job fields but execution does not prepare them.
- Direct shell and headless tools share jobs/tool handlers but do not share a workspace/sandbox broker.
- Project-instruction discovery walks to filesystem root.
- Inferred: resource preparation can build on the typed registry and feed normalized arguments to existing handlers, avoiding a handler rewrite.

## Scope

Allowed: `src/`, focused tests/snapshots, this plan, `NEXT_STEPS.md`, `TESTING.md`, `CONTINUITY.md`, `ARCHITECTURE.md`.

Forbidden: `oracles/`, `reference/`, credentials, runtime config/sessions, installed binary, live processes outside hermetic tests.

Runtime state: tests use temporary roots/jobs/config only.

## Design and Invariants

- `WorkspaceContext` owns stable root IDs, primary/additional roots, current cwd, and root-relative display paths.
- Primary root is Git top-level when discoverable, otherwise canonical launch cwd.
- Existing resources canonicalize exactly; creations canonicalize the nearest existing ancestor and append only normal components.
- Absolute model paths, traversal, missing restored roots, and symlink escapes outside roots fail before approval/execution.
- Explicit user attachments get one-shot reads only and do not add roots.
- `PreparedToolInvocation` contains validated normalized arguments and prepared resources. Approval and execution consume it without raw re-resolution.
- `ApprovalBroker` tags main/review/agent/MCP origin and persistent rules never widen workspace roots.
- `SandboxBackend` is injectable. macOS process policy uses `/usr/bin/sandbox-exec`; unsupported required mode fails closed. Policy-only/off requires explicit configuration.
- Sandboxed commands may read required system/runtime paths, write workspace roots and a per-job temp directory, and cannot access network unless both request and host grant are true.
- `--yolo` affects approval prompts only, never root, sandbox, or network enforcement.
- Jobs carry run/session ownership; job-control operations require the same owner.

## Milestones

### M1 — Workspace characterization and resolution
- Add temporary-root traversal, symlink, creation-ancestor, Git-root, additional-root, and instruction-boundary tests.
- Implement roots, stable IDs, resolve operations, `/add-dir`, and `/workspace` state/persistence semantics.

### M2 — Prepared resources and approval broker
- Add registry-exhaustive preparation tests and origin/rule tests.
- Normalize resource arguments once and route model/headless tool decisions through the broker.

### M3 — Sandbox and network policy
- Add injectable backend and hermetic filesystem plus loopback denial/grant tests.
- Route tool/direct shell jobs through sandbox policy; parse explicit network requests/flags.

### M4 — Ownership, TUI surfaces, and completion
- Enforce job ownership, render workspace controls, review wide/narrow snapshots, run all gates, reconcile docs, move plan.

## Acceptance Criteria

- Every registry path/cwd field is contained or rejected before execution.
- Creation cannot escape through traversal or a symlinked ancestor.
- Project instructions do not cross the selected root.
- Approval sees the same prepared resource execution consumes.
- Network is denied absent both request and host grant; yolo cannot widen it.
- Unsupported required sandbox fails deterministically.
- Cross-owner job await/stdin/kill fails closed.
- Automated tests and all completion gates pass without touching user runtime state.

## Validation

Focused: PASS — workspace (4), approval (47), jobs (10), job tools (3), headless (10), session (6), command/input and orchestration policy tests, plus macOS filesystem and loopback network checks.
Completion: PASS — `cargo test` (226 unit + 15 CLI), `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo build --release`, `target/release/ca --help`, and `target/release/ca --version`.
Manual/visual: reviewed workspace-root and updated command-palette snapshots at 100×30 and 60×24; 48 accepted snapshots and zero `.snap.new`. The live TUI was not touched.
Blocked: clean Cursor fixture under `PAR-005`.

## Progress

- [x] Current resolution, registry resource, approval, jobs, direct-shell, and instruction paths traced.
- [x] Workspace resolution implemented.
- [x] Prepared approval/execution implemented.
- [x] Sandbox/network/ownership implemented.
- [x] TUI and completion gates complete.

## Decisions

- Decision: normalize arguments rather than let handlers resolve raw paths.
  Evidence: registry resource metadata already describes the affected fields.
  Consequence: approval and existing handlers can share one immutable prepared JSON value.
- Decision: network permission is an intersection of model request and host/run grant.
  Evidence: requirements explicitly forbid yolo from enabling network.
  Consequence: either missing bit denies network.
- Decision: required sandbox mode is the default on every platform.
  Evidence: unsupported platforms must fail sandbox-required execution unless policy-only/off is explicitly selected.
  Consequence: portability requires an intentional configuration choice rather than a silent downgrade.

## Surprises and Discoveries

- `ToolSpec::resources` already covers nearly all filesystem/process fields, making exhaustive preparation testable.
- macOS sandbox operations distinguish `/var` from its `/private/var` target during symlink traversal; scoped policies need the canonical target plus literal `/var`/`/tmp` traversal access.
- Broad `/private` read access was unnecessary; `/private/etc` plus explicit workspace/temp roots is sufficient for the tested shell lifecycle.

## Recovery and Rollback

Reverse focused patches by milestone with `apply_patch`; do not use destructive Git operations. All persisted additions are optional/defaulted.

## Completion Record

Changed files: `src/{workspace.rs,sandbox.rs,approval.rs,jobs.rs,app.rs,session.rs,config.rs,main.rs,headless.rs}`, `src/agent/{registry.rs,tools.rs}`, `src/tui/{commands.rs,input_tests.rs,render.rs,render_tests.rs,snapshots/*workspace_roots*.snap,snapshots/*command_palette*.snap}`, `tests/cli.rs`, and harness documentation.

Commands/results: focused suites PASS; `cargo test` PASS (226 + 15); format PASS; strict Clippy PASS; release PASS; local help/version PASS.

Remaining risks: `sandbox-exec` is deprecated and capability-checked at runtime; policy-only/off deliberately provide no OS process confinement. No live interactive TUI or external-network process was exercised. Cursor parity remains unverified.

Harness updates: `NEXT_STEPS.md`, `TESTING.md`, `CONTINUITY.md`, and `ARCHITECTURE.md` reconciled; this plan moved to completed.
