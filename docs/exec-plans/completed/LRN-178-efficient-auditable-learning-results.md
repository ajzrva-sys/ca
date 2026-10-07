# LRN-178 — Efficient, Auditable Learning Results

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-25

## Goal

Make future direct `/learn` and `/distill` runs complete clear zero-change
archives in about two provider turns, require signal-directed deep inspection,
and render a compact receipt backed by an exact run-specific audit.

## Non-Goals

- Do not reopen or rerun the completed 1,596-message learning revision.
- Do not weaken privacy, retirement, publication, rollback, or undo rules.
- Do not modify oracle/reference trees, runtime profile state, or the installed
  binary without separate explicit authorization.

## Evidence and Current Behavior

- `LearningCorpus::adaptive_overview` always embeds only cluster page zero, so
  the observed 119-cluster archive requires a redundant second-page provider
  round even though the complete catalog fits the overview byte bound.
- `generate_adaptive_learning_pass` eagerly sends up to 32 KiB of reusable
  assets and 24 KiB of memory, retains the immediately preceding retrieval
  bodies, and offers separate search/read/inventory tools. The real zero-change
  run used five turns, 18 retrievals, and 147,966 input tokens.
- `publish_generated_runs` renders every model decision and every cluster ID in
  one Markdown table. A grouped singleton decision therefore becomes an
  unreadable wall.
- Receipt candidate counts are proposal counts, but the current sentence
  follows the existing inventory count and reads as if no assets exist.
- Direct journal version 4 persists coverage and undo data, not the complete
  decision/asset/memory audit needed by `/learn status <run-id>`.
- The user selected signal-directed inspection and compact transcript plus
  exact run drilldown.

## Scope

Allowed files: `src/{learning,codex_learning,main}.rs`, focused TUI
command/render tests and learning snapshots, this plan, and current learning
harness documents.

Forbidden: `oracles/`, `reference/`, credentials, runtime learning/profile
stores, installed binaries, services, Git history, and unrelated dirty changes.

Runtime state affected: none during implementation and automated validation.

## Design and Invariants

- High signal means at least three distinct sessions or an explicit recurring
  instruction; medium means two sessions; low means an ordinary singleton.
- Every high/medium cluster must have two inspected representative sessions,
  or every available session when fewer exist, before final validation.
- Existing assets are typed, digest-bound catalog entries. Duplicate and
  extend-existing decisions cite exact catalog IDs; receipts resolve paths from
  the host catalog, never model text.
- Every high/medium cluster gets a typed memory disposition. Already-covered
  decisions cite exact current memory IDs.
- Initial, individual retrieval, aggregate retrieval, and retained-context
  bounds remain independent. Raw outputs and excluded evidence never enter the
  index or audit.
- Journal v5 adds audit data while versions 1–4 remain readable. Legacy run
  drilldown is coverage-only and never mutates or migrates the old journal.
- The compact transcript shows at most twelve prioritized rows and collapses
  singleton detail. Exact decisions remain available with
  `/learn status <run-id>` and `ca learn status [run-id]`.

## Milestones

### M1 — Characterization and typed indexes

- Add failing tests for a fitting 119-cluster overview, typed asset counts and
  references, memory dispositions, and signal-directed inspection.
- Implement typed asset/memory indexes and bounded relevant lookup.

### M2 — Adaptive protocol efficiency

- Add batched cluster inspection with evidence, asset, and memory matches.
- Enforce inspection coverage, aggregate result bounds, immediate consumed-turn
  compaction, and exact in-run correction.
- Prove the real-shaped clear zero-change fixture finishes in two scripted
  turns without catalog pagination.

### M3 — Audit journal and compact receipt

- Add journal v5 decision, resolved-asset, memory, route, usage, and turn audit.
- Add exact run status parsing/rendering with legacy degradation.
- Replace the unbounded table with prioritized compact rows, collapsed
  singleton summaries, explicit existing/proposed counts, asset paths, memory
  summary, and drilldown command.

### M4 — Product and completion validation

- Add wide/narrow snapshots and focused failure/recovery/privacy coverage.
- Run focused learning/TUI tests and all repository completion gates.
- Reconcile harness documents and move this plan to completed only after actual
  results are recorded.

## Acceptance Criteria

