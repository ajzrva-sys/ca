# AGT-024 — Asynchronous Subagent Manager

Status: COMPLETE (2026-08-09)
Owner/session: Codex primary task
Last updated: 2026-08-09

## Goal

Replace the inline legacy `task` implementation with a process-scoped asynchronous manager that gives every child stable identity, bounded lifecycle/event state, explicit ownership and limits, common tool/policy enforcement, targeted messaging/cancellation, and visible `/agents` control.

## Non-Goals

- Detached or automatically resumed processes, wider child authority, full parent transcripts by default, or implicit approval inheritance.
- Parallel execution beyond configured limits or Cursor parity while `PAR-005` is blocked.

## Evidence and Current Behavior

- `src/agent/subagent.rs::run_subagent` runs inline for up to ten turns and returns one string.
- `ToolHandler::Task` loads global config at execution time and calls raw child tools without the common workspace preparation, sandbox, approval broker, job ownership, or typed manager lifecycle.
- `SessionV3` now has non-running `agent_summaries`; full/history branches intentionally clear them.
- The typed registry already has `ToolProfile::Subagent`, capability/resource metadata, and origin-aware approval boundaries.

## Scope

Allowed: `src/`, focused tests/snapshots, this plan, and harness docs.
Forbidden: oracle/reference trees, credentials, user runtime state, installed binaries, and the live TUI/process.
Runtime tests: injected fake child runners/providers, temporary workspaces, and short-lived sandboxed jobs only.

## Design and Invariants

- Stable `AgentId`, status, parent/task ownership, depth, context mode, exact grants, bounded events/output, result summary, and manager-owned cancellation token/handle.
- Defaults: 3 concurrent children, depth 2, 8 children per parent turn, 30 provider turns, 30 minutes, and 256 KiB retained output.
- Default context is a bounded state summary (goal, approved plan/task, constraints, checkpoint, active skills, assigned files), never the full transcript unless explicitly permitted.
- Children receive only the intersection of requested tools and exact parent-approved capabilities/resources. Workspace roots, sandbox, network, job ownership, and registry validation remain common and cannot widen.
- Out-of-scope calls pause only that child in an origin-tagged approval state; approval resumes the prepared exact call.
- `task` becomes foreground spawn-plus-await compatibility; new spawn/await/message/cancel/list tools address stable IDs.
- `/agents` lists status/task/elapsed/approval/recent output and supports open/cancel/message/approval actions.
- Shutdown cancels all children and their owned jobs. Restored summaries become `Interrupted`; no process resumes.

## Milestones

### M1 — Manager state machine and characterization
- Add limits, ownership, bounded events/output, messaging, cancellation, timeout, and shutdown tests around an injected runner.

### M2 — Provider/tool policy integration
- Replace raw legacy execution with the manager and registry/workspace/sandbox/broker path; implement exact grants and per-child approval waits.

### M3 — Tools, TUI, persistence summaries
- Add stable tools, foreground `task` compatibility, `/agents`, snapshots, and interrupted summary restore.

### M4 — Completion
- Run focused/full gates, reconcile docs, and move this plan only when all checks pass.

## Acceptance Criteria

- Concurrency/depth/turn/runtime/children/output limits fail deterministically.
- Child tools cannot exceed exact inherited grants, roots, network, jobs, or sandbox policy; approval pauses one child only.
- Spawn/list/await/message/cancel are stable-ID based; `task` waits on the same manager record.
- Shutdown cancels children and owned jobs; restored running states become interrupted and never resume.
- `/agents` wide/narrow states are deterministic and reviewed.

## Validation

Focused: PASS — 6 manager/policy/nested-inheritance tests plus registry, session, headless, commands, and 30 render tests.
Completion: PASS — 271 unit tests plus 18 CLI integration tests; format, strict Clippy, release, help, and version all pass.
Manual/visual: PASS — `/agents` snapshots reviewed at 100×30 and 60×24; 56 accepted total and zero pending. Live TUI remained forbidden.
Blocked: live-provider child execution requires credentials and clean Cursor comparison remains blocked.

## Progress

- [x] Legacy handler, runner, registry, policy, session, and TUI boundaries traced.
- [x] Manager characterization complete.
- [x] Secure provider/tool integration complete.
- [x] Tools/TUI/persistence complete.
- [x] Completion gates/docs complete.

## Decisions

- Decision: remove runtime global config loading from the child handler.
  Evidence: it defeats injected paths/secrets and cannot carry parent run/session ownership.
  Consequence: manager construction receives a resolved runtime snapshot before any spawn.

## Surprises and Discoveries

- The legacy handler loaded global config and bypassed policy; main/headless dispatch now intercepts manager tools, while the raw handler fails closed without injected manager context.
- Prepared resource inheritance required explicit assigned-file and exact-command scopes in addition to tool names and workspace root IDs.

## Recovery and Rollback

Use milestone-scoped `apply_patch` reversals; never kill unrelated/live processes or mutate user runtime files.

## Completion Record

Changed files: `src/agent/{subagent,registry,tools}.rs`, `src/{app,main,config,headless,session}.rs`, `src/tui/{commands,render_tests}.rs`, two agent snapshots, and harness docs.
Commands/results: focused suites PASS; `cargo test` PASS (271 unit, 18 CLI); format, strict Clippy, release, help, and version PASS.
Remaining risks: provider execution was not run against live credentials; nested children require explicit parent grants, inherit only a subset, and remain bounded by depth/concurrency/turn limits.
Harness updates: architecture, continuity, testing, and next-step records now point to `REV-025`.
