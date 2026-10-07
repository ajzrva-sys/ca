# TUI-212 — Copy, Mermaid, and Terminal Polish

Status: COMPLETED (2026-08-28)
Owner/session: CUR-204 goal session
Last updated: 2026-08-28

## Goal

Add message/part copy selection, bounded Mermaid rendering, terminal theme/title/
focus awareness, and safe clickable links without blocking the event loop.

## Non-Goals

No hosted PR discovery, unsafe OSC links, manual-theme override, event-loop
rendering work, or unsupported Mermaid behavior.

## Evidence and Current Behavior

`/copy` supports `last|all`; Markdown strips destinations; notifications lack
focus gating; CA has configured themes/status and terminal notifications but no
OSC 11 sync/title indicators. Mermaid is an ordinary code block.

## Scope

Allowed: app/markdown/TUI/input/event/render/terminal/settings code, mmdflux
dependency pin, focused PTY/tests/snapshots/docs.

## Design and Invariants

No-arg `/copy` searches all stored roles and drills into sanitized content,
visible thought, tool calls/results, and provider parts. Pin `mmdflux=2.6.1`
with defaults off; render flowchart/class/state/sequence off-loop with 64 KiB
input, 1 MiB output, digest/width cache, and code fallback. OSC 11 detection is
async/rate-limited at startup/focus/manual sync and never overrides manual
theme. Titles clear on restore. Unknown focus means focused; suppress BEL/OSC 9
while focused. OSC 8 permits only http/https/mailto and contained file targets,
with sanitized bytes and control-sequence-free width.

## Milestones

### M1 — Copy picker and render model

Add picker/drilldown tests and preserve link targets in the terminal model.

### M2 — Mermaid

Add supported/invalid/unsupported/oversize/cache/failure tests and off-loop
bounded rendering with wide/narrow snapshots.

### M3 — Terminal awareness

Add OSC 11/focus/title/link parsers and PTY cases for focus notifications,
malformed escape, link width, and restoration cleanup.

## Acceptance Criteria

Existing `copy last|all` remain; invalid diagrams fall back; input remains
responsive; links cannot emit unsafe control sequences or escape workspace;
focus/theme/title behaviors cleanly restore.

## Validation

Focused markdown/copy/input/event/render/PTY tests, reviewed snapshots, and
child gates.

## Progress

- [x] Failure-first copy/link tests.
- [x] Copy picker and link model.
- [x] Mermaid renderer/cache.
- [x] Terminal awareness and child gates.

## Decisions

Plain styled text is always the compatibility fallback.

## Surprises and Discoveries

- Mermaid cache identity must include pane width because `mmdflux` layout is
  width-sensitive; the bounded worker therefore caches by source digest and
  width and falls back when a supported diagram still exceeds the pane.
- Markdown labels are not unique identifiers. When the same visible label maps
  to different destinations, OSC 8 rendering fails closed instead of attaching
  a possibly wrong destination.
- Automatic OSC 11 theme selection is authoritative only when `ui.theme` is
  absent. An explicit configured theme always wins, including after focus
  return and manual `/sync-theme` requests.
- Cursor behavior remains implementation evidence; no runtime parity claim was
  made.

## Recovery and Rollback

Clear only process-local caches/title state; reverse focused dependency/hunks.

## Completion Record

Completed 2026-08-28. `/copy` now opens a searchable message/part picker while
preserving `last` and `all`; only stored, already-sanitized visible parts are
copyable. Markdown retains destinations outside layout text, and OSC 8 output
allows only sanitized HTTP(S), mailto, or workspace-contained file targets.
`mmdflux=2.6.1` renders supported bounded Mermaid diagrams off the event loop
with digest/width caching and exact code-block fallback. Focus events gate
notifications; bounded asynchronous OSC 11 probes drive only automatic themes;
optional title indicators clear during terminal restoration. `/sync-theme` and
`/status-indicators` expose the controls.

Validation passed: focused copy/input/render/markdown/Mermaid/terminal tests;
reviewed wide/narrow copy and Mermaid snapshots; both serial workspace test
matrices; `cargo fmt --all -- --check`; strict all-target/all-feature workspace
Clippy; all-feature workspace release build; `git diff --check`; zero
`.snap.new`; exact protected-baseline comparison; and protected-tree verifier.
The first default matrix used a stale pre-fix test binary and identified three
deterministic expectations (relative file link, duplicate-label fixture, and
command-count snapshots); focused reruns and the complete corrected matrix
passed. No user runtime state, provider request, installed binary, oracle, or
reference file was changed.
