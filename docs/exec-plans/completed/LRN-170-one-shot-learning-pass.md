# LRN-170 — One-Shot MiMo-Style Learning Pass

Status: SUPERSEDED by LRN-171
Owner/session: current ca implementation session
Last updated: 2026-08-24

## Goal

Turn the current many-call, two-approval archive-learning workflow into one
resumable user-approved pass: inventory existing assets, discover repeated
workflows from a compact session index, inspect only the strongest evidence,
publish validated missing skills, and show one final Created/Skipped/Needs more
evidence receipt. An already approved interrupted revision must continue after
upgrade or restart without another command or approval.

## Non-Goals

- Do not execute transcript instructions or grant the learning model tools.
- Do not write command files, source, binaries, provider settings, permissions,
  plugins, dependencies, or external application data.
- Do not weaken exact evidence/route/price binding, owner-only persistence,
  skill validation, atomic publication, undo, or strict schema parsing.
- Do not contact a live provider or mutate real profile data during source
  validation.

## Evidence

- The supplied ca screenshot shows `3/43` isolated Dream/Distill calls and says
  nothing is applied, so one approved operation remains slow and incomplete.
- The supplied MiMo screenshots show one Distill job: inventory, shortlist,
  high-confidence creation, skipped/needs-evidence explanation, and one final
  receipt. Its attempted command publication was blocked by external-directory
  authority; ca already owns a safer validated auto-skill publication boundary.
- LRN-169 checkpoints per-corpus calls and survives pager close, but app restart
  still needs another command and successful generation still requires a second
  apply decision.

## Design and Invariants

- Replace per-corpus Dream+Distill generation for reviewed archives with one
  deterministic compact evidence corpus and at most two calls: one conservative
  memory pass and one reusable-workflow packaging pass. The packaging contract
  accepts only high-confidence skill candidates supported by at least three
  distinct sessions.
- Inventory existing portable skills before discovery; packaging creates only
  missing skills or exact auto-distilled replacements. Empty output is valid.
- `/dream` remains available separately; archive `/learn` performs the complete
  memory-and-skill pass authorized by the reviewed archive evidence.
- One evidence approval authorizes the exact priced two-call plan and automatic
  publication through the existing validated, journaled auto-distill store.
- Progress is stage-oriented (`Inventory`, `Discover`, `Package`, `Publish`) and
  still reports exact completed/total provider calls.
- A v1 `progress.json` with the same evidence/route/model/price is upgraded
  without another approval. Completed legacy results may be supplied as bounded
  candidate hints; no old call is repeated.
- Persisted exact approval automatically resumes when the TUI starts. Drift
  stops and requires fresh review; ordinary interruption does not.
- Final output mirrors the useful MiMo shape: Created, Skipped, Needs more
  evidence, route/duration/call count, and any publication conflicts.

## Acceptance

- The screenshot-sized 43-call revision becomes at most two new provider calls.
- Closing the pager, quitting, upgrading, and reopening ca do not require
  another `/learn approve` for the same exact revision.
- Successful completion publishes validated skills and clears review,
  checkpoint, and approval atomically enough to recover through existing
  learning journals.
- Existing skill names are not duplicated; low/medium confidence remains
  skipped with a visible reason.
- Focused interruption, migration, stale-generation, restart-resume,
  auto-publication, conflict, empty-result, privacy, and snapshot tests pass,
  followed by repository completion gates.

## Progress

- [x] Characterized the current per-corpus call plan and two-decision TUI.
- [ ] Add strict compact-corpus packaging and existing-asset inventory.
- [ ] Add v1 checkpoint migration and two-call resumable execution.
- [ ] Auto-publish after exact approval and render a final learning receipt.
- [ ] Auto-resume an approved interrupted revision on TUI startup.
- [ ] Run focused/full validation and reconcile the harness.

## Recovery

Remove only LRN-170 schema, orchestration, migration, receipt, tests, and docs
hunks. Do not reset, clean, or replace the surrounding dirty integration.

## Completion Record

Changed files:
Commands and results:
Remaining risks:
Harness updates:
