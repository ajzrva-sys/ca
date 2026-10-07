# LRN-170 — MiMoCode-Style Learning and Distillation

Status: COMPLETE (2026-08-24)
Owner/session: current `/goal` task
Last updated: 2026-08-24

## Goal

Make one manual CA learning/distillation pass find repeated workflows from faithful recent trajectory evidence, inventory existing reusable assets, create and publish only high-confidence missing knowledge/skills, and finish in one resumable provider pass with a useful MiMoCode-style result summary.

## Non-Goals

- Do not copy MiMoCode source or make MiMoCode an executable dependency.
- Do not enable the background learning service, install a binary, contact a live provider, read credentials, or mutate the user's real learning/session/config state during implementation.
- Do not add GUI surfaces, scheduler activation, source-edit authority, plugins, dependencies, or arbitrary executable artifacts.
- Do not weaken exact-digest approval, read-only trajectory access, secret filtering, owner-only state, journaled publication, undo, or workspace authority checks.

## Evidence and Current Behavior

- User screenshots show CA presenting `3/43 bounded provider calls` while making no changes, whereas MiMoCode performs an adaptive raw-trajectory investigation, inventories assets, produces a compact shortlist, and attempts only high-confidence creations.
- `src/codex_learning.rs::materialize_mimocode_message` currently accepts only visible text parts. It discards tool calls and therefore removes the command sequences needed to recognize repeated operational workflows.
- `src/codex_learning.rs::scan_mimocode_session` queries `agent_id = 'main'`; the MiMoCode oracle documents and the supplied runtime screenshot show an empty `agent_id` as the main agent representation.
- `src/codex_learning.rs::compact_archive_session` retains at most eight first/last text messages, so even indexed procedure evidence can disappear before generation.
- `src/codex_learning.rs::generate_proposal_with_progress` runs separate rigid Dream and Distill requests. Current dirty source has already compacted the historical 43-call fan-out to two requests, but it still does not reproduce MiMoCode's unified outcome or evidence quality.
- `src/learning.rs::LearningCorpus::build` discards native tool-call inputs, and direct `/distill run` calls `generate_distill` without an existing-asset catalog.
- `oracles/MiMo-Code-main/packages/opencode/src/agent/prompt/distill.txt` establishes these observable behaviors: default 30-day window; raw trajectory as source of truth; memory as an index; existing skill/agent/command/plugin inventory; confirmation by repeated tool/command evidence; compact shortlist; smallest justified reusable form; creation only at high confidence; and explicit Created/Skipped/Needs-more-evidence output.
- `oracles/MiMo-Code-main/packages/opencode/src/session/auto-dream.ts` runs one dedicated Distill agent session rather than a fixed per-corpus fan-out.
- Baseline `cargo test --bin ca codex_learning::tests --no-fail-fast` passes 38 tests on 2026-08-24, proving the existing behavior before LRN-170 changes.
- Inferred: the dominant cause of poor output is evidence loss (main-agent filtering plus removal of tool actions), compounded by rigid two-stage generation and a result format that does not explain candidate decisions.

## Scope

