# LRN-171 — Direct Adaptive MiMo-Style Learning

Status: COMPLETE — real-profile source discovery regression repaired and verified
Owner/session: current user-requested implementation
Last updated: 2026-08-24

## Goal

Replace the manual archive-review ceremony with one direct, visible, adaptive
learning run. `/learn [focus]` performs memory consolidation plus reusable-asset
distillation; `/distill [focus]` performs asset distillation only. Both publish
only validated output and finish with a compact MiMo-style receipt.

## Non-Goals

- Do not copy MiMoCode implementation source or modify any oracle/reference.
- Do not expose raw tool outputs, email, credentials, incomplete turns, or
  subagent chatter to the learning model.
- Do not permit learned source, dependency, plugin, permission, model, or
  scheduled-execution changes.
- Do not overwrite human-authored reusable assets.
- Do not contact a live provider, install a binary, or mutate the user's real
  learning profile while implementing or validating this change.

## Evidence and Current Behavior

- The supplied CA screenshots show a 142-page evidence pager, exact-digest
  modal, blank apply pager, and a terminal `invalid` retry state.
- `src/codex_learning.rs` implements that review/approve/proposal state machine
  and materializes the full bounded corpus before a strict one-shot request.
- `src/main.rs` has a separate `/distill run` maintenance path, but it is silent,
  current-store-only, and publishes only skills.
- `src/learning.rs` has no native command artifact; the completed LRN-170 plan
  deliberately treated a skill as CA's “command-like” form.
- The read-only MiMo oracle performs one direct manual Distill session, queries
  trajectory and memory adaptively, inventories assets, chooses the smallest
  form, and returns Shortlist/Created/Skipped/Needs-more-evidence.
- Inferred: the product failure is the manual review protocol and rigid prompt
  architecture, not merely the number of provider calls.
- A real manual run on 2026-08-24 still stopped before provider work. The first
  invocation sent CA's native `*.turns.jsonl` bundle to the Codex rollout parser
  and reported `archive file has no valid first session_meta`; the next stopped
  on a valid 96,100,172-byte Codex rollout because of the legacy 64 MiB
  per-file ceiling.
- Read-only MiMo source evidence confirms that Distill works from the native
  indexed session store, treats raw trajectory as corroboration, prefers bounded
  recent inspection, and permits a successful no-op. One malformed trajectory
  entry is not a global preflight ceremony or terminal state.

## Scope

Allowed files: learning/archive state and generation, reusable-command
discovery/execution, TUI command/input/transcript orchestration, focused tests
and snapshots, and current harness documentation.

Forbidden files/areas: `oracles/`, `reference/`, credentials, installed
binaries/services, Git history, unrelated dirty changes, and arbitrary source
or external-application mutation.

Runtime state affected after a future user invocation: active-profile
learning/memory journals, profile auto-distilled generations, and validated
workspace `.ca/{commands,skills,agents}` artifacts. Implementation tests use
temporary profiles/workspaces only.

## Design and Invariants

- A direct manual invocation is the bounded authorization. No evidence pager,
  digest approval, proposal modal, or second Apply action remains in the TUI.
- Build a transient 30-day evidence index from native, Codex, and MiMo sources.
  Persist only identifiers, revisions, digests, counts, checkpoints, and
  journals.
- Start with a small deterministic overview, then allow bounded host-controlled
  search/session/asset tools. Five investigation turns, one forced-final turn,
  and two repair turns form the eight-turn maximum. Invalid submissions are
  repaired inside the same run.
- `/learn` may publish memory plus assets; `/distill` may publish assets only.
  Candidate creation requires high confidence and either two distinct sessions
  or one explicit recurring user instruction with a stable procedure.
- Project-specific evidence publishes only under the current workspace `.ca`
  roots. Cross-workspace or explicitly global evidence may publish only into
  active-profile auto-distill generations and may not contain repository paths.
- Add inert prompt commands with literal `$ARGUMENTS`/`$1`–`$9` substitution.
  Built-ins cannot be shadowed and invocation never evaluates a shell.
- Skills keep the existing validator. Generated agents have no model override
  and an explicit read-only tool allowlist. Automation remains a recommendation.
- Only exact prior auto-distilled generations may be replaced. Publication and
  undo remain journaled and recoverable.
- Manual progress and the final receipt render in the normal transcript. Raw
  evidence remains private and never becomes pager content.

## Milestones

### M1 — Characterization and command contract

- Add failing tests for direct command parsing, absence of review actions, and
  native command discovery/expansion/collision rules.
- Implement validated workspace/profile command discovery and ordinary-prompt
  invocation.

### M2 — Adaptive evidence and generation protocol

- Add the transient evidence overview/search/session reader.
- Add the eight-turn tool loop, strict final schema, correction behavior,
  bounded request sizes, checkpoints, and scripted fake-provider coverage.

### M3 — Scoped publication and recovery

- Extend output/publication to commands and safe agents alongside memory and
  skills, with evidence-derived scope, conflicts, recovery, registry refresh,
  and undo.

### M4 — Direct transcript workflow

- Route `/learn` and `/distill` into the adaptive runner, retain explicit
  status/sync/on/off/undo controls, and turn legacy review/approve into direct
  compatibility aliases.
- Remove manual learning modal/pager transitions and render inline stages plus
  the final inventory/shortlist/receipt.

### M5 — Validation and reconciliation

- Run focused tests and reviewed snapshots, then every Rust completion gate in
  `TESTING.md`.
- Reconcile continuity, next steps, architecture/parity/testing docs, archive
  this plan, and record actual validation evidence.

## Acceptance Criteria

- The supplied BSD fixture yields three parameterized command candidates;
  variable dedup remains a skill/needs-evidence candidate and one-off FreeBSD
  investigations are skipped.
- A 797-message/17-workspace/44-thread fixture has bounded overview/retrieval
  payloads, completes the clear scripted case in two turns, and never renders a
  revision pager or approval modal.
- The runner never exceeds eight logical turns; a malformed final submission is
  corrected without another user command.
- MiMo SQLite remains query-only without WAL/SHM creation. Secrets, tool output,
  email, subagent, and incomplete evidence are absent from model-facing data.
- Custom commands are immediately discoverable, expand arguments literally,
  cannot shadow built-ins, and are undoable when auto-generated.
- Human-authored assets are never replaced. Project/profile authority,
  generation conflicts, interrupted publication, recovery, and undo are tested.
- Zero creation is a successful evidence-backed receipt.
- Native CA bundle roots are indexed through their metadata/history contract,
  not the Codex `session_meta` parser; invalid sibling bundles are skipped
  without discarding valid sessions.
- A valid Codex rollout larger than 64 MiB is streamed with independently
  bounded records and evidence, and does not abort source discovery. Unsupported
  individual archives do not abort valid sibling evidence.

## Validation

Focused: learning/archive/command/TUI unit tests and wide/narrow direct-run
snapshots.

Completion: `cargo test --workspace --no-fail-fast`, the all-target/all-feature
matrix, `cargo fmt --all -- --check`, strict workspace Clippy, all-feature
release build, diff/snapshot hygiene, and protected-tree status.

Manual/visual: deterministic fake-provider and disposable temporary-profile
checks only.

Blocked checks: live provider quality, real profile publication, service
activation, and installed-binary replacement are outside implementation
validation.

## Validation record

The final MiMo behavior audit added an end-to-end first-run proof rather than
another isolated stage test. A fake two-turn provider starts from automatically
discovered native sessions, uses adaptive search, publishes both a workspace
memory convention and `.ca/commands/check-rsync.md`, renders the final receipt,
and removes both through the aggregate undo journal. No review-era command is
called anywhere in the test.

Session titles are now available only in the ephemeral corpus and ranked search;
they are sanitized, independently bounded, and absent from persisted source
indexes. MiMo SQLite title discovery remains query-only and creates no WAL/SHM.
The exact Dream failure was also traced to Unicode-character truncation followed
by byte-length validation. New sanitized learning text is byte-bounded and
idempotent, so the unchanged 8,234-byte failure cannot recur. A product-path
regression builds that multibyte corpus, queues it through the enrolled
supervision bridge, reloads/validates it, and exports it successfully.

`cargo test --bin ca learning::tests -- --test-threads=1` passed all 79 focused
tests. The final default workspace matrix passed 2,567 tests with three
documented ignores; the all-target/all-feature matrix passed 2,714 with four.
Formatting, strict workspace/all-target/all-feature Clippy, the all-feature
release build, and release help/version smokes passed.

The reopened audit reproduced both reported failures before changing source
discovery. Five focused regressions now cover native CA bundles, automatic
native discovery, unsupported Codex siblings, a sparse valid rollout above the
legacy 64 MiB boundary, and exclusion of native email/incomplete turns.
`cargo test --bin ca codex_learning::tests -- --test-threads=1` passed all 49
tests.

`cargo test --workspace --no-fail-fast -- --test-threads=1` passed 2,560 tests
with three documented manual/external ignores. The all-target/all-feature
matrix passed 2,707 tests with four documented ignores. Formatting, strict
workspace/all-target/all-feature Clippy, the all-feature release build,
release help/version smokes, diff/snapshot hygiene, and protected-tree status
all passed.

The optimized uninstalled binary then previewed the actual registered source
roots read-only: 151 files, 1,063,142,977 bytes, and 1,632 completed candidate
messages in 3.91 seconds. It skipped six secret-shaped messages, made zero
provider calls, and made zero profile writes. This proves the native bundle and
96 MB rollout that stopped the supplied run no longer abort discovery without
performing a live learning publication.

