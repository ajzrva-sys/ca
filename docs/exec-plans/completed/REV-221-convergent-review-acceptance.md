# REV-221 — Convergent Review and Acceptance Packets

Status: COMPLETE
Owner/session: ORC-218 goal session
Last updated: 2026-08-29

## Goal

Add an explicitly configured, dual-route review mode in which two isolated
read-only reviewers receive the same objective evidence and must both pass,
then preserve the host-verifiable result as an immutable, bounded
`ca.acceptance.v1` packet without widening repair or repository authority.

## Non-Goals

- No same-provider/model fallback, context-only second opinion, hidden third
  reviewer, ambient confidence database, or belief decay.
- No automatic commit, push, merge, installation, provider-route discovery, or
  authority widening after a passing or failing review.
- No raw prompts, hidden reasoning, tool arguments/output, provider payloads,
  credentials, or absolute paths in acceptance packets.
- No paid-provider or hosted-service validation and no changes under
  `oracles/`, `reference/`, or the protected codex-team subtree.
- Existing single-route `/review` and `--check` behavior remains compatible
  unless convergence is explicitly requested.

## Evidence and Current Behavior

- `src/review.rs::run_review` already creates a fresh context, advertises only
  bounded read-only review tools plus `report_review_findings`, sanitizes its
  structured result, and records request usage.
- `src/main.rs` currently resolves `/review` through one optional
  `review.provider`/`review.model` route, then delivers one `ReviewReady` event.
- `src/headless.rs::run_verification` implements existing verify-only and
  explicitly enabled repair behavior through ordinary approval, permission,
  sandbox, network, and tool-selection boundaries.
- `src/usage.rs` uses exact decimal token pricing and rejects usage whose token
  subsets are not sufficient for an exact charge.
- Completed AGT-157 owns host-created verification receipts, deterministic
  source/workspace revision checks, writable-lane serialization, and monotonic
  autonomy budgets. REV-221 may read valid completed receipt digests but does
  not alter their authority.
- The ORC-218 baseline contains 868 dirty-tree records with canonical SHA-256
  `2dcaf84dc86d3ae27bfea593bc811ce16f46f9782382780901fdcf890ff39978`.
  All pre-existing bytes remain independently owned.

## Scope

Allowed files: focused additive review/acceptance modules and tests; narrow
edits in configuration, CLI/headless orchestration, TUI command/event/render
state, session/work state, usage integration, architecture/continuity/testing,
the queue/index, and the ORC umbrella plan.

Forbidden files/areas: `oracles/`, `reference/`, protected codex-team paths,
credentials/provider payloads, user runtime state, unrelated baseline hunks,
Git history, and the installed binary.

Runtime state affected: disposable profile/session/workspace fixtures only.
Real reviewer routes are never contacted during validation. Acceptance-store
tests use temporary private directories.

## Design and Invariants

- Extend `ReviewConfig` additively with exactly two named route slots plus
  bounded request/output/cost controls. Legacy `provider` and `model` retain
  the existing single-review meaning; convergence never derives or falls back
  from them.
- A convergence preflight resolves both routes before any request, requires
  non-empty distinct provider/model pairs, configured/available compatible
  providers and models, sufficient explicit pricing or a process-local
  unpriced acknowledgment, and budgets that can admit one pair.
- Capture one immutable review input and one objective rubric. Start both
  review futures together with separate backends, usage spans, tool registries,
  and message vectors. Neither result is added to the other context.
- A reviewer passes only by returning a valid structured report with no
  findings. Both must pass in the same round. Any error, disagreement, budget
  exhaustion, or third-round failure is terminally blocked.
- A plain convergence invocation runs one pair. Repair requires the existing
  explicit headless `--check-repair` authority or a Cancel-first TUI approval;
  repairs stay in the normal agent/tool path. Each post-repair review is a new
  pair. No run exceeds three pairs, six reviewer invocations, configured
  physical-request/output limits, or an exact configured cost ceiling.
- Acceptance packets are immutable content-addressed private records capped in
  count and bytes. They include only digests/metadata: request and revision
  identity, changed relative paths, valid AGT-157 receipt digests, host work
  event metadata, reviewer route/verdict/usage metadata, unresolved risks, and
  claim labels `verified`, `inferred`, `unverified`, or `contradicted`.
- Only evidence emitted and revalidated by host code may be labelled
  `verified`. Reviewer assertions cannot upgrade their own claims. A packet is
  stale when its bound source, workspace, or session revision differs.
- `/acceptance create|latest|list|export` is read/metadata-first. Create and
  export use bounded atomic private publication with collision and drift
  refusal. No command performs repository integration or installation.
- Existing public output/session/configuration bytes remain unchanged unless
  the new convergence/acceptance surface is explicitly selected; older saved
  configuration/session data loads through defaults without eager rewrites.

## Milestones

### M1 — Characterize routes, budgets, and isolation

- Add failing tests for missing, identical, unavailable, incompatible, and
  unpriced routes; identical objective input; concurrent isolated execution;
  pass/pass, pass/fail, and error outcomes; request/output/cost ceilings; and
  three-round exhaustion.
