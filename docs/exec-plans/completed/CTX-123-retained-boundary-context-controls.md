# CTX-123 — Retained-Boundary Context Controls

Status: COMPLETE
Owner/session: Codex / current reliability continuation
Last updated: 2026-08-16

## Goal

Add an explicit completed-exchange retention boundary to manual compaction and
a deterministic 5×20 category map to `/context`, while preserving every
existing bare/focused/automatic compaction and exact textual-accounting
behavior.

## Non-Goals

- No automatic retained boundary, checkpoint schema change, session-format
  change, provider-history rewrite beyond the selected manual checkpoint, or
  provider/model/config change.
- No oracle/reference access or Cursor parity claim.

## Scope

Allowed: `/compact here [N]` parsing and validation, completed-exchange
boundary derivation without splitting tool interactions, prefix-only
checkpoint construction, byte-faithful retained canonical suffixes, stable
largest-remainder grid allocation, deterministic colors/glyphs, tests,
snapshots, disposable manual validation, native FreeBSD gates, and harness
documentation.

Forbidden: branches, worktrees, stashes, intermediate commits, installed
binaries, user runtime mutation, oracle/reference access, or changes to
automatic and focused compaction semantics.

## Design Invariants

- `here` defaults to two completed exchanges, accepts only 1–32, and refuses
  malformed or unavailable boundaries deterministically.
- An exchange begins with one canonical user turn and ends after its complete
  assistant/tool-continuation response. No tool call/result can be split.
- Only the prefix before the retained suffix is summarized. The retained
  canonical messages remain byte-for-byte unchanged and the existing
  checkpoint message boundary continues to encode persistence.
- `/context` retains exact totals, percentages, limit, and checkpoint coverage;
  its 100-cell category map uses deterministic largest-remainder allocation.

## Milestones

1. Add failing parser, boundary, preservation, grid-allocation, and render tests.
2. Implement typed retained-boundary requests and prefix-only checkpointing.
3. Add the deterministic context grid and reviewed wide/narrow snapshots.
4. Run focused, local, disposable-manual, and physical-FreeBSD gates.
5. Reconcile docs, archive this plan, and activate CFG-124.

## Validation

Focused: the initial `cargo test compact_here` characterization run failed with
10 expected missing-symbol/API errors before implementation. After the typed
request, boundary, orchestration, allocation, and rendering changes,
`cargo test compact_here --no-fail-fast`, `cargo test retained_checkpoint
--no-fail-fast`, `cargo test context_dashboard --no-fail-fast`, `cargo test
context_grid --no-fail-fast`, `cargo test automatic_checkpoint --no-fail-fast`,
and `cargo test compact_focus --no-fail-fast` all pass.

Completion: PASS. Local default passes 1,427 main plus 37 CLI tests; local
all-feature passes 1,456 main, 118 experiment, and 37 CLI tests. The documented
manual-Chromium and external-fixture cases remain ignored. Formatting, strict
all-feature Clippy, all-feature release, help/version, diff check, and snapshot
hygiene pass.

Manual/visual: PASS. A disposable 100x30 no-session TUI used a credentialless
loopback provider for three completed exchanges and one retained checkpoint.
The fourth request contained exactly the first exchange, excluded both
retained exchanges, and `/context` displayed the 5x20 map plus checkpoint
coverage of 2 of 6 canonical messages. No session was created; the fixture was
moved to Trash.

Physical FreeBSD: PASS on 15.1-RELEASE-p2 amd64 as unprivileged `aji`.
Focused tests pass; default passes 1,414 main plus 37 CLI tests; all features
pass 1,443 main, 116 experiment, and 37 CLI tests, with the two documented
ignores. Native formatting, strict Clippy, release, help/version, snapshot
hygiene, x86-64 ELF identity, and linkage pass. The exact source tree was
removed; the shared build cache remains for later tasks.

## Progress

- [x] Plan activated after USG-122 completion.
- [x] Failing characterization tests added.
- [x] Retained-boundary checkpointing and context grid implemented.
- [x] Focused/local/manual/FreeBSD gates complete.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: derive retained boundaries from complete canonical user exchanges
  and unresolved tool-call identity rather than message count alone.
  Evidence: tool continuations can span assistant/tool messages, and the
  checkpoint boundary must never split one interaction.
  Consequence: unavailable or malformed boundaries fail before provider I/O.
- Decision: allocate the 100 context-map cells from existing seven category
  totals by largest remainder with stable category-order ties.
  Evidence: this preserves exact textual accounting while making the map
  deterministic across platforms.
  Consequence: the map is display-only and changes no request or persistence
  bytes.

## Surprises and Discoveries

- The PTY walkthrough path resolves `/tmp` to `/private/tmp` on macOS; the
  disposable trusted-workspace fixture therefore used the canonical path.
- Ratatui emits context-map words in cursor-positioned spans, so visual
  inspection reconstructed the final screen coordinates instead of treating
  the raw terminal byte stream as linear prose.

## Completion Record

Changed files: `src/context.rs`, `src/main.rs`, `src/tui/commands.rs`,
`src/tui/render.rs`, `src/tui/render_tests.rs`, two context-dashboard
snapshots, and reconciled harness documents.

Commands and results: focused compaction/grid/orchestration tests, default and
all-feature suites, formatting, strict Clippy, release, help/version,
diff/snapshot hygiene, disposable loopback TUI, and exact-source physical
FreeBSD gates all pass as recorded above.

Remaining risks: context estimates remain estimates and the manual retained
boundary deliberately refuses a suffix that would leave no completed prefix.

Harness updates: `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`,
`ARCHITECTURE.md`, and `docs/.INDEX.md` reconciled. `PARITY.md` is unchanged
because CTX-123 is not a Cursor parity claim. CFG-124 is next.

## Recovery

CTX-123 changes no persistence schema. Source rollback removes only its typed
manual boundary handling, context grid presentation, tests/snapshots, and docs
while retaining OBS-117 through USG-122.
