# RND-095 — Strong-Planner / Cheap-Executor Cost Experiment

Status: COMPLETE (2026-08-12)
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Determine whether `ca` can lower expected dollars per successful coding task by using one strong planning request followed by cheaper execution, verification, and bounded escalation. Produce an evidence-backed design, a provider-neutral `reasoning_artifact` contract, a reproducible benchmark/cost method, and—only if the research supports it—a small feature-flagged offline prototype that does not alter normal `ca` execution.

## Non-Goals

- Extracting, decoding, decrypting, or exposing hidden chain of thought.
- Replaying opaque provider state across providers or models through undocumented behavior.
- Training or fine-tuning a model, changing provider defaults, or selecting a production routing policy without benchmark evidence.
- Sending live provider traffic, reading credentials, modifying user config/session state, or changing the normal TUI/headless/service paths.
- Claiming Cursor parity; this is an evidence-backed `ca` experiment.

## Evidence and Current Behavior

- `src/agent/stream.rs` and `src/agent/provider.rs` retain route-bound provider continuation state for exact canonical OpenAI, DeepSeek, and MiMo paths. It is deliberately scoped to provider/protocol/endpoint identity and is not a portable planning representation.
- `src/multi_run.rs` already provides an explicit process-only exact-route best-of-N workflow over isolated worktrees, while `src/agent/subagent.rs` owns bounded child execution.
- `src/usage.rs` records request kinds, tokens, outcomes, and optional exact configured pricing; unknown prices remain unknown.
- `src/context.rs`, `src/session.rs`, and `src/worktree.rs` provide checkpoint, durable transcript, and repository-isolation primitives, but no cross-model reasoning artifact exists.
- `src/memory.rs` is opt-in and approval-gated; it is not an automatic routing learner.
- Current OpenAI documentation describes opaque persisted reasoning and compatible-item replay within Responses continuations. It does not document a provider-neutral or arbitrary cross-model reasoning transfer contract.
- The user-named `stolen-thoughts` work concerns reasoning-extraction security research. Its extraction/decryption path is explicitly excluded from this task; only the high-level distinction between supported continuation and unsupported cross-model reuse is relevant.

## Scope

Allowed files:

- `docs/exec-plans/active/rnd-095-planner-executor-cost-experiment.md`, later moved to `completed/`
- `docs/STRONG_PLANNER_CHEAP_EXECUTOR.md`
- `docs/.INDEX.md`
- `NEXT_STEPS.md`, `CONTINUITY.md`, `ARCHITECTURE.md`, `PARITY.md`, `TESTING.md`
- `Cargo.toml`
- `src/bin/reasoning_artifact_experiment.rs`

Forbidden files/areas:

- `oracles/`, `reference/`, user config/session/Keychain data, provider defaults, normal request/event loops, TUI input/rendering, and installed binaries.

Runtime state affected:

- None by default. Any prototype is build-time feature gated, reads only explicit fixture/input files, performs no network I/O, and never writes config/session/runtime state.

## Design and Invariants

- Separate five concepts: ordinary visible history; visible lossy summaries; provider-signed or encrypted continuation state; server-side continuation handles; and a provider-neutral structured artifact.
- Treat opaque state as a same-route optimization only when the provider documents it. Never infer that cross-model acceptance means semantic portability or a supported API contract.
- The portable artifact contains conclusions and evidence needed for execution, not hidden chain of thought. It is schema-versioned, bounded, source-attributed, tied to selected repository dependencies, and screened for recognizable credential patterns. The artifact remains sensitive repository data; screening is not proof that arbitrary prose is secret-free.
- A production repository checkpoint must bind task input plus base commit/worktree state and selected path/symbol content digests. The offline prototype verifies selected existing dependencies and explicit must-not-exist create targets only; it stores but does not authenticate the full base/worktree/task identity.
- The executor receives only the artifact, explicit task constraints, current repository state, and tools it needs. Verification is independent and can request a bounded planner refresh or escalate.
- Evaluate expected dollars per successful task, not raw token count. Unknown provider prices remain unknown; benchmark inputs must record model/provider, token categories, retries, cache reads/writes, tool cost, and outcome.
- No learned routing decision becomes active automatically. Offline evidence may produce a reviewable policy proposal only.

## Milestones

### M1 — Provider and research fact map

