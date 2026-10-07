# AUD-091 — Full capability program completion audit

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-12

## Goal

Prove the approved 49-item Full Oracle-Informed Capability Program against the current checkout, repair any contract gaps found by the audit, rerun the affected and repository-wide gates, and leave one durable requirement-to-evidence matrix before marking the persistent goal complete.

## Non-Goals

- Do not add a fiftieth capability or broaden any accepted roadmap item.
- Do not implement undocumented ChatGPT/Codex OAuth, modify oracle/reference trees, or touch real user config, Keychain, sessions, external services, or an installed binary.
- Do not treat an archived ExecPlan label alone as proof of implementation.

## Evidence and Current Behavior

- All 49 roadmap IDs have one artifact under `docs/exec-plans/completed/`; 48 report complete and AUTH-060 records the program-authorized blocked-without-workaround result.
- The active-plan directory was empty before this audit, and every roadmap plan has zero unchecked progress items.
- Current `cargo test --no-fail-fast` passes 1085 unit tests and 36 CLI tests; formatting, strict Clippy, and release build pass after UPD-090.
- Source inventory confirms every capability module/state surface and 440 reviewed ratatui snapshots. Cargo/source do not depend on `oracles/` or `reference/`.
- The first public-contract pass found that service operations/events, schedule definitions, and plugin manifests exist but do not expose the exact public boundary names required by the program.

## Scope

Allowed: visibility/naming-only corrections in `src/{service,schedule,plugin}.rs`, focused compile/serialization tests, a durable audit matrix, standard harness reconciliation, and this plan.

Forbidden: runtime behavior changes outside the named contract correction; oracle/reference or real runtime-state writes.

## Design and Invariants

- Rename the internal service wire enums to exact public `ServeOp` and `ServeEvent` names without changing their serde representation or released `ca.serve.v1` bytes.
- Make the existing strict `ScheduleDefinition` and `PluginManifest` types public without widening persistence, CLI, approval, network, or secret authority.
- Keep sensitive fields and safe Debug behavior unchanged.
- Audit evidence must combine current source, current tests, completed-plan acceptance/walkthrough records, and explicit program-boundary checks.

## Milestones

1. Inventory all roadmap IDs, plans, source seams, snapshots, and completion evidence.
2. Repair exact public type-name gaps with characterization tests.
3. Build the 49-item requirement/evidence matrix and recheck cross-cutting compatibility/security boundaries.
4. Run focused and repository-wide gates, reconcile the harness, archive this plan, and only then mark the persistent goal complete.

## Acceptance Criteria

- Every roadmap ID has an implementation or the exact accepted blocked outcome, a completed plan, current source evidence, and test/walkthrough evidence.
- Exact public types named by the program compile under their requested names.
- Released service JSON remains `ca.serve.v1` compatible.
- Current full tests, format, strict Clippy, release build, and local help/version/profile/manager smokes pass.
- No active feature plan, unchecked program milestone, oracle/reference dependency, or unexplained requirement remains.

## Validation

- Public boundary characterization: PASS. `ServeOp` decodes the released v1 `status` operation and `ServeEvent::Goodbye` serializes unchanged; exact public schedule/plugin type names compile.
- Focused service/schedule/plugin suites: PASS (21 service, 11 schedule-filtered plus released CLI, and 14 plugin/catalog-filtered plus released CLI tests).
- Public-contract/default-closed audit test: PASS.
- `cargo test --no-fail-fast`: PASS, 1087 unit tests and 36 CLI integration tests; one explicit manual browser test remains ignored by the ordinary gate and previously passed under BRW-086.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- Released help/version/default/named/bare/serve/attach/schedule/plugin/runtime/local/update smokes: PASS under one disposable home. Missing profile/config/data paths remained absent and no pending snapshots exist.

## Progress

- [x] Inventory the 49 roadmap plans and current source/snapshot seams.
- [x] Repair and test named public boundary gaps.
- [x] Record the requirement-to-evidence matrix and cross-cutting audit.
- [x] Rerun final gates after making definition fields inspectable and adding the explicit typed dependency edge, then archive.

## Decisions

- Decision: use a dedicated audit plan rather than retroactively changing the scope of UPD-090.
  Evidence: the persistent goal requires a full requirement-by-requirement completion proof, and the public-type discrepancy crosses three already completed plans.
  Consequence: the correction and final proof remain explicit without inventing another capability.

## Surprises and Discoveries

- Plan status and runtime behavior were both present for service, schedules, and plugins, but the exact public names requested by the program were not, demonstrating why plan labels alone are insufficient completion evidence.

## Recovery and Rollback

The source correction is visibility/naming-only and can be reverted with a targeted patch. Validation uses existing hermetic tests and temporary homes only.

## Completion Record

Changed files: `src/{main,service,schedule,plugin,agent_graph}.rs`, `docs/FULL_CAPABILITY_AUDIT.md`, this plan, and standard harness documents. The strict schedule/plugin definition fields and service event summary fields are inspectable, and `AgentDependencyEdge` is an explicit serializable public edge projection; runtime authority is unchanged.

Commands and results: 49/49 audit rows and archived plans resolved with no broken links or unchecked milestone; public contract/default-closed tests passed; focused service/schedule/plugin tests passed; 1087 unit plus 36 CLI tests passed; formatting, strict Clippy, release build, snapshot hygiene, and released manager/profile smokes passed.

Remaining risks: AUTH-060 remains intentionally unavailable until OpenAI publishes a supported third-party direct OAuth/provider contract or the user separately approves an app-server adapter. Individual historical walkthroughs are evidenced by their archived plans; this audit did not repeat destructive, microphone, external-browser, or runtime-install scenarios against real user state.

Harness updates: the knowledge index points to the 49-item audit matrix; architecture/continuity/testing/next steps record the exact public contract correction and final program gate; the refreshed AUTH-060 plan records current official evidence.