- A 190-session/1,596-message/119-cluster fixture makes everything searchable,
  deeply inspects every high/medium cluster, decides every cluster, cites the
  three existing BSD command paths, retires truthfully, and finishes in two
  scripted turns with zero duplicate publication.
- Fitting catalogs are complete in the initial request; overflow remains
  paginated and cannot retire until fully delivered and decided.
- Missing inspection, memory disposition, asset identity, or current digest is
  repaired in-run or fails without retirement/publication.
- The normal receipt contains no raw all-cluster list, no misleading candidate
  wording, and no more than twelve shortlist rows.
- Exact v5 drilldown reports every decision and reference; legacy v1–v4
  drilldown is safe and explicit about unavailable detail.
- Cancellation, interruption/resume, rollback, eight-turn exhaustion, privacy,
  and undo behavior remain passing.

## Validation

Focused:

- `cargo test -p ca learning::tests -- --test-threads=1`
- `cargo test -p ca codex_learning::tests -- --test-threads=1`
- focused command parsing and TUI render/snapshot tests

Completion:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo build --workspace --release --all-features`
- release help/version/doctor, `git diff --check`, and snapshot hygiene

Manual/visual: review the new wide/narrow compact receipt and drilldown
snapshots. No live `/learn` run is authorized.

## Progress

- [x] Characterization covers the fitting 119-cluster protocol and receipt.
- [x] Typed catalogs, inspection policy, and bounded adaptive protocol pass.
- [x] Journal v5, status drilldown, and compact transcript pass.
- [x] Focused and complete validation pass and harness records are reconciled.

## Decisions

- Decision: use signal-directed inspection rather than an arbitrary session
  floor.
  Evidence: user selection after reviewing the six-session real receipt.
  Consequence: confidence scales with meaningful clusters, not archive size.
- Decision: retain exact audit behind `/learn status <run-id>`.
  Evidence: user selected compact transcript plus drilldown.
  Consequence: normal output stays readable without losing provenance.

## Surprises and Discoveries

- The complete encoded 119-cluster catalog fits the existing 32 KiB overview
  bound even though its conceptual pagination count is greater than one. The
  provider therefore needs no catalog-list tool round trip for the real-shaped
  case.
- Stable typed asset IDs include the current content digest. This makes a stale
  or drifted reference fail as an unknown ID without requiring a second mutable
  digest field in the model result.
- Strict Clippy found one derivable default and one redundant fixture branch;
  both were corrected before the final lint and release gates.

## Recovery and Rollback

Use targeted patches only. New journal fields are additive/defaulted; never
rewrite an existing journal. A failed publication retains the existing
prepared/applied rollback path and retires nothing.

## Completion Record

Changed files:

- `src/learning.rs`: typed asset and memory indexes, deterministic signal,
  fitting complete catalog delivery, batched cluster inspection, exact audit
  validation, independent retrieval budgets, continuation compaction, and the
  190-session/1,596-message acceptance fixture.
- `src/codex_learning.rs`: journal v5 audit persistence, run-specific status,
  compact prioritized receipts, resolved duplicate paths, memory review, and
  legacy drilldown behavior.
- `src/{main,skills}.rs` and `src/tui/{commands,render_tests}.rs`: status routing,
  discovered-skill paths, readable operator syntax, compact wide/narrow render
  coverage, and four direct-learning snapshots.
- `CONTINUITY.md`, `NEXT_STEPS.md`, `docs/.INDEX.md`, and this plan: durable
  completion and validation record.

Commands and results:

- Focused adaptive/archive learning suites passed, including the exact
  190-session/1,596-message/119-cluster/132-asset zero-change fixture in two
  scripted provider turns and the late-evidence 184-session fixture.
- `cargo test`: passed across the complete workspace, including 1,980 CA unit
  tests and all integration, snapshot, and embedded `codex-team` suites.
- `cargo fmt --check`: passed.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `cargo build --release`: passed.

Remaining risks:

- No live provider `/learn` was run by design. Provider latency and real usage
  remain operational observations for a separately authorized future run; the
  completed 1,596-message revision was neither reopened nor rerun.
- Journal versions 1–4 cannot expose decisions they never recorded; their
  run-specific status explicitly reports that limitation and remains read-only.

Harness updates:

- LRN-178 is marked complete in `NEXT_STEPS.md` and indexed under completed
  plans. `CONTINUITY.md` records protocol, UI, compatibility, validation, and
  runtime non-mutation facts.
