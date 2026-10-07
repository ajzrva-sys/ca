# SKL-181 — Session-Automatic Skill Activation

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-25

## Goal

Let the model activate an exact relevant skill as part of an ordinary session
without stopping for generic approval, then use that digest-pinned instruction
body on the next provider turn.

## Non-Goals

- Do not activate skills from lexical composer matching or the removed passive
  footer.
- Do not let aliases, descriptions, or ambiguous names activate a skill.
- Do not execute skill scripts, install dependencies, widen tools, bypass
  persistent Deny/Ask rules, or auto-approve later actions suggested by a skill.
- Do not change learning publication, skill creation/editing, bundles, plugins,
  subagent authority, or oracle/reference content.

## Baseline Evidence

- `src/skills.rs::SkillRegistry::catalog_prompt` exposes bounded metadata and
  previously told the model to activate only when the user explicitly
  requested a skill.
- `src/agent/registry.rs` exposes `activate_skill` only to the Main profile,
  constrains its `id` to exact available selectors, and classifies it as
  `Mutating`.
- `src/main.rs::execute_pending_tools_inner` sends an ordinary unmatched model
  activation through generic approval in the baseline; the focused
  characterization test proved it was inactive until approval was resolved.
- `src/headless.rs::execute_call` and `src/service.rs::execute_service_call`
  independently gate unmatched model activations, so the behavior is not owned
  by one shared approval branch.
- `SkillRegistry::activate_exact` reopens and hashes the selected skill, rejects
  drift/ambiguity/disabled content, and enforces the shared instruction budget.
- Session V4 stores exact active skill ID/path/digest refs and restore revalidates
  all three. Switching sessions rediscovers the project registry and restores
  only that session's refs.

## Scope

Allowed files: `src/skills.rs`, `src/agent/registry.rs`, `src/main.rs`,
`src/headless.rs`, `src/service.rs`, focused existing tests in those modules,
architecture and harness documents, this plan.

Forbidden files/areas: `oracles/`, `reference/`, runtime config/session data,
installed binaries, provider credentials, plugin installation, skill source
trees outside test fixtures.

Runtime state affected: only the current run/session's in-memory active skill
set and the already-existing digest-pinned refs written when that session is
normally saved.

## Design and Invariants

- The model, not a lexical host matcher, decides relevance from the existing
  bounded metadata catalog and calls the exact enum-constrained
  `activate_skill` tool.
- An unmatched `activate_skill` call is an approval-free session-context
  operation in interactive, ordinary headless, and service sessions. An exact
  persistent Deny still denies it; an exact Ask remains an explicit override.
- `activate_skill` remains classified as mutating for workflow/evidence truth;
  only its generic approval default changes.
- Successful activation adds the already-validated body to the next provider
  request. It grants no process, filesystem, network, MCP, email, privilege, or
  tool authority. Every later action follows unchanged policy.
- Exact ID, ambiguity, drift, disabled-state, token-budget, session-save, and
  restore checks remain unchanged.

## Milestones

### M1 — Characterize the approval-free session contract

- Change/add focused tests so ordinary interactive, headless, and service model
  activation succeeds without a generic approval response.
- Retain or add denial/Ask and later-mutating-tool coverage.
- Run the focused tests and confirm they fail before implementation.

### M2 — Implement automatic model-selected activation

- Update the catalog instruction to tell the model to activate a relevant exact
  skill before proceeding.
- Bypass only generic unmatched approval for `activate_skill` in all three
  ordinary session execution paths.
- Keep exact explicit Ask/Deny and every execution/authority invariant.

### M3 — Validate and reconcile

- Run focused skill, headless, service, and session tests; refresh only affected
  snapshots if any.
- Run repository completion gates from `TESTING.md` and record actual results.
- Update `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`, and
  this plan; move the plan to completed only when all required work is done.

## Acceptance Criteria

- A model call `activate_skill {"id":"<exact available id>"}` executes without
  a generic approval interruption in interactive, ordinary headless, and
  service sessions.
- The next provider request includes only the activated digest-pinned body.
- Unknown, ambiguous, changed, disabled, or over-budget skills still fail.
- Exact persistent Deny denies activation and exact Ask can still require
  approval.
- Activating a skill never auto-approves a later mutating, network, email,
  privilege, or process action.
- Saved/restored sessions retain only their own exact active refs.

## Validation

Focused:

- The three new interactive, headless, and service automatic-activation tests
  failed before implementation at their generic approval boundaries.
- `cargo test automatic --no-fail-fast`: PASS after implementation.
- `cargo test catalog_is_metadata_only_and_activation_is_hash_bound
  --no-fail-fast`: PASS.
- `cargo test skill --no-fail-fast`: PASS (54 CA, two CLI, one embedded test).
- `cargo test headless --no-fail-fast`: PASS (39 CA, six CLI tests).
- `cargo test service --no-fail-fast`: PASS (30 CA tests).
- `cargo test session --no-fail-fast`: PASS serially (185 CA and seven
  embedded tests). Five unrelated process/socket tests that failed during a
  concurrent first run each passed exactly before the complete serial rerun.

Completion:

- `cargo fmt --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  PASS.
- `cargo build --release`: PASS.
- The first `cargo test --workspace --all-features --no-fail-fast --
  --test-threads=1` run found one stale AutoReview test assertion; all other
  targets passed. The assertion was updated and its exact rerun passed.
- Final `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS
  (2,006 CA, 38 CLI, 17 role-run, 556 embedded tests, and all integration and
  doc-test targets; three expected manual tests ignored).

Manual/visual: no new TUI surface is expected; deterministic request/tool-loop
tests are the acceptance surface.

Blocked checks: none currently.

## Progress

- [x] Current prompt, registry, approval, interactive, headless, service, and
  session persistence paths inspected.
- [x] Failing focused characterization tests added for interactive, headless,
  and service sessions; all three reproduced the approval interruption.
- [x] Implementation complete across catalog prompting and interactive,
  headless, and service approval paths.
- [x] Focused and completion validation recorded.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: let the model select relevance from metadata instead of restoring
  passive lexical activation.
  Evidence: the catalog and exact enum-constrained tool already exist; lexical
  suggestions were distracting and can produce false positives.
  Consequence: automation is contextual while the composer stays quiet.

- Decision: preserve exact Ask/Deny overrides and later tool approvals.
  Evidence: these are explicit user policy and distinct from generic unmatched
  approval.
  Consequence: automatic skill use does not become an authority bypass.

## Surprises and Discoveries

- The activation approval decision is duplicated across interactive, headless,
  and service execution; changing only the central mutation label would also
  misclassify workflow evidence and other read-only policy.

## Recovery and Rollback

Revert the catalog wording and the three narrowly scoped approval exceptions;
the session schema and persisted data require no migration or cleanup.

## Completion Record

Changed files: `src/skills.rs`, `src/agent/registry.rs`, `src/main.rs`,
`src/headless.rs`, `src/service.rs`, `ARCHITECTURE.md`, `CONTINUITY.md`,
`NEXT_STEPS.md`, `TESTING.md`, `docs/.INDEX.md`, and this plan.

Commands and results: focused automatic/skill/headless/service/session suites,
formatting, strict workspace Clippy, release build, and final default serial
workspace suite all pass. The all-feature suite's only failure was a stale
test expectation corrected and passed exactly; all other all-feature targets
passed.

Remaining risks: relevance quality depends on the model and bounded skill
metadata. Exact selectors, digest revalidation, explicit policy, instruction
budget, and later tool authority contain a mistaken relevance choice.

Harness updates: architecture, continuity, task queue, validation record, and
index now describe the completed session-automatic contract.
