# LRN-134 — Autonomous Dream and Distill

Status: COMPLETE
Owner/session: Codex / current user-requested implementation
Last updated: 2026-08-17

## Goal

Add bounded idle-time Dream memory maintenance and Distill skill generation
for interactive persistent profiles, with strict evidence filtering, route and
authority limits, crash-safe journals, exact undo, and explicit status/control
commands.

## Non-Goals

- Do not grant generated content tools, permissions, network, sandbox,
  executable metadata, plugins, subagents, commands, scripts, assets, or
  dependencies.
- Do not run in bare, no-session, headless, service, scheduled, gateway,
  review, checkpoint, or otherwise non-interactive runners.
- Do not add a drawer, MCP sampling, foreign subprocess handoff, home-directory
  scanning, or rewrite manual/imported/project/plugin/bundled skills.

## Evidence and Current Behavior

- `src/memory.rs` owns profile/workspace stores, validators, digest-bound
  records, recall, and explicit proposal persistence. Current missing switches
  default off and must change compatibly so explicit `false` remains decisive.
- `src/skills.rs` and `src/skill_cli.rs` own discovery, validation, enabled
  state, and safe one-file skill publication boundaries.
- `src/pricing.rs` owns automatic cheapest-eligible route selection and rejects
  unpriced or stale routes; maintenance must reuse that closed authority.
- `src/session.rs` owns bounded profile session reads and now labels
  learning-ineligible foreign imports.
- `src/usage.rs` owns typed request kinds and process-local accounting.
- The interactive TUI event loop in `src/main.rs` is the only surface with both
  an idle signal and immediate user-input priority.

## Scope

Allowed files: typed maintenance/state modules; compatible config/runtime,
memory, skill, session, request/usage, TUI command/input/render, and event-loop
integration; profile stores; deterministic tests/snapshots; harness documents.

Forbidden files/areas: `oracles/`, `reference/`, credentials, installed binary,
real user runtime state, unrelated project/user/plugin/bundled skill sources,
and any runner excluded above.

Runtime state affected: after due or explicit maintenance in an interactive
persistent profile only, Dream-owned memory records and profile-owned
auto-distilled one-file skill generations may be atomically created, replaced,
disabled, or exactly undone. Routine status remains body-free.

## Design and Invariants

- Missing Dream/Distill settings mean enabled at seven/thirty-day intervals;
  explicit `false` remains off. Missing corresponding memory generation/recall
  settings mean enabled while explicit `false` remains authoritative.
- At most one maintenance job runs, one provider request is issued per due
  Dream or Distill operation, and any user input cancels it before ordinary
  input processing continues.
- Automatic route selection uses the existing cheapest priced eligible route
  at `standard`; absence is a typed skip, never an unpriced fallback.
- Corpus selection is deterministic and capped at 32 eligible sessions, 64
  latest messages each, and 256 KiB overall. Dream uses seven days, Distill
  thirty. Hidden/provider/tool/media/private/credential content and
  learning-ineligible imports are excluded.
- Dream accepts a closed maximum-32 operation batch with source session IDs and
  expected digests. Automatic updates/deletes can touch only prior Dream-owned
  records. Manual, approved-proposal, and imported records are immutable to
  maintenance. User-wide facts require a global user statement or two distinct
  workspaces. A crash-recoverable journal and CAS revalidation wrap application.
- Distill accepts at most two closed packages. Each contains only one validated
  `SKILL.md`, is marked `auto_distill`, and is enabled profile-globally. It may
  replace/disable only a prior auto-distilled generation; the prior generation
  remains disabled for exact rollback.
- `/dream` and `/distill` expose `run`, `status`, `on`, `off`, and
  `undo <run-id>`. Manual runs use identical routing, corpus, validation,
  authority, persistence, and accounting.

## Milestones

### M1 — Characterize storage, routing, and scheduling seams

- Trace memory/skill ownership, defaults, session corpus access, usage kinds,
  priced route selection, idle input priority, and current durable patterns.
- Add failing compatibility/default, corpus, schema, and eligibility tests.

### M2 — Typed maintenance core and persistence

- Add settings, due-state/run journal, sanitized corpus construction, strict
  Dream/Distill schemas, CAS application, skill-generation stores, and undo.
- Prove bounds, provenance protection, crash recovery, conflicts, and forbidden
  generated content with hermetic tests.

### M3 — Interactive orchestration and presentation

- Add commands/status views, async idle scheduling, input-first cancellation,
  priced standard route dispatch, and distinct usage kinds.
- Add wide/narrow snapshots and disposable credentialless loopback PTY checks.

### M4 — Completion

- Run focused/default/all-feature, formatting, strict Clippy, release, diff,
  and snapshot gates; reconcile the harness and archive this final roadmap plan.

## Acceptance Criteria

