# LRN-175 — Closed-Loop MiMo/Hermes Learning

Status: COMPLETE (2026-08-25)
Owner/session: current user-requested implementation
Last updated: 2026-08-25
Completed only after the additive correction path succeeded against the real
provider and committed a fully decided archive-wide run.

## Goal

Make one direct `/learn` investigation recover MiMo-level procedural signal,
make or refine complete Hermes-style skill trees, decide every workflow cluster
before retiring its evidence, and publish all changes through CA's existing
recoverable provenance boundary.

## Non-Goals

- Do not copy or modify oracle implementation source.
- Do not expose raw tool output, email, secrets, reasoning, incomplete turns, or
  subagent chatter to the learning model.
- Do not add a credential-free execution sandbox or run generated scripts while
  publishing them.
- Do not install dependencies, plugins, schedules, services, or a release
  binary.
- Do not broaden background maintenance to the manual adaptive budget.

## Evidence and Current Behavior

- `LearningCorpus` already indexes all eligible archive evidence and bounds
  adaptive search/inspection responses independently.
- `WorkflowCluster` currently contains only an ID, label, and session IDs;
  final `LearningDecision` rows do not identify clusters.
- `retired_ids_for_coverage` treats complete catalog-page delivery as complete
  coverage, so the real run retired 1,533 messages after inspecting eight
  sessions without explicitly deciding all 116 clusters.
- The model-facing projection currently contains sanitized requests,
  conclusions, and safe tool inputs but loses action outcomes, recoveries, and
  stopping conditions.
- CA's portable registry discovers `SKILL.md`, references, scripts, assets, and
  agent metadata, while auto-distill publication is restricted to one inert
  `SKILL.md` and has no create/patch/file lifecycle.
- The completed real run `9c24e870-3ae0-4036-b95d-80879b283659` used 225,464
  input tokens and created one generic command. Its coverage must be reopened
  exactly once while preserving that command.
- Read-only MiMo evidence shows trajectory-first workflow extraction; official
  Hermes behavior supports targeted learning and complete skill-tree
  create/patch/refinement. These are behavioral references only.

## Scope

Allowed files: `src/learning.rs`, `src/codex_learning.rs`, `src/skills.rs`,
direct command/tool/orchestration call sites, focused TUI tests/snapshots, and
current learning harness documents.

Forbidden files/areas: `oracles/`, `reference/`, credentials, installed
binaries/services, Git history, and unrelated dirty worktree changes.

Runtime state: automated validation uses temporary profiles. After every gate
passes, the repository-built CA may run bare `/learn` exactly once against the
existing profile, as explicitly authorized. External failure is terminal for
that attempt and must not retire or publish incomplete evidence.

## Design and Invariants

- Bare `/learn` searches the full recent archive and current conversation.
  Focus text remains an archive query and, when it resolves to an explicit
  procedure/path/URL, adds one bounded untrusted source to the ephemeral index.
  `/distill` shares the evidence path but cannot mutate memory.
- Host-derived action projections may contain input shapes, success/failure,
  stable error class, recovery, correction, outcome, and stopping condition;
  they never contain raw output bodies or authority-bearing content.
- Every deterministic cluster receives an explicit final outcome. Equivalent
  clusters may share a decision only when every stable cluster ID is listed.
  Catalog delivery is discovery, not retirement authority.
- Retired evidence remains searchable historical support. New matching
  evidence, concrete use failure, or user correction reactivates the cluster.
- Full skill trees are digest-pinned, path-confined, text-bounded, staged, and
  journaled. Human-authored writable skills may be changed only by exact digest;
  read-only ownership becomes a conflict recommendation.
- Structurally valid generated skills are active immediately. Runtime execution
  uses existing CA approvals; metadata cannot grant tools, models, permissions,
  dependency installation, network, or credentials.
- Publication never executes scripts. Same-task refinement requires a pinned
  invoked skill plus concrete failure/recovery/correction evidence, allows at
  most two repair submissions, and commits at most one skill transaction per
  user turn. Automatic deletion is forbidden.
- Memory, prompt commands, agents, complete skill trees, and coverage retirement
  share one recoverable publication journal and one undo receipt.

## Milestones

### M1 — Characterize cluster decisions and procedural projection

- Add failing tests for explicit cluster coverage, outcome/recovery projection,
  and the named 1,533-message reopening.
