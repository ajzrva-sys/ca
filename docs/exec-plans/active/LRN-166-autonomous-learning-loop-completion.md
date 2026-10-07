# LRN-166 — Autonomous Learning Loop Completion

Status: ACTIVE — background activation deferred by user's manual-learning decision
Owner/session: Codex / current user-requested implementation
Last updated: 2026-08-23

## Goal

Make one user-scoped background supervisor continuously harvest the default
`ca` session store and the active/archived Codex JSONL roots, replay-evaluate
bounded Dream/Distill candidates, and publish only approved memory or inert
skill artifacts without per-run approval prompts.

## Non-Goals

- Do not permit learned source, binary, dependency, provider, routing,
  permission, plugin, executable, or external-communication changes.
- Do not import Codex transcripts into ordinary `ca` sessions or persist raw
  archive text in the archive index or supervisor state.
- Do not change oracle/reference files, expose credentials, or absorb unrelated
  dirty-tree work.
- Do not install runtime artifacts before the exact final activation preview is
  reviewed once.

## Evidence and Current Behavior

- `src/learning.rs` already owns bounded 7/30-day Dream/Distill state,
  sanitized native-session corpora, supervised intent export, journals, and
  exact undo, but only the idle interactive TUI creates an intent.
- `src/codex_learning.rs` already streams and deduplicates Codex active/archive
  JSONL, but its LaunchAgent performs indexing only and provider/application
  work is held behind two manual reviews.
- `crates/codex-team/src/self_improvement/{supervisor,learning_loop}.rs` already
  own a continuous health-gated supervisor, frozen replay evaluation,
  independent judgment, immutable candidates, promotion, pause, and rollback.
  The supervisor acknowledges a `ca` intent immediately after import and
  publishes promoted artifacts only inside its vault.
- The installed `ca` reports zero archive sources and no scanner service. The
  machine learning supervisor reports unenrolled, its LaunchAgent is absent,
  and the required execution profile does not exist.
- The current repository is intentionally dirty and the learning source/plan
  files are untracked. Enrollment V1 requires clean repositories even though
  this slice grants no source or binary learning authority.

## Scope

Allowed files: `src/{learning,codex_learning,main,app,config,runtime}.rs`, focused
`src/tui/**`, `crates/codex-team/src/{app,cli,config}.rs`,
`crates/codex-team/src/self_improvement/**`, the supervisor binary, focused
tests/snapshots, and current harness documentation.

Forbidden files/areas: `oracles/`, `reference/`, unrelated dirty source,
provider secret values, saved ordinary session contents, and source/binary
self-modification paths.

Runtime state affected after exact confirmation: default-profile learning and
memory/auto-distill state, registered read-only Codex roots, a private
machine-learning vault, a least-authority execution profile, installed verified
learning binaries, and one user LaunchAgent.

## Design and Invariants

- The stable supervisor is the only recurring service. Each 30-second tick
  performs health/recovery, runs at most one bounded workflow stage, and asks
  `ca` to harvest only when the 15-minute scan lease or a 7/30-day maintenance
  deadline is due.
- Learning bridge V2 separates idempotent harvest/export from terminal
  completion. Export never deletes evidence. Terminal completion binds the
  enrollment, intent, candidate, evaluation, artifact content, and destination
  state digests; only successful publication or terminal rejection removes the
  queued intent.
- Default is legal as an evidence/publication profile but never as a general
  `role-run` profile. Provider work stays in a dedicated required-sandbox
  execution profile with exact enrolled routes and credential references.
- Native and Codex evidence share the existing sanitized `LearningCorpus`
  contract and stable IDs. Index state contains no bodies; materialized text is
  bounded and transient or carried only in the existing private intent/vault
  evidence records.
- Promotion accepts only Memory and Skill. Memory re-enters the Dream CAS
  journal; skills re-enter the auto-distill validator/generation journal.
  Duplicate terminal delivery is a no-op with the same receipt.
- One knowledge-only enrollment binds exact installed component digests,
  evidence roots/profile, execution routes, automatic memory/skill policy, and
  unbounded metered spend. It does not require a clean source checkout because
  source and binary candidate activation are absent from this contract.
- Foreground leases, corrupt/drifted state, route drift, low disk, provider
  failures, and publication conflicts defer or pause; they never widen
  authority or silently discard evidence.

## Milestones

### M1 — Characterization and protocol

- Add failing tests for background native/archive harvest, retained-until-final
  intent lifecycle, exact terminal publication, and single-service status.
- Add the V2 bridge/enrollment DTOs while retaining V1 parsing compatibility.

### M2 — Autonomous harvest and publication

- Add default-profile host harvesting independent of the TUI and fold archive
  scanning/materialization into the same bounded intent producer.
- Deliver replay-approved memory/skill artifacts back through existing
  journals; reject every other artifact class before mutation.

### M3 — Unified service and UI

- Route `ca learn status|pause|resume|run|service ...` and `/learn` to the
  combined local/supervisor state while retaining compatibility commands.
- Replace the separate archive LaunchAgent path with the stable supervisor and
  deterministic migration/status handling.

