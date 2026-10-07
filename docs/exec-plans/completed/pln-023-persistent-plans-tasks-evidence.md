# PLN-023 — Persistent Plans, Tasks, and Evidence

Status: COMPLETE (2026-08-09)
Owner/session: Codex primary task
Last updated: 2026-08-09

## Goal

Upgrade sessions without eager mutation and add a revisioned, user-approved work artifact whose plan, tasks, decisions, and validation evidence are changed through typed orchestration and survive normal saves and full-history branches.

## Non-Goals

- Asynchronous child execution, which belongs to `AGT-024`.
- Automatic approval by a model, Markdown-only evidence, exact historical work-state reconstruction, or inheritance of running processes.
- Oracle parity claims while `PAR-005` remains blocked.

## Evidence and Current Behavior

- `src/session.rs` reads V1 and V2 into `SessionV2`, writes version 2, and keeps one stable file per UUID.
- `SessionV2` already persists reasoning effort, workspace roots, active skill references, messages, and checkpoint, but has no revisioned work state or review/agent records.
- `CommandId::Plan` currently toggles a boolean mode only in `src/main.rs`; there are no plan/task artifact surfaces or typed mutation tools.
- Session atomic writing and immutable full/historical branching already have temporary-root tests and form the compatibility boundary.
- Inferred: CAS must be applied by the orchestration owner, not by tool handlers, so background work cannot overwrite newer revisions.

## Scope

Allowed files: `src/`, session/plan tests and snapshots, this plan, `NEXT_STEPS.md`, `TESTING.md`, `CONTINUITY.md`, and `ARCHITECTURE.md`.

Forbidden: `oracles/`, `reference/`, credentials, user runtime config/session files, installed binaries, and the live TUI/process.

Runtime state: tests use injected temporary session roots and in-memory/fake orchestration only.

## Design and Invariants

- Introduce `SessionV3`; V1/V2 deserialize compatibly and are upgraded only on a normal atomic save with the same UUID/storage filename.
- Optional V3 fields include work state, reasoning effort, workspace roots, active skills, review records, and agent summaries with serde defaults.
- `WorkState` owns stable `PlanId`/`TaskId`, a monotonically increasing revision, approval state, tasks, decisions, timestamps, evidence references, and source lineage.
- Typed plan/task/evidence mutations carry an expected revision. One orchestration reducer serializes them and rejects stale revisions without partial mutation.
- Model tools may propose/edit/update but only an explicit user action may approve or reject a plan.
- Evidence references an existing recorded command/test/review event ID; unsupported prose cannot become validation evidence.
- Full-history forks clone work state under new plan/task IDs and retain source references. Historical branches omit work state and expose a notice.
- `/plan` with no argument keeps mode switching; `show|approve|reject|edit` manage the artifact. `/tasks` opens a deterministic task surface.

## Milestones

### M1 — Schema and migration characterization
- Add V1/V2/V3 load/save, stable-filename, rollback, default, and CAS characterization tests.
- Implement V3-compatible store without changing existing external session call sites unnecessarily.

### M2 — Typed work reducer and registry tools
- Add stable work types, validation-event references, revision/CAS reducer, and typed registry tools for proposal/task/evidence mutations.
- Add ordering, stale revision, approval authority, and evidence-integrity tests.

### M3 — Branching and TUI orchestration
- Implement full/history branch semantics, `/plan` actions, `/tasks`, user-only approval, persistence, and reviewed wide/narrow snapshots.

### M4 — Completion
- Run focused and repository-wide gates, reconcile harness docs, and move this plan only after every check passes.

## Acceptance Criteria

- V1/V2 load without filesystem mutation; next save writes V3 atomically to the same storage file and UUID.
- Stale expected revisions fail without changing work state, while accepted mutations increment exactly once.
- Only user orchestration can approve; tool calls cannot claim approval or fabricate validation evidence.
- Full forks regenerate plan/task IDs with source links; historical forks visibly omit work state.
- `/plan` compatibility and artifact actions plus `/tasks` are covered at wide/narrow widths.

## Validation

Focused: PASS — 3 work reducer, 7 session migration/store/branch, registry, typed orchestration, command, and 29 render tests.
Completion: PASS — 264 unit tests plus 18 CLI integration tests; format, strict Clippy, release, help, and version all pass.
Manual/visual: PASS — persistent plan/task/evidence snapshots reviewed at 100×30 and 60×24; 54 accepted total and zero pending. Live TUI remained forbidden.
Blocked checks: clean Cursor comparison remains blocked under `PAR-005`.

## Progress

- [x] Existing session/store/branch and plan-command boundaries traced.
- [x] V3 migration and work-state characterization complete.
- [x] Typed reducer and registry tools complete.
- [x] Branch/TUI/session integration complete.
- [x] Completion gates and documentation complete.

## Decisions

- Decision: preserve the public in-memory session type name during incremental migration only if it avoids unrelated churn; serialized version, not Rust naming, defines compatibility.
  Evidence: current orchestration has many direct `SessionV2` call sites while storage dispatch already inspects the JSON version.
  Consequence: migration tests must assert JSON version 3 and same file/UUID regardless of internal aliasing.

## Surprises and Discoveries

- Plan mode previously advertised no tools; its prompt and profile had to change together so `propose_plan` is the sole creation path while filesystem/process/network tools remain absent.

## Recovery and Rollback

Use milestone-scoped `apply_patch` reversals. Never rewrite test/user session roots or use destructive Git commands.

## Completion Record

Changed files: `src/work.rs`, `src/session.rs`, `src/{app,main}.rs`, `src/agent/{registry,tools}.rs`, `src/tui/{commands,render_tests}.rs`, two plan/task snapshots, and harness documentation.
Commands and results: focused suites PASS; `cargo test` PASS (264 unit, 18 CLI); `cargo fmt --check`, strict Clippy, release build, help, and version PASS.
Remaining risks: review records and agent summaries are schema-ready but populated by `REV-025` and `AGT-024`; exact historical work state is intentionally unavailable and visibly omitted.
Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, and `TESTING.md` record ownership, compatibility, invariants, validation, and next handoff.