- Validate with focused learning and archive-learning tests.

### M2 — Unified targeted evidence and truthful retirement

- Add bounded path/URL/current-workflow evidence ingestion.
- Add action outcome shapes, stable cluster digests/IDs, explicit cluster
  decisions, reactivation, context compaction, and versioned coverage journals.
- Preserve the full searchable archive while reopening only the named run.

### M3 — Full-tree skill management and feedback

- Add typed create/patch/edit/delete/write/remove operations, complete-tree
  validation/digests, atomic publication/undo, immediate registry refresh, and
  active script/dependency-bearing skills.
- Pin invoked skill digests and add bounded same-task refinement evidence.

### M4 — Product reporting and end-to-end fixtures

- Add the late BSD fixture, targeted source fixtures, lifecycle/rollback/privacy
  coverage, and wide/narrow transcript snapshots.
- Prove the clear case completes in two learning turns and every cluster receives
  a validated decision.

### M5 — Completion gates and one real run

- Run focused suites, full tests, formatting, strict Clippy, and release build.
- Reconcile continuity/index/testing documents and archive this plan.
- Run repository-built bare `/learn` exactly once and record the result without
  automatic retry.

## Acceptance Criteria

- Every eligible session/message remains searchable while each tool result stays
  bounded; clear late BSD evidence creates `check-rsync`,
  `check-transmission`, and `check-seedbox` in two provider turns.
- No complete revision retires until all clusters have explicit validated
  outcomes and publication commits. Partial/failing/cancelled work retires only
  the policy-authorized subset or nothing.
- The named real run reopens exactly 1,533 IDs once, preserves its command and
  memory, and leaves older journals untouched.
- One explicit stable workflow, bounded path, or bounded URL can create a skill;
  untrusted content and excluded data cannot issue instructions or enter the
  persistent index.
- Complete skill trees can be created and exactly patched with safe support
  files, are active immediately after structural validation, and remain bounded
  by normal runtime approvals.
- Concrete use feedback can refine the pinned skill once in the same user turn;
  digest drift, invalid trees, read-only ownership, or rollback failure never
  overwrite the prior skill.
- Final receipts report source totals, every cluster decision, created/extended
  assets, skipped/needs-evidence items, six evidence states, validation, model,
  turns, usage, run ID, and a working undo command.

## Validation

Focused: learning corpus/projection, adaptive protocol, archive retirement and
migration, skill registry/management, command execution, direct publication,
and reviewed wide/narrow snapshots.

Completion: `cargo test --workspace --no-fail-fast -- --test-threads=1`,
`cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`, and
`cargo build --workspace --release --all-features`.

Manual: exactly one real bare `/learn` after automated gates pass.

Blocked checks: record unavailable provider/credential/network stages rather
than retrying or weakening validation.

## Progress

- [x] Existing LRN-171A corpus, cluster, retirement, journal, publication, and
  skill-registry boundaries traced.
- [x] Failing cluster-decision, procedural-projection, and reopening tests added.
- [x] Unified targeted evidence and truthful cluster retirement implemented.
- [x] Full-tree skill lifecycle and same-task feedback implemented.
- [x] Product reporting, fixtures, and snapshots implemented.
- [x] Focused and completion gates passed.
- [x] One authorized real bare `/learn` completed or its external failure
  recorded.

## Decisions

- Decision: do not add a disposable execution sandbox.
  Evidence: explicit user choice.
  Consequence: publication performs structural validation only; real execution
  remains behind ordinary CA runtime approvals.
- Decision: structurally valid generated skills are active immediately,
  including scripts and dependency declarations.
  Evidence: explicit user choice.
  Consequence: missing dependencies fail visibly on use and are never silently
  installed.
- Decision: explicit outcomes are required for every cluster.
  Evidence: the completed real run retired the archive after catalog delivery
  but weak investigation.
  Consequence: page delivery alone cannot mark evidence complete.
- Decision: exact-digest mutation may include writable human-authored skills.
  Evidence: explicit user choice.
  Consequence: ownership is preserved by digest drift refusal, journaling, and
  undo rather than blanket immutability.

## Surprises and Discoveries

- The current repository already has a full portable skill-tree reader; the
  missing work is safe mutation/publication and lifecycle integration rather
  than a second incompatible skill format.