### M4 — Validation and activation

- Run focused and workspace completion gates, review wide/narrow snapshots,
  reconcile harness documents, and produce one exact activation preview.
- After one exact confirmation, install/start the service, immediately harvest,
  and verify nonzero sources/evidence plus one bounded terminal workflow.

## Acceptance Criteria

- Closed fake-provider coverage proves native plus active/archive Codex
  evidence reaches one replay-evaluated memory/skill publication without a TUI
  or per-run approval.
- An imported intent survives interruption, rejection, provider failure, and
  publication conflict until one matching terminal completion is durable.
- Repeated scans/completions do not duplicate evidence, memory, skills, usage,
  or acknowledgements.
- No tested path can publish or activate source, binary, routing, permission,
  credential, plugin, dependency, executable, or ordinary-session changes.
- One installed service reports loaded/healthy, nonzero registered evidence,
  metered provider usage, next due work, last result, and exact undo state.

## Validation

Focused: PASS. Deterministic tests cover combined native/Codex active/archive
harvesting, the 15-minute lease, terminal archive acknowledgement, exact and
idempotent memory/skill publication, undo, default-profile bridge enrollment,
knowledge-only contracts, secret-free execution-profile materialization,
legacy-scanner migration, one RunAtLoad/KeepAlive LaunchAgent, and a
process-level Codex/DeepSeek/MiMo fake-provider skill promotion.

Completion: PASS before activation. `cargo test --workspace` passed 2,500 tests
with three manual-only ignores. `cargo fmt --all -- --check`, strict workspace
all-target/all-feature Clippy, the workspace all-feature release build,
`git diff --check`, and release help/version smokes pass.

Manual/visual: existing `/learn`, Dream, Distill, and autonomous-learning
cockpit snapshots all passed unchanged in the full suite. The read-only release
service preview binds the four evidence roots, three release binaries, isolated
execution profile, unbounded metered policy, and memory/inert-skill-only
authority with activation digest
`1aae1e531b984a353c8221a94bd14b8d8f7b87a6316dbd83e4fc35adde2e1bdd`.
That preview is now historical: LRN-167 adds an optional MiMoCode database root
and the user chose manual learning. A future background activation requires a
fresh preview and confirmation.

Blocked checks: LaunchAgent installation, live provider processing, and the
real terminal-workflow smoke are deferred unless the user requests background
learning again.

## Progress

- [x] Existing local maintenance, archive scanner, bridge, supervisor, cockpit,
      and live inactive runtime state characterized.
- [x] Failing autonomous harvest/publication/service tests recorded.
- [x] V2 harvest/completion and memory/skill publication implemented.
- [x] Unified command/TUI/service lifecycle implemented.
- [x] Focused and pre-activation completion validation recorded.
- [ ] Exact activation and live terminal workflow verified.

## Decisions

- Decision: complete the existing supervisor rather than introduce another
  daemon.
  Evidence: it already owns health, foreground deferral, stage serialization,
  replay evaluation, cooldown, metering, pause, and rollback.
  Consequence: the archive scanner becomes a harvest stage and only one
  LaunchAgent remains.
- Decision: acknowledge evidence only after terminal disposition.
  Evidence: current import-time acknowledgement loses the only `ca`-side queue
  record before evaluation/publication succeeds.
  Consequence: V2 completion is exact, retryable, and idempotent.
- Decision: use knowledge-only enrollment.
  Evidence: clean-repository and A/B binary enrollment is unrelated to the
  requested memory/skill-only authority and blocks this dirty integration.
  Consequence: installed binary digests remain bound without granting learned
  binary activation.

## Surprises and Discoveries

- The previously reported autonomous implementation contains both producer and
  consumer halves, but the producer runs only in an idle TUI and the consumer
  acknowledges before terminal publication.
- The archive scanner and autonomous supervisor currently install different
  LaunchAgents and have no archive-to-supervisor bridge.
- The first process-level bridge run exposed two protocol bugs that isolated
  unit tests did not: `ca` and the supervisor canonicalized completion JSON
  differently, and the export cursor was not advanced after terminal
  publication. Sorted-key canonical JSON and post-receipt cursor advancement
  now make retry/recovery exact.
- Knowledge-only activation can derive a dedicated required-sandbox profile
  from provider/model metadata plus external credential references without
  copying literal credentials, MCP commands, hooks, or approval rules.
- The first confirmed install attempt stopped before mutation because two
  independent processes serialized provider `HashMap` entries in different
  orders, changing the activation digest. The execution-profile document now
  uses ordered maps; a regression test and two independent release previews
  produce the same replacement digest.

## Recovery and Rollback

All profile/vault writes remain private atomic files with exact journals.
Service installation retains exact-content uninstall checks. A failed
activation removes only a newly written matching plist and leaves source
archives and queued evidence intact. Runtime publication uses existing Dream
and Distill undo receipts. Never reset, clean, stash, or overwrite unrelated
working-tree changes.

## Completion Record

Implementation and pre-activation validation are complete. Runtime activation
and one bounded real terminal workflow remain pending on the exact digest above;
the plan stays ACTIVE until those checks pass.
