# LRN-169 — Zero-Jitter TUI, Copyable Sessions, and Streamlined Learning

Status: COMPLETE
Owner/session: current ca implementation session
Last updated: 2026-08-24

## Goal

Eliminate streaming-layout jitter, make the complete privacy-safe visible session copyable, preserve mouse selections across scrolling, make `/learn approve` observable and interruption-safe, and stop the low-yield guard from treating distinct successful edits as failure to progress.

## Non-Goals

- Do not weaken exact evidence or proposal digest validation.
- Do not apply generated memory or skills automatically.
- Do not activate or broaden the autonomous learning service.
- Do not permit autonomous source, binary, provider, permission, routing, plugin, or dependency mutation.
- Do not contact a live provider, mutate the real learning profile, install binaries, or alter protected oracle/reference trees during validation.

## Evidence and Current Behavior

- `src/codex_learning.rs::parse_tui_request` accepts only status, sync, and review; `/learn approve` is rejected.
- `src/main.rs` already runs typed learning work on a dedicated Tokio task and keeps the TUI responsive, but receives only one final `LearningReviewReady` event.
- `ArchiveLearningStore::generate_proposal` performs sequential Dream/Distill calls and exposes no progress callback; `process_review` prints only after every call finishes.
- The live approved review planned 42 bounded calls and held an established provider connection while appearing idle.
- `ArchiveLearningStatus::render` says processing has not started whenever the exact digest is reserved, even though that marker is written immediately before provider generation.
- The autonomous status schema error is binary skew: current `codex-team` source includes `evidence_roots`, while the installed supervisor parser rejected it. Source compatibility must remain strict; the TUI/manual review path must not depend on that supervisor status.
- `src/tui/render.rs` appended a visible cursor or space to every rendered streaming Markdown line. Blink phases therefore changed wrapping and the transcript maximum, producing the exact up/down movement in `1.png` and `2.png`.
- `/copy` selected the latest assistant message even when it was an empty tool-only tail and invoked macOS `pbcopy`; there was no whole-session command or shortcut.
- Transcript and pager selections stored terminal coordinates for only the current rendered frame, and every wheel event cleared them.
- Successful `edit_file`/`apply_patch` observations always incremented the generic no-progress streak even when their mutation receipt proved bytes changed.

## Scope

Allowed files: `src/codex_learning.rs`, `src/loop_guard.rs`, `src/main.rs`, `src/app.rs`, `src/keymap.rs`, `src/tui/{commands,event,input,render,transcript}.rs`, focused tests/snapshots, this plan, and current architecture/continuity/testing/queue/index documents.

Forbidden files/areas: runtime profile data, credentials, installed binaries/services, unrelated dirty work, `oracles/`, `reference/`, Git history, and autonomous publication authority.

Runtime state affected: none during implementation. A later user-run `/learn approve` may contact the already selected provider and create an inert proposal under the existing exact-digest contract.

## Design and Invariants

- Add a typed `ApproveCurrentEvidence` TUI request. It reads the current pending priced review digest from the store and passes that exact digest through existing `process_review` validation. Unpriced or absent evidence fails with visible guidance.
- Preserve `ApproveEvidence { digest }` for the existing Enter-confirmation flow and stale-result tests.
- Add a bounded typed progress event containing phase, completed calls, total planned calls, and a human-readable current operation. No transcript or provider response text enters progress state.
- Emit progress before the first call and after each Dream/Distill call. The final count must equal the exact materialized call count used for pricing; mismatches fail closed in tests.
- Route progress through the existing generation-tagged learning task and `StreamMsg` channel. Ignore stale generations and update only the learning pager/status while it remains associated with the task.
- Keep the pager closable without cancelling work. Reopening `/learn` while busy shows the latest progress rather than restarting or duplicating provider calls.
- Replace the false persisted status sentence with wording that the revision is reserved and may be processing in another live invocation; persisted state alone cannot prove process liveness.
- Keep strict supervisor schema parsing. Report binary/schema skew clearly rather than silently dropping enrollment fields.
- Render exactly one constant-width streaming cursor at the end of the final non-empty response line. Hidden phases retain the same symbol and use only terminal style.
- Preserve bottom-follow on new deltas and bottom-relative anchoring while scrolled upward; blink-only frames cannot alter any transcript coordinate or scroll measurement.
- Store validated Dream/Distill results in a separate strict owner-only `progress.json`, bound to evidence digest, provider, model, price, and an exact materialized-plan digest. Checkpoint after every successful call and execute only missing work on retry.
- `/copy` and `/copy last` choose the latest non-empty response, including an active non-empty stream. `/copy all` uses the TUI's privacy-safe semantic projection and hydrates complete paged history first.
- `copy_transcript` defaults to `Ctrl+X C`, stays available during active turns, and uses the existing in-process clipboard boundary.
- Selection points use document rows. While dragging, each visible frame is cached, wheel and edge scrolling advance one row, and release copies the traversed range in either direction. Resize/incompatible geometry cancels; 200,000 cached cells is a hard bound.
- A successful mutation receipt with different before/after bytes resets the generic no-progress streak after repeated-edit detection. Similar edits and repeated failures retain their existing stricter incidents.

## Milestones

### M1 — Streaming and loop-guard regressions

- Reproduce the screenshots' long response at wide, narrow, Markdown, resize/reflow, bottom-follow, and scrolled-up positions.
- Assert alternating cursor styles leave every symbol coordinate and bottom-relative offset unchanged.
- Characterize four distinct byte-changing edits versus repeated similar edits and failures.

### M2 — Durable in-TUI learning

- Add parser coverage for `/learn approve`.
- Add fake-backend progress sequence/count coverage.
- Add orchestration coverage for generation-tagged progress and pager updates.