Validation did not contact a live provider, read credentials, mutate the
user's real learning profile, activate a service, install a binary, or modify
oracle or reference content.

## Progress

- [x] User screenshots, current CA paths, and read-only MiMo behavior inspected.
- [x] Direct/publish, evidence-based scope, and deep-adaptive preferences fixed.
- [x] Characterization tests and reusable-command contract.
- [x] Adaptive evidence tools and eight-turn runner.
- [x] Scoped publication, recovery, and undo baseline.
- [x] Direct transcript workflow and compatibility aliases baseline.
- [x] Visible bounded investigation cards matching the MiMo interaction shape.
- [x] Logical-turn interruption/resume without persisting transcript text.
- [x] Active conversation model preference with safe configured fallback.
- [x] Atomic publication/recovery proof across every generated asset type.
- [x] Re-run focused and completion validation after the audit repairs.
- [x] Reconcile the completion record only after every requirement has proof.
- [x] Reproduce the real native-bundle and 96 MB rollout failures in focused
  tests, then repair source discovery without weakening content bounds.
- [x] Re-run focused and completion gates and reconcile the completion claim
  against the real-profile failure.

## Decisions

- Decision: manual `/learn` and `/distill` are direct; background scheduled
  maintenance stays conservative.
  Evidence: MiMo Distill is manual and the user selected direct/publish.
  Consequence: the adaptive eight-turn budget is never silently spent by the
  scheduler.
- Decision: implement a real inert command format.
  Evidence: the supplied useful MiMo result creates commands and CA has no
  equivalent runtime.
  Consequence: skills no longer masquerade as commands.
- Decision: model access to trajectory is typed and host-controlled rather than
  arbitrary bash/SQLite.
  Evidence: adaptive retrieval is needed for intelligence, while the repository
  guardrails require read-only bounded evidence and secret exclusion.
  Consequence: observable investigation behavior is reproduced with a smaller
  attack and latency surface.

## Surprises and Discoveries

- LRN-170 reduced call count but explicitly retained exact review approval and
  rejected native command support, preserving the two user-visible mismatches.
- The first LRN-171 publication pass wrote its aggregate journal only after
  memory, skills, commands, and subagents had already changed. The audit moved
  publication to prepared/applied/committed journals with synchronous rollback,
  startup recovery, grouped multi-workspace undo, and evidence re-eligibility.
- Escape could previously abort the provider task while leaving resumable inert
  state, and a very late Escape could race the receipt. A profile-local cancel
  marker now binds pending evidence or the exact just-committed run and consumes
  it without marking cancelled evidence processed.
- Corpus compaction chose the historically dominant workspace as the `.ca`
  publication authority. Direct runs now bind the active TUI workspace; when it
  has no supporting evidence, no historical workspace is treated as current.
- A persistent checkpoint originally resumed only the provider-call envelope.
  The adaptive checkpoint now records completed logical turns and selected
  session identifiers only, so completed investigation turns are not replayed.
- The completion fixture covered native history only through the older
  `LearningCorpus` path. Manual direct discovery separately registered the
  native session directories but parsed every JSONL as a Codex rollout, so the
  first real CA bundle aborted the run. The 64 MiB per-file guard then made a
  tool-heavy but valid 96 MB Codex rollout a second global hard stop.
- Native roots now reuse `SessionStore` metadata/history validation and retain
  only recent completed user/assistant evidence plus safe tool-action
  summaries. Codex rollouts stream with a 1 GiB file ceiling, independently
  bounded 1 MiB records and 8 KiB retained messages, while the aggregate local
  scan has 4 GiB of headroom. Unsupported individual archives are skipped, and
  first-time standard-source registration no longer triggers a redundant
  second full scan.
- MiMo's operator searches trajectory titles and related workflow terms rather
  than requiring one exact phrase in message bodies. CA originally omitted
  titles from its transient corpus and used exact phrase matching, which could
  miss the same repeated workflows even after source discovery succeeded.
- Dream sanitization originally bounded Unicode scalars while its validator
  bounded UTF-8 bytes. Multibyte evidence could therefore be safely unchanged
  yet permanently over the byte limit. The sanitizer now has one byte contract
  and is idempotent under it.

## Recovery and Rollback

Remove only LRN-171 command/adaptive/direct-flow changes and matching tests/docs.
Do not reset, clean, stash, or overwrite surrounding dirty work. State readers
must preserve older versions or retire their pending review metadata without
marking evidence processed.

## Completion Audit

Closed. The reported native-bundle, 96 MB rollout, missing-pending-review, and
8,234-byte Dream failures have exact regressions. The disposable product-level
test completes first-run learning, memory/command publication, and aggregate
undo in one invocation and two provider turns. Both repository matrices pass,
and the optimized uninstalled binary completed a read-only preview of the
actual 1.063 GB source set in 3.91 seconds with zero provider calls and zero
profile writes.