- Add CLI parser coverage for `--check-converge` and its conflicts, plus command
  parsing for `/review converge` and `/acceptance`.

### M2 — Implement convergent review

- Add defaulted configuration and strict preflight.
- Reuse the existing review runner with explicit per-run physical request and
  output budgets; run the two routes concurrently and record exact usage.
- Integrate fresh-review pair delivery into TUI and headless flows. Preserve
  existing single-review/check paths and authority-preserving repair behavior.

### M3 — Implement immutable acceptance packets

- Add `ca.acceptance.v1` types, validation, revision binding, host-evidence
  classification, private immutable storage, bounded list/latest/export, and
  stale-state evaluation.
- Connect packets to successful and blocked convergence records, current
  change snapshots, valid AGT-157 receipts, work events, and session revision.
- Add TUI acceptance browser/preview coverage at wide and narrow widths.

### M4 — Validate and reconcile

- Run focused route, review, headless, acceptance, session, command, and render
  suites; formatting; strict all-target/all-feature Clippy; and all-feature
  release build.
- Inspect snapshots and all diffs, verify zero `.snap.new`, confirm protected
  and oracle trees remain untouched, then reconcile ORC harness pages and move
  this plan to completed.

## Acceptance Criteria

- Convergence cannot make a provider request unless exactly two distinct,
  explicitly configured compatible routes pass the complete preflight.
- Both reviewers receive byte-identical objective material in independent fresh
  contexts and execute concurrently. Neither can observe the other's result.
- One pair is the default. Repair is explicitly authorized, keeps existing
  permissions, and never exceeds three rounds/six reviewer invocations.
- Both reviewers must pass. Disagreement, request/output/cost exhaustion,
  missing prices without one-run acknowledgment, and route errors remain
  visibly blocked without repository integration or authority widening.
- Acceptance packets are schema-valid, immutable, private, bounded, free of
  excluded content, host-evidence-labelled, and truthfully stale after any
  bound source/workspace/session revision changes.
- Existing single-route review, ordinary `--check`, sessions, provider
  protocols, and structured output remain compatible.

## Validation

Focused: review/config/CLI/headless/acceptance/session/work/render suites and
deterministic loopback backends only.

Completion: `cargo fmt --all -- --check`; strict
`cargo clippy --workspace --all-targets --all-features -- -D warnings`;
`cargo build --workspace --release --all-features`; `git diff --check`; zero
`.snap.new`; direct protected/oracle/reference inspection. Complete workspace
matrices remain the integrated REL-229 gate.

Manual/visual: review wide/narrow acceptance and convergence snapshots; inspect
blocked, stale, unpriced-acknowledgment, and disagreement copy.

Blocked checks: paid or hosted review routes are deliberately not contacted.

## Progress

- [x] Child plan and queue entry created after RES-220 completion.
- [x] Failure-first route, isolation, budget, outcome, and packet coverage added.
- [x] Dual-route convergence implemented for TUI and headless paths.
- [x] Immutable acceptance packet lifecycle implemented.
- [x] Focused suites and child completion gates pass.
- [x] Harness reconciled and plan completed.

## Decisions

- Decision: keep existing single-route review configuration as a legacy,
  independent surface and require two new explicit routes for convergence.
  Evidence: fallback to the active route would violate the user's locked
  distinct-route rule and could silently spend an unintended provider request.
  Consequence: old profiles remain compatible while incomplete convergence
  configuration fails before network access.
- Decision: treat an empty valid structured findings list as a reviewer pass.
  Evidence: `ReviewResult` already has a strict report-only termination
  contract and represents success as “No findings reported.”
  Consequence: no second, model-authored confidence/verdict protocol is needed.

## Surprises and Discoveries

- Exact review pricing cannot be inferred from token totals alone: a priced
  request that omits any required usage subset must fail the shared budget
  rather than silently undercounting.
- Immutable no-overwrite packet publication is safest as a synchronized
  private staging file followed by a hard-link publication. This preserves
  content-addressed identity without permitting replacement races.

## Recovery and Rollback

Reverse only REV-221-labelled hunks and delete only REV-created paths with
`apply_patch`. Tests use disposable profile/workspace roots. No provider,
credential, user session, protected tree, or installed binary is mutated.

## Completion Record

Changed files: additive `src/acceptance.rs` and
`src/review_convergence.rs`; narrow configuration, review-runner, headless,
CLI/TUI state/command/orchestration, runtime-path, and documentation changes.

Commands and results: convergence 3/3, acceptance 4/4, review 5/5, headless
pass/repair 2/2, configuration 2/2, and CLI parser coverage pass. `cargo fmt
--all -- --check`, strict workspace all-target/all-feature Clippy,
`cargo build --workspace --release --all-features`, `git diff --check`, zero
`.snap.new`, and direct oracle/reference inspection pass.

Remaining risks: real configured reviewer routes were deliberately not
contacted. TUI repair remains an ordinary explicitly initiated agent turn;
each subsequent `/review converge` is a fresh one-pair invocation, while the
headless `--check-repair` path owns the bounded automatic three-round loop.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`,
`TESTING.md`, the knowledge index, and the ORC-218 umbrella record the final
authority, evidence, and validation boundaries.