- Add current-review approval resolution and progress callback plumbing.
- Add a progress stream event and state application in the TUI event loop.
- Update command discovery and busy/reopen presentation.
- Add strict atomic checkpoint load/write/resume/cleanup and binding drift refusal.

### M3 — Copy and cross-scroll selection

- Share the privacy-safe visible transcript projection between rendering and copy-all.
- Add last/all command parsing, active-turn shortcut, paged-history hydration, and clipboard failure coverage.
- Replace frame-local selection with bounded document-row caching for transcript and pager scrolling.

### M4 — Status/schema diagnostics

- Correct the reserved-review status wording.
- Turn strict supervisor schema mismatch into actionable installed-component skew guidance without weakening parsing.

### M5 — Validation and reconciliation

- Run focused learning/parser/orchestration/render tests, then required Rust completion gates.
- Reconcile durable docs and move this plan to completed when all applicable gates pass.

## Acceptance Criteria

- `/learn approve` starts exactly the currently priced evidence revision from inside the TUI; no digest or second terminal is required.
- The pager visibly advances through `0/N` to `N/N` bounded calls and identifies Dream versus Distill without exposing evidence text.
- The TUI remains responsive; closing/reopening the pager does not cancel or duplicate processing.
- Source or route drift clears approval and returns a fresh review requiring another `/learn approve`.
- Completion opens the existing inert proposal review; publication still requires a separate explicit proposal action.
- Fake-provider tests prove exact progress order/count and no live provider or runtime profile is touched.
- Supervisor schema skew is actionable and does not block manual `/learn review` or `/learn approve`.
- Cursor visible/hidden phases are geometry-identical; new deltas follow the bottom or leave the scrolled visible top anchored.
- Four distinct successful byte-changing edits do not open the low-yield dialog; existing repeated-edit/failure incidents still do.
- `/copy all` includes complete saved history, visible tool/diff content, active streaming, Unicode, Markdown, and enabled reasoning, while excluding system prompts, hidden reasoning, provider-only state, and transient UI.
- Forward and reverse mouse selections traverse transcript and pager scroll frames without losing Unicode cells; the bound and cancellation paths are explicit.

## Validation

Focused:
- `cargo test --bin ca codex_learning::tests --no-fail-fast`
- `cargo test --bin ca learning_review_ --no-fail-fast`
- relevant command/render tests if snapshots change

Completion:
- `cargo test --workspace --no-fail-fast`
- `cargo test --workspace --all-targets --all-features --no-fail-fast`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo build --workspace --release --all-features`
- `git diff --check`, snapshot hygiene, protected-tree status

Manual/visual: deterministic fake backend only. No live review run or installation during implementation.

Native visual: PASS with the rebuilt, uninstalled binary in a disposable 100x30
PTY and credentialless delayed loopback provider. The four wrapped screenshot
response rows received no terminal writes during repeated Thinking ticks; the
streaming cursor updated only one final-line cell.

Installed-binary and live-provider verification were intentionally not run;
both were outside the user's authorization and are unnecessary for source
completion.

## Progress

- [x] Current command, async task, exact-digest, provider loop, status, and schema-skew paths traced.
- [x] Streaming cursor and low-yield regressions added and fixed.
- [x] `/learn approve`, generation-tagged progress, and exact checkpoint resume implemented.
- [x] Status/schema diagnostics corrected.
- [x] Copy-last/all, active shortcut, privacy projection, and paged-history continuation implemented.
- [x] Cross-scroll transcript/pager selection, Unicode handling, cancellation, and bounds implemented.
- [x] Focused and completion validation recorded.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: `/learn approve` is itself the explicit authorization and resolves only the store's currently priced revision.
  Evidence: the user requested a one-command in-TUI approval without digest retyping.
  Consequence: the existing Enter confirmation remains available but is not required.
- Decision: progress reports operation boundaries, not provider token events or evidence text.
  Evidence: the work loop has a stable exact call plan and provider streams may vary by backend.
  Consequence: progress is deterministic, bounded, and privacy-preserving.
- Decision: retain strict supervisor state parsing.
  Evidence: `evidence_roots` is authority-bearing enrollment state.
  Consequence: binary skew is diagnosed; fields are never silently ignored.

## Surprises and Discoveries

- Persisted `approved_review_digest` is both a reservation marker and an in-flight checkpoint; it cannot distinguish a live process from an interrupted one.
- Current source already contains the `evidence_roots` supervisor schema, proving the observed parse error comes from mismatched installed components rather than absent source support.

## Recovery and Rollback

Remove only LRN-169 request/progress types, event routing, focused tests, command metadata, and documentation hunks. Do not reset, clean, stash, or replace the surrounding dirty integration.

## Completion Record

Changed files: `src/{app,codex_learning,keymap,loop_guard,main}.rs`,
`src/tui/{commands,event,input,render,render_tests,transcript}.rs`, four focused
streaming snapshots, and the current architecture/continuity/testing/queue/index
documents.

Commands and results: focused loop-guard (16), streaming (6), learning (34),
copy (8), transcript (7), event/keymap/command/progress/snapshot regressions all
pass. The default workspace matrix passes 2,532 tests with three ignores; the
all-target/all-feature matrix passes 2,679 with four ignores. Formatting,
strict all-target/all-feature Clippy, the all-feature release build, release
help/version smokes, `git diff --check`, snapshot hygiene, and protected-tree
status pass. The disposable native PTY walkthrough passes.

Remaining risks: installed binaries still contain the prior behavior until the
user separately authorizes installation. Live provider latency and clipboard
integration remain environment-dependent, but their boundaries are covered by
fake/injected tests and failures remain visible and non-destructive.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`,
`TESTING.md`, and `docs/.INDEX.md` now record the invariants, completion, and
actual validation evidence.