- Existing explicit `false` settings remain off; absent settings adopt the new
  enabled defaults without rewriting configuration on load.
- Excluded runners and ineligible imports cannot enter maintenance scheduling
  or corpus construction.
- Every run is one-request, bounded, priced-route-only, typed, journaled, and
  cancelable by user input; status/diagnostics expose no corpus text.
- Dream cannot alter non-Dream records or cross workspace evidence boundaries;
  exact CAS conflicts fail without overwrite and undo refuses drift.
- Distill publishes only validated one-file auto-owned skills, retains prior
  disabled generations, and cannot edit any other skill origin.
- Old configs/sessions/skill stores remain readable without automatic rewrite.

## Validation

Focused:

- PASS: 10 `learning::` tests cover defaults, scheduling, private typed
  requests, distinct usage, corpus filtering/bounds, Dream ownership,
  cross-workspace evidence, CAS drift, crash recovery, Distill authority,
  replacement, rollback, and redacted diagnostics.
- PASS: three `maintenance_` orchestration/request/command tests plus the
  configuration, public-contract, request-tool-count, usage-snapshot, and
  process-cancellation regressions that surfaced during the first full run.
- PASS: reviewed wide/narrow Dream and Distill status snapshots and updated
  `/usage` snapshots; no `.snap.new` or `.snap.pending` remains.

Completion:

- PASS: `cargo test --no-fail-fast` — 1,548 main and 37 CLI tests; one ignored.
- PASS: `cargo test --all-targets --all-features --no-fail-fast` — 1,577 main,
  118 experiment, and 37 CLI tests; two ignored.
- PASS: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D
  warnings`, `cargo build --release --all-features`, release help/version,
  `git diff --check`, snapshot hygiene, and oracle/reference diff check.
- PASS on physical FreeBSD 15.1-RELEASE-p2 amd64: 1,535 default main plus 37
  CLI tests; 1,564 all-feature main plus 116 experiment plus 37 CLI tests;
  packaged formatting, strict Clippy, all-feature release, help/version,
  snapshot hygiene, x86-64 ELF, linkage, and SHA-256 inspection. Only the
  documented manual-browser and external-fixture tests were ignored.

Manual/visual:

- PASS: a released native TUI under a disposable home/workspace rendered due
  automatic no-evidence skips, full `/dream status`, and an explicit `/dream
  off` mutation. The owner-only stores retained two distinct bounded run
  records and the intended explicit/absent defaults. The fixture moved
  recoverably to `/Users/aji/.Trash/ca-lrn134-pty-20260817`.

Blocked checks: none. A credentialless custom PTY provider is intentionally
ineligible for automatic maintenance because the shared router accepts only
officially priced routes; the injected provider test covers the exact
one-request structured path without weakening that boundary.

## Progress

- [x] Memory, skill, routing, session, usage, and idle seams characterized.
- [x] Typed maintenance settings, corpus, schemas, stores, journal, and undo complete.
- [x] Interactive scheduling, commands, cancellation, usage, and snapshots complete.
- [x] Native walkthrough, full gates, and harness complete.

## Decisions

- Decision: generated material has content authority only within dedicated
  Dream/auto-distill stores.
  Evidence: existing memory and skill origins carry different ownership and
  execution implications.
  Consequence: maintenance uses new provenance-bearing records and never
  reclassifies manual material.

## Surprises and Discoveries

- The existing pricing router correctly rejects a custom credentialless
  loopback route, so a native provider POST cannot be demonstrated without
  weakening the task's priced-route-only acceptance criterion. The provider
  seam remains covered with an injected one-attempt backend.
- A first complete test run exposed two stale public expectations (memory
  defaults and the added proposal tool), the two new usage categories in
  snapshots, and one unrelated flaky process-cancellation test. The
  expectations/snapshots were updated and all four tests passed on focused
  rerun before both complete suites passed.

## Completion Record

`LRN-134` is complete. Missing learning and corresponding memory switches now
adopt the enabled defaults while explicit `false` stays authoritative. Idle
interactive persistent profiles alone can issue one priced standard request
per serialized Dream/Distill run; corpus, ownership, persistence, generated
skill authority, cancellation, status, accounting, recovery, and undo are all
bounded and tested. No oracle/reference file, installed binary, real provider,
or real user runtime state changed. The exact combined roadmap tree also passes
the native FreeBSD completion matrix; its disposable release SHA-256 is
`e5ed2c281efac7391d5dc4db793576871978996b3f517b22a307abe541c34c8c`.

## Recovery and Rollback

Every published run records bounded before-images and after-digests. Startup
recovery can finish or roll back an exact journal phase without guessing.
`undo` applies only when current after-digests still match; otherwise it makes
no change. No destructive Git command or broad filesystem deletion is needed.