- Review current official OpenAI, Anthropic, Google, DeepSeek, Xiaomi, and other material provider documentation.
- Classify supported continuation, opaque/signed state, caching, and cross-model claims.
- Review primary routing/cascade/agent-evaluation research and explicitly separate evidence from inference.

Validation: every material current-provider claim has a direct primary-source URL; unsupported cross-model behavior is labeled undocumented or speculative.

### M2 — `ca` architecture and artifact contract

- Trace multi-run, subagent, request, usage, checkpoint/session, memory, and worktree ownership.
- Specify `reasoning_artifact.v1`, invalidation rules, executor contract, verifier feedback, and escalation state machine.
- Define failure containment and authority boundaries.

Validation: one self-contained design maps each new responsibility to an existing or proposed module and does not widen normal runtime authority.

### M3 — Benchmark and cost model

- Define A/B/C variants, task stratification, repeated trials, success criteria, contamination controls, and measurements.
- Define expected cost, success-adjusted cost, break-even escalation rate, and cache-aware accounting.

Validation: a third party can reproduce the comparison without credentials embedded in fixtures or subjective outcome grading as the sole signal.

### M4 — Isolated prototype

- If M1-M3 support it, add a `planner-executor-experiment` Cargo feature and an offline experimental binary for schema generation/validation, staleness checks, and cost aggregation.
- Keep the default build and normal `ca` behavior unchanged.

Validation: focused feature-enabled tests pass; default help/version contain no experimental command; the prototype performs no network or writes.

### M5 — Harness reconciliation

- Record conclusions, actual commands/results, remaining uncertainty, and the next implementation seam.
- Mark the work as a `ca` extension with no Cursor parity claim.

Validation: docs agree on status and the plan is archived only after all claimed checks run.

## Acceptance Criteria

- The report directly answers technically possible versus speculative for each transfer mechanism.
- A provider matrix distinguishes summaries, signed blocks, encrypted state, continuation handles, prompt caching, and ordinary history.
- `reasoning_artifact.v1` has explicit bounds, provenance, freshness, task/plan/evidence/test/escalation fields, and a weak-executor contract.
- The benchmark compares strong-only, planner/executor, and cheap-only or cascade controls on identical isolated tasks and reports dollars per success.
- The cost model includes planner, executor, verifier, retries, escalation probability, cache read/write pricing, and tool fees.
- Any prototype is opt-in at compile time, offline, deterministic, best-effort screened for recognizable credentials, and absent from normal CLI/TUI behavior.
- No claim relies on extracting hidden reasoning or undocumented provider behavior.

## Validation

Focused:

- `cargo test --features planner-executor-experiment --bin ca-reasoning-artifact-experiment --no-fail-fast`
- `cargo run --features planner-executor-experiment --bin ca-reasoning-artifact-experiment -- schema`
- deterministic validation/cost fixtures documented in the completion record

Completion:

- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build --release`
- default released `--help` and `--version`

Manual/visual:

- Inspect prototype JSON output for stable ordering, no raw opaque-state fields or recognizable credential patterns, and actionable validation failures; continue treating arbitrary prose as sensitive.
- Confirm default `ca --help` does not expose the experiment.

Blocked checks:

- Live provider cost/quality trials require explicit credentials and spend authorization and are not authorized here.

## Progress

- [x] Read the user request and current repository harness.
- [x] Establish the active plan and safety boundary.
- [x] Complete current provider and research evidence review.
- [x] Complete architecture/schema/benchmark/cost design.
- [x] Decide and implement the isolated prototype.
- [x] Run focused and completion validation.
- [x] Reconcile harness documents and archive the plan.

## Decisions

- Decision: use an explicit provider-neutral artifact, not hidden or opaque reasoning, for cross-model handoff.
  Evidence: current providers expose mutually incompatible continuation mechanisms and do not document arbitrary provider-neutral replay.
  Consequence: the experiment tests whether distilled, verifiable work product helps a cheaper executor, not whether it can inherit the planner's private cognition.

- Decision: keep any prototype offline and behind a non-default Cargo feature.
  Evidence: the user asked for a small isolated experiment and the repository forbids credential/runtime mutations without explicit authority.
  Consequence: this task can validate contracts and accounting without spending money or altering normal behavior.

- Decision: treat Google's documented compatible Gemini model-switch state as a provider-specific benchmark arm, not a generic runtime abstraction.
  Evidence: Google instructs stateless callers to retain prior thought blocks across compatible model switches, while Anthropic explicitly requires stripping thinking blocks when changing Claude models and OpenAI publishes no general Sol/Terra/Luna state matrix.
  Consequence: the benchmark includes matched `GX-IR`/`GX-STATE` arms, while ordinary cross-model execution always uses fresh provider state plus visible IR.

- Decision: keep planner provenance and execution authority as caller-supplied, self-consistency-checked fields in this offline binary and require harness injection before live execution.
  Evidence: an offline JSON validator has no independent provider call, task source, approval broker, or repository snapshot from which to authenticate those values.
  Consequence: validation never authorizes execution, and the report explicitly lists authenticated provenance/full repository identity as a later integration boundary.

## Surprises and Discoveries

- The current OpenAI docs distinguish opaque persisted reasoning from visible reasoning summaries and describe compatible earlier reasoning items, but do not establish arbitrary cross-model semantic portability.
- OpenAI's current caching model includes separately metered cache writes and reads, so a cost comparison that counts only input/output tokens is incomplete.
- DeepSeek and MiMo expose Responses-shaped APIs with materially different continuation/effort semantics; wire compatibility is not behavioral compatibility.
- Independent review exposed freshness, aggregate-I/O, create-lifecycle, symlink-race, special-file, secret-screening, and price-provenance gaps. The prototype now uses explicit existing/create intent, one creator with dependency-ordered reuse, descriptor-relative no-follow/nonblocking Unix traversal, aggregate byte accounting, and date-stamped declared price cards; authenticated provenance remains intentionally unimplemented.

## Recovery and Rollback

Remove the feature-gated binary entry and its single source file, revert the research/harness additions with targeted patches, and leave all existing runtime modules untouched. Do not use destructive Git commands.

## Completion Record

Changed files:

- `Cargo.toml`
- `src/bin/reasoning_artifact_experiment.rs`
- `docs/STRONG_PLANNER_CHEAP_EXECUTOR.md`
- `docs/.INDEX.md`
- `ARCHITECTURE.md`, `CONTINUITY.md`, `PARITY.md`, `TESTING.md`, `NEXT_STEPS.md`
- this completed plan

Commands and results:

- focused feature tests: PASS, 16 tests.
- focused feature-bin strict Clippy: PASS.
- released schema/validate/cost disposable fixtures: PASS; schema was 12,313 bytes, matching dependency validation returned one stable digest, changed bytes failed with exit 2, and a one-success/one-failure fixture totaled exactly `$3` and `$3` per success.
- `cargo test --no-fail-fast`: PASS, 1130 unit tests, one existing ignored manual-Chromium test, and 36 CLI tests.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- default and experiment `cargo build --release`: PASS.
- released default `--help`/`--version`: PASS, `ca 0.1.0`, no experimental command.
- snapshot hygiene: PASS, zero `.snap.new` files.

Remaining risks:

- No live provider quality/cost trial ran; dollars-per-success improvement is unproven.
- The offline artifact's provenance, task/repository identity, policy, and approvals are caller supplied. A live harness must inject and authenticate them.
- `--repo` verifies selected paths, not Git HEAD, complete dirty state, submodules, task input, toolchain, or environment. The non-Unix fallback cannot provide the Unix descriptor-relative race guarantee.
- Secret scanning recognizes common patterns only; artifact prose remains sensitive repository data.
- Pricing is exact arithmetic over explicit date-stamped caller-declared rates and disjoint counters, not provider billing attestation.
- Google-specific compatible cross-model state still requires an authorized, model/version/modality-matched empirical arm; all other opaque cross-model transfer remains unsupported or undocumented.

Harness updates:

- `docs/STRONG_PLANNER_CHEAP_EXECUTOR.md` is the evidence-backed decision, architecture, schema, benchmark, cost, risk, and staged implementation reference.
- `ARCHITECTURE.md` and `docs/.INDEX.md` map the isolated binary; `PARITY.md` records an intentional `ca` extension with no Cursor/Ante claim.
- `CONTINUITY.md` records current scope and the next controlled-benchmark handoff; `TESTING.md` records exact gates; `NEXT_STEPS.md` marks RND-095 complete.