- The one authorized live run correctly reopened the named 1,533-message
  coverage-less revision and delivered both catalog pages, but its correction
  turns replaced rather than extended the prior structured submission. After
  eight turns, 44 of 116 clusters still lacked an explicit decision. The run
  failed before publication and retired nothing.
- The adaptive protocol now retains every otherwise-valid partial submission,
  merges only non-conflicting new cluster decisions and artifacts, and returns
  the complete missing-ID set. A 116-cluster regression proves a 72-cluster
  first submission plus a 44-cluster supplement validates in two provider
  turns without replaying the first payload.
- Compacting a retained submission initially rewrote the visible tool call but
  left the provider's opaque canonical Responses item attached. OpenAI rejected
  that inconsistent replay. Retained submissions now discard that opaque item
  when they become host summaries; a canonical-response regression exercises
  the same path.
- Failed or exhausted completed invocations are terminal rather than startup
  resume candidates. Their evidence remains eligible and their progress is
  cleared, while a true process interruption still retains its resumable
  checkpoint.
- Real run `58f65db2-272f-464c-bac3-b610be749ef3` exercised two additive
  correction rounds, retained 64 decisions before the last round, validated all
  116 clusters on turn eight, and committed successfully.

## Recovery and Rollback

Revert only LRN-175 additions without reset, clean, stash, or broad formatting.
Publication stages exact prior tree bytes/modes and restores them through the
same journal. A failed real run publishes and retires nothing incomplete.

## Completion Record

Changed files:

- `src/learning.rs`: complete cluster decisions, procedural evidence,
  full-archive adaptive retrieval, bounded targeted sources, retained-context
  compaction, additive final-submission repair, generated complete skill trees,
  and focused fixtures.
- `src/codex_learning.rs`: direct-source ingestion, exact legacy reopening,
  truthful coverage/retirement, atomic publication, registry refresh, compact
  receipts, and undo integration.
- `src/skills.rs`: complete-tree validation and digests, typed `skill_manage`
  create/patch/edit/delete/write/remove lifecycle, recovery journals, runtime
  dependency preflight, and pinned same-task refinement.
- `src/agent/{registry,tools}.rs`, `src/tool_selection.rs`, `src/main.rs`, and
  `src/prompt_commands.rs`: tool registration, ordinary approval routing,
  skill invocation pinning/refinement evidence, and immediate slash discovery.
- `src/tui/render_tests.rs` and learning snapshots: wide/narrow running and
  final coverage reports.
- `CONTINUITY.md`, `NEXT_STEPS.md`, `docs/.INDEX.md`, and this plan: durable
  implementation, validation, and live-attempt record.

Commands and results:

- Focused learning suite: 94 CA tests plus 18 embedded learning tests passed.
- Focused skill, prompt-command, and TUI render suites passed (21, 4, and 177
  tests respectively).
- `cargo test`: passed across CA, CLI, embedded scheduler, and integration
  suites; only the documented linker compact-unwind warning appeared.
- `cargo fmt --check`: passed.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `cargo build --release`: passed; the artifact was not installed.
- `git diff --check`: passed and no `.snap.new` files remain.
- After the user explicitly requested another real attempt, repository-built
  bare `/learn` run `58f65db2-272f-464c-bac3-b610be749ef3` used
  `openai-codex/gpt-5.6-sol`, delivered 2/2 catalog pages, and committed after
  eight provider turns. It made all 186 sessions and 1,551 messages searchable,
  retrieved 236 messages from 69 sessions, inspected 113 messages from 11
  sessions, cited 329 messages from 66 sessions, explicitly decided all 116
  clusters, and retired exactly 1,551 messages. The zero-change conclusion was
  successful and truthful: existing reusable assets were inventoried, no new
  memory or skill package was warranted, and structural validation passed. The
  receipt reported 148,767 input plus 8,611 output tokens, run-scoped undo
  `/learn undo 58f65db2-272f-464c-bac3-b610be749ef3`, and no publication-time
  script execution or dependency installation.

Remaining risks:

- The real archive was ambiguous enough to consume the full eight-turn budget;
  the two-turn target remains proven by the deterministic clear-case fixture,
  not this heterogeneous profile.

Harness updates:

- `CONTINUITY.md` records the final behavior and successful live receipt.
- `NEXT_STEPS.md` marks LRN-175 complete and points to this archived plan.
