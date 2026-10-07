# BUG-257 — Long-session responsiveness and bounded failure recovery

Status: ACTIVE — implementation complete; duration/network validation and installation pending
Owner/session: Codex, 2026-10-04

## Goal

Implement the user-approved plan for all screenshot/review findings: bounded
event scheduling, cached visible transcript rendering, input-safe theme
fallbacks, one safe automatic replacement with retained failure diagnostics,
and pricing admission suppression. Preserve all pre-existing work.

## Evidence

Review: screenshots ca-error.png through ca-error4.png in the workspace root.
The same unbounded FIFO carries keys and 50 ms ticks; every tick redraws.
Activity header layout repeatedly clones/wraps transcript prefixes. The
isolated optimized layout probe took 67/264/1050 ms for 1024/2048/4096 lines.
Runtime logs record 216 slow frames since October 1 and a 4527 ms frame.
Deferred draws bypass the frame interval; direct OSC11 reads can swallow keys.
Provider footers discard failure categories; idle pricing failures recur.

## Scope and invariants

Allowed: root application/TUI/provider diagnostics, focused tests and scripts,
repository harness docs. Oracle/reference trees and unrelated dirty files are
evidence only. No production provider calls, config or session changes for
validation. Implementation may create recovery sessions during normal use.
Keyboard/paste ordering, Unicode wrapping, selection, scroll anchors, tool
authority, cancellation generations and accounting must remain intact.

## Decisions

- User chose one automatic replacement only when no tool/side effect started.
- User chose safe environment/cached theme fallback, with no OSC11 queries.
- Replacement preserves the failed source branch and uses the frozen route;
  no hook/tool replay. Permanent errors and uncertain execution stay manual.
- Styled layout cache budget is 32 MiB per pane; canonical history is intact.
- Missing verified prices block experiments, never ordinary chat, with one
  notice per unchanged admission configuration.

## Milestones and progress

- [x] Event scheduling, draw limiter, theme safety and focused tests.
- [x] Linear row indexes, visible rendering, bounded pane cache and tests.
- [x] Off-loop/throttled maintenance and pricing admission tests.
- [x] Safe replacement, errors viewer and metadata diagnostics tests.
- [x] Focused tests, fmt, strict Clippy, optimized workspace release and short PTY.
- [x] Harness documentation and signature/hash/version/help verified candidate.
- [ ] Actual eight-hour soak (running), loopback replacement and full-workspace gates.
- [ ] Installation (write denied here; recoverable normal-terminal script prepared).

## Validation

Focused scheduling/row-layout/recovery/pricing/theme tests, including simulated
eight-hour missed ticks. Fake-provider/PTTY tests use disposable state and
assert no tool/hook duplication, loss of input or late recovery. Release layout
probe repeats the reviewed workload; controlled warm input target p95 <100 ms.
Actual eight-hour disposable soak is required before claiming long-duration
validation. Gates: cargo test --workspace, cargo fmt --check, cargo clippy
--all-targets -- -D warnings, cargo build --release --workspace --all-features.
Separately failing workspace checks are reported without assuming all are
baseline or environment-only failures. No skipped network check is a pass.

## Recovery and delivery

Task baseline copies: /private/tmp/ca-fixes-baseline.8v4l1d. Do not reset the
dirty checkout. Follow standing recoverable/signature-verified installation
after gates; if filesystem policy prevents installation, retain a verified
workspace candidate and report that limitation. Leave running sessions alone.

## Completion record

Source files: `src/{main,app,activity,idle_learning,turn_recovery,terminal_awareness,
runtime_diagnostics,pricing,harness_control,harness_idle}.rs`,
`src/tui/{event,input,render,transcript_cache,commands,mod}.rs`, four intentional
snapshots and disposable `tools/bug257_{pty.py,render_probe.rs,install.sh}`.
Existing unrelated dirty work is preserved. See ARCHITECTURE.md for ownership.

Passing focused run: 566 passed, 0 failed, 1 ignored, 2,151 filtered;
`/private/tmp/ca-bug257-focused-final.log`. Formatting, strict all-target Clippy,
diff whitespace and all-feature optimized workspace build pass. Exact commands
and receipts are in TESTING.md. Background release job 98631 completed.

Controlled signed-release PTY, 4,096 messages: initial/post-zero-idle/narrow
keyboard p95 19.497/19.565/19.676 ms. Ordered paste, `/theme sync` without
OSC11, resize and terminal restoration pass. Receipt:
`/private/tmp/ca-bug257-pty-bmki0xit/result.json`. Standalone release probe
reduced repeated prefix layout from 1,077.73 ms to 10.78 ms at 4,096 lines
with equal viewport buffers; not a whole-app throughput claim.

Eight-hour disposable debug PTY soak running, job 89271, initial p95 20.777 ms:
`/private/tmp/ca-bug257-pty-uds9g6kg/result.json`, earliest finish
2026-10-05 05:22:54 UTC / 01:22:54 America/New_York. Leave it running and
require its actual completion receipt. Simulated missed ticks are separately
covered by tests; they do not replace elapsed-time acceptance. No RSS
measurement available because this environment denies process probes.

Workspace test job 74837 ended with 12 failed targets (main unit 2,521 pass,
157 fail, 40 ignored). TCP binds/nested sandbox/Git commit operations are
denied; other failures include absent ancestor binary/schema fixture paths
and LSP/MCP assertions. The new loopback replacement integration test is
blocked at bind EPERM; network-free recovery tests pass. Log:
`/private/tmp/ca-bug257-workspace-tests-final.log`. No blanket baseline claim.

Signed candidate SHA-256
`6e5e8811c09b5033c33dc43bd16183ae7b9417e1f13620bba210a0d61a2d91ea`,
build digest `32d0b4cad67dfcd419594b06c29c11736f8c8903ec8e73a4fcd4b6ccb34b9591`,
83,560,400 bytes, executable owned by aji, `ca 0.1.0` version/help pass,
strict ad-hoc signature valid. Candidate path: `target/release/ca`.
Installed `/Users/aji/.local/bin/ca` unchanged at SHA-256
`46d60ee15c7a4893c357e75eccd19b8f05eaaeacc1fc52c25cf122e913c07b41`;
destination staging mkdir denied by workspace policy before replacement.
`bash tools/bug257_install.sh --check` performs a read-only preflight;
normal-terminal invocation without it backs up the pinned prior hash, signs
staged/installed copies, atomically replaces and validates or rolls back.
Installer syntax and the read-only preflight passed; replacement/rollback
execution remains untested under the denied destination write policy.
No runtime configs, saved sessions or active user sessions changed for tests.

Next task remains BUG-257: inspect soak, rerun blocked checks in a suitable
environment, install/record the outcome, then move this plan to completed.