Allowed files: `src/codex_learning.rs`, `src/learning.rs`, `src/main.rs`, focused TUI state/render/tests/snapshots only if the result view requires them, this plan, and current `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `PARITY.md`, `TESTING.md`, and `docs/.INDEX.md`.

Forbidden files/areas: `oracles/`, `reference/`, runtime profile/config/session/learning data, credentials, installed binaries/services, Git history, unrelated dirty work, and source/plugin/dependency publication authority.

Runtime state affected: none during implementation or validation. Later user-invoked learning continues to use the existing exact approval and journaled publication boundary.

## Design and Invariants

- Build a bounded, sanitized workflow-evidence projection that retains user/final-answer context plus non-secret tool name/input summaries and stable temporal/session identity. Never persist raw transcript or tool-input bodies; persistent indexes remain digest/metadata only.
- Recognize both current empty and compatibility `main` MiMoCode agent identifiers while excluding subagents, imports, incomplete/errored turns, synthetic/ignored parts, tool output bodies, email activity, and secret-shaped inputs.
- Prefer the newest timestamped sessions inside the fixed corpus bound and expose their timestamps to the learning pass. Retain older sessions as corroboration when capacity allows instead of hard-dropping potentially valuable evidence.
- Preserve enough evidence across up to the existing corpus bounds to count repeated workflow shapes. Compaction may truncate individual text but must not erase the bounded tool-action summary that establishes repetition.
- Inventory supported existing assets before generation. CA may choose the smallest supported inert form; where CA has no native custom-command format, a focused portable skill is the command-like reusable form rather than inventing an unsupported runtime.
- Replace reviewed archive Dream+Distill generation with one strict structured provider request returning memory operations, skill packages, and a bounded decision shortlist. Validate each existing Dream/Distill authority rule independently before checkpointing or publication.
- The reviewed call plan, pricing, progress, checkpoint, retry, and exact digest all bind one provider call. Legacy two/many-call checkpoints migrate only when their completed results can be reused without ambiguity; otherwise refresh requires a new approval.
- Direct `/dream run` remains Dream-only. Direct `/distill run` remains Distill-only but receives the same recent tool evidence and supported-asset inventory.
- Completion reports Shortlist, Created/extended, Skipped, and Needs more evidence. Empty creation remains a successful terminal outcome with an evidence-backed explanation.

## Milestones

### M1 — Failing trajectory-fidelity characterization

- Add fixtures for empty-agent MiMoCode main turns, intermediate tool-call parts, secret-shaped tool inputs, subagent exclusion, and recent-first bounded selection.
- Add native-session corpus coverage proving repeated safe tool actions survive while outputs/email/secrets do not.
- Add reviewed-plan coverage requiring one provider call and a decision summary.

### M2 — Faithful bounded workflow evidence

- Implement deterministic recent-first session selection and safe tool-action projection for MiMoCode and native CA sessions.
- Preserve workflow evidence through archive compaction and stable digest/revision calculation.
- Keep read-only/query-only SQLite behavior and digest-only persistent state.

### M3 — Unified one-pass reviewed generation

- Add the strict combined result schema and one-call generator.
- Bind review pricing/progress/checkpoint/resume to the one-call plan.
- Reuse current memory/skill validation, publication journals, conflicts, and undo.
- Give direct Distill the same supported-asset inventory and repeated-source standard.

### M4 — Useful terminal outcome

- Render a compact MiMoCode-style decision summary with evidence counts and created/skipped/needs-more sections.
- Update progress language and deterministic TUI snapshots only where the user-visible view changes.

### M5 — Completion validation and reconciliation

- Run focused scanner/corpus/generation/publication/TUI tests.
- Run every Rust completion gate required by `TESTING.md`.
- Reconcile durable docs, move this plan to completed, and record actual results and blocked live checks.

## Acceptance Criteria

- A MiMoCode fixture with empty main-agent IDs and repeated intermediate bash/tool inputs yields repeated-workflow evidence; subagent, output, email, secret, and incomplete evidence does not. When evidence exceeds the corpus cap, newer sessions are selected first without forbidding older corroboration inside the cap.
- The stored learning index contains only digests/metadata and the SQLite fixture proves read-only/query-only access without WAL/SHM creation.
- A large approved review executes exactly one structured provider request, checkpoints it atomically, and resumes without repeating a completed request.
- The combined result cannot publish a memory operation or skill that fails the existing authority, provenance, source-session, secret, size, generation, conflict, or journal checks.
- Direct `/distill run` inventories existing supported assets and does not propose a substantially duplicate skill in its fake-provider contract.
- A zero-creation result is successful and explains what was skipped or needs more evidence; a creation result names the published memory/skills and their evidence frequency/confidence without requiring a second run.
- `/learn approve` remains responsive, closable/reopenable, exact-digest-bound, drift-safe, and truthful about `0/1` through `1/1` progress.
- No oracle/reference file, credential, live provider, real runtime profile, service, or installed binary is touched by validation.

## Validation

Focused:

- `cargo test --bin ca codex_learning::tests --no-fail-fast`
- `cargo test --bin ca learning::tests --no-fail-fast`
- `cargo test --bin ca learning_review_ --no-fail-fast`
- focused command/render snapshot tests changed by this work

Completion:

- `cargo test --workspace --no-fail-fast`
- `cargo test --workspace --all-targets --all-features --no-fail-fast`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo build --workspace --release --all-features`
- `git diff --check`, snapshot hygiene, and protected-tree status

