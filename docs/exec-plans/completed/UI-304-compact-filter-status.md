# UI-304 — compact filter status

Status: complete; installed ca 0.1.62 (2026-10-25).

## Acceptance

Normal entry point: ca terminal, top filter banner and provider preview. Replace two banner rows with one left-aligned row: off hidden; enabled/empty `Filter on · Nothing armed`; active `Filter active · {summary}`; stopped `Filter stopped · {local explanation}`. Use authoritative typed state, quiet normal styling, warning/error tokens, cell-aware ellipsis. Keep preview content, scrolling, filtering and commands unchanged. Full explanation remains in `/filter`.

## Work

- [x] Add focused regressions and typed presentation snapshot.
- [x] Integrate one-row rendering and preserve preview behavior.
- [x] Freeze source, obtain independent review, run focused tests/format/strict lint.
- [x] Bump once, build one release, install recoverably and verify installed artifact.
- [x] Prove installed pending second preview update with representative long history.

## Verification and boundaries

Use disposable source and application state. Cover off/empty/active/stopped at 24/60/80/100 columns, tiny heights, state transitions and Unicode clipping. Reuse existing preview tests and loopback PTY fixture with 192 historical messages and withheld responses. No production config/session mutation, oracle edits or backend/filter semantics changes. This checkout has no .git or .agent/PLANS.md; before-images and hashes are in scratch/UI-304. Existing source/installed version is 0.1.61. Focused workflow is sufficient for this presentation change; no complete workspace gate is claimed.

## Source validation

Failure-first old banner test failed as intended. First integrated run: 335 pass, 3 failed (two missing complete `/filter` trip-detail assertions and one normalized-version fixture mismatch). Independent review found the pager omission, unused wrappers and Off tiny-height preservation; repaired. Second run: 337 pass, one failed Unicode fixture assertion because text snapshots include wide-glyph continuation cells. Corrected the assertion to inspect actual terminal cells. Final run: 338 pass, 1 ignored; formatting and strict all-feature ca-bin Clippy pass. No filter/gate criteria weakened. Final five-file independent review identity: `b411e95efe12d5430bd20d40c35d5752f80ff531fc4497a2736acb9fbdf9921e`. Evidence: `scratch/UI-304/negative.log`, `focused1.log`, `focused2.log`, `focused3.log`, `review.json`; final source report `scratch/check-change/runs/20261025T171347Z-30087/report.json`.

After these checks, version advanced once from 0.1.61 to 0.1.62; the release job requires the tested source to differ by exactly Cargo.toml/Cargo.lock and builds the matching stable source copy. Job/session 50731; log `scratch/UI-304/release-job.log`.

## Installed result

One release build finished in 4m43s; background session 50731 completed build, ad-hoc signing, candidate verification and recoverable installation. The tested source differs from release source only by the required Cargo.toml/Cargo.lock version bump. Installed `/Users/aji/.local/bin/ca --version` reports `ca 0.1.62`; candidate/staged/installed signatures, hashes, owner/mode and help/version pass. Installed SHA-256: `6a4e3ab708cb36ffe4d4ca5469fd5949fe75237e5575cfcad2809519f2bbb231`. Recovery: `/Users/aji/.local/bin/.ca-release-stage.WO0xvy/ca.recovery-ORC258-20261025T171959Z-1a3692f6904a688aff741f74dbabc22e11226fb945e1bb66f3a2d3cf33e0fe81`. Logs/receipt: `scratch/UI-304/release-job.log`, `release/install.log`, `release/receipt.json`.

Actual installed TUI uses credentialless loopback providers and disposable HOME/workspace with 192 historical messages. Normal and stopped runs each show first request pending, then filtered latest file result in the first eight pane rows while request 2 remains withheld; latest generation/working state stays visible. The compact status is at row 0 and the title/provider heading occupy the reclaimed row 1. The stop run also shows the authoritative Stopped status and complete local explanation in the actual `/filter` pager; both runs exit zero and restore termios. Captured terminal frames were read back and compared. Source/config/history fixture hashes and the installed binary are unchanged through acceptance. No production configuration, saved session, or unrelated running process was operated on.

The first stopped fixture failed cleanup (exit -15 / unrestored termios): it sent `/filter` immediately on seeing the stop row, before the active turn settled, and its initial predicate mistook the duplicate pane notice for an open pager. Preserve this failure. The corrected driver explicitly waits for the turn to settle, requires `q/Esc close`, closes the pager, then exits; `installed-stopped2/receipt.json` passes. This was driver synchronization; no product changes/rebuild followed. Normal receipt: `installed-normal/receipt.json`.

Coverage bounds: all four states, widths, short heights, theme severity, Unicode and same-UI empty→active→stopped projection are deterministic source checks. Installed live evidence covers Active→Stopped, full stop pager, and pending preview freshness at 100 columns. No full workspace gate or paid/external provider result is claimed. Final evidence: `scratch/UI-304/final-assessment.json`.

## Changed files

- `src/provider_filter.rs`: typed presentation snapshot and complete stopped pager explanation; enforcement unchanged.
- `src/app.rs`, `src/tui/event.rs`: coherent process-local snapshot through existing draw refresh.
- `src/tui/render.rs`: compact status row, severity tokens and cell-aware ellipsis; provider pane chrome/content/controls retained.
- `src/tui/render_tests.rs`: status matrix, transition/Unicode/theme tests and updated preview geometry expectations.
- `Cargo.toml`, `Cargo.lock`: required 0.1.62 release bump only.
- `CONTINUITY.md`, `NEXT_STEPS.md`, this plan: completion evidence.