Manual/visual: deterministic fixtures and fake providers only.

Blocked checks: live provider quality, real profile publication, service activation, and binary installation are intentionally outside current authorization.

## Progress

- [x] User screenshots, current CA source, and read-only MiMoCode oracle behavior inspected.
- [x] Existing 43-to-2 call partial repair and the remaining evidence/architecture mismatch identified.
- [x] Baseline focused archive-learning suite recorded.
- [x] Failing trajectory-fidelity and one-pass characterization tests added.
- [x] Bounded workflow evidence implemented.
- [x] Unified one-pass reviewed generation implemented.
- [x] Direct Distill inventory and output-summary behavior implemented.
- [x] Focused and completion validation recorded.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: treat MiMoCode as behavioral evidence for this CA extension, not Cursor parity and not an implementation dependency.
  Evidence: repository policy and `PARITY.md` prohibit copying oracle implementation and scope Cursor separately.
  Consequence: tests reproduce observable inputs/outcomes with native Rust structures.
- Decision: tool inputs are evidence, tool outputs are not.
  Evidence: repeated commands establish workflow shape; outputs are high-volume, more sensitive, and unnecessary for frequency detection.
  Consequence: the provider receives bounded sanitized action summaries while persistent state retains only their digests.
- Decision: use a portable skill as CA's command-like artifact until CA has a native custom-command contract.
  Evidence: current CA discovers portable skills and custom agents but no custom command files.
  Consequence: LRN-170 does not silently add a second command runtime.

## Surprises and Discoveries

- The completed LRN-167 design explicitly removed tool-call assistant steps, which also removed the strongest repeated-workflow evidence needed by Distill.
- The real MiMoCode schema represents main-agent rows with an empty `agent_id` in the supplied evidence, while CA's importer test and query use only `main`.
- Dirty source already reduced reviewed generation from 43 calls to two and added automatic publication; the user's complaint therefore requires evidence and decision-quality repair, not merely another progress UI change.

## Recovery and Rollback

Remove only LRN-170 evidence projection/schema/generator/result-render changes, focused tests/snapshots, and matching documentation hunks. Do not reset, clean, stash, or replace the surrounding dirty integration. Existing state readers must either retain compatibility or fail with actionable refresh guidance.

## Completion Record

Changed files: `src/codex_learning.rs`, `src/learning.rs`, `src/main.rs`, and the LRN-170 harness/documentation entries. Oracle and reference trees remained read-only.

Commands and results: focused archive learning (38), learning (56 matching), TUI learning-review (3), and reviewed pager snapshot (1) tests pass. The default workspace matrix passes 2,536 tests with three documented ignores; the all-target/all-feature matrix passes 2,684 tests with four documented ignores. Formatting, strict workspace/all-target/all-feature Clippy, the all-feature release build, release help/version smokes, diff hygiene, snapshot hygiene, and protected-tree status pass.

Remaining risks: provider output quality was not tested against a live paid route, and the user's real learning profile was intentionally not rerun or mutated. The structured request, fake-provider response, publication, resume, and output contracts are deterministic and complete.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `PARITY.md`, `TESTING.md`, and `docs/.INDEX.md` record the final boundary and evidence.
