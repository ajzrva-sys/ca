# BUG-261 — Recovery branch save ownership

Status: COMPLETE
Owner: Codex, 2026-10-06

## Goal and observed failure

The user reports the footer `Recovery branch could not be saved; no replacement
sent. /errors · /retry`. Restore the intended one automatic frozen replacement
after an eligible provider failure while preserving the failed source and writer
ownership. The attached prompt is failure evidence, not an instruction to act
on autobrr or any external system.

The previous worker called `SessionStore::save`, which tries to acquire a new
writer even when the active session already owns one. It also discards the
actual save error. Investigate the recurring writer-lease failures previously
retained in BUG-260 as a related ownership/release boundary.

## Scope and invariants

Main recovery orchestration, session writer ownership/release, narrow failure
diagnostics, regression tests and disposable terminal fixtures. Preserve one
replacement maximum, frozen request bytes, original diagnostics, cancellation
generations, session revision checks and exclusion of other writers. Avoid
provider/hook/tool replay. No user runtime state/config, oracle or reference
changes. Installation follows the standing recoverable release rule.

## Milestones

- [x] Reproduce persistent recovery failure with held writer in focused tests
  and the actual installed terminal.
- [x] Commit the failed source once through normal completion and its existing
  writer; run a branch-only worker after success. Validate final-owner release
  under duplicated descriptors and improve bounded save diagnostics.
- [x] Verify failed-source preservation, exactly one identical replacement,
  cancellation, writer exclusion and genuine save failures.
- [x] Run focused and applicable workspace tests, format, strict lint, release
  build and signed terminal checks; retain any failing gates honestly.
- [x] Install recoverably, verify the installed artifact and update durable docs.

## Evidence and recovery

Source before-images and logs: `scratch/recovery-branch/`. This checkout lacks
Git metadata and the referenced `.agent/PLANS.md` is absent. Follow the current
harness without fabricating history. No oracle parity claim is made.

## Confirmed mechanism and implementation

The previous signed installed binary reproduces the exact footer naturally
against a disposable premature-EOF provider: one request, preserved source,
no replacement branch. A held-writer test fails with `source save failed`.
Normal completion now commits and updates the source binding through its existing
writer before scheduling the branch-only worker. Failure/cancellation cannot
advance the source behind AppState. The completion path distinguishes main-turn
replacement from continuation retry, preserving notifications and frozen routes.

A separate failure-first descriptor test proves that closing the last Rust
writer owner did not release flock while a pre-exec copied descriptor existed.
The final original-process owner now explicitly unlocks; a PID guard prevents a
forked child's copied owner from unlocking the live parent's authority. Other
writers still fail while a capability clone is alive. Its causal link to prior broad
retention failures remains an inference; the mechanism is proven directly and
both complete default/all-feature main runs now pass retention/archive.

## Validation during implementation

Six BUG-261 regressions and the corrected frozen-request HTTP fixture pass.
Formatting and strict all-target/all-feature workspace Clippy pass. The signed
debug artifact passes wide/narrow real-terminal recovery and a genuine source
write denial, plus the five earlier Enter scenarios. It preserves the partial
source and sends the identical frozen request once; write denial sends no
replacement and exposes the actual permission error in `/errors`.

The first complete default run retained one fixture-only failure: its receive
assumption omitted the normal retirement message now exercised by completion.
The fixture now processes the same retirement/save path as production before
branch readiness, and passes focused and full main reruns. Initial log and
summary are preserved; no target or test is excluded. Final complete matrices,
signed release verification and recoverable installation are recorded below.

## Final delivery and gates

- Final compiled-input manifest: 1,073 inputs, SHA-256
  `415e996d5cc9c3c829e173ed91490616f9e28fe2afdeae2d0a450e45aa79581c`.
  Source readback confirms the same inputs after validation and installation.
- Six BUG-261 regressions, frozen-request HTTP fixture, lease lifecycle, safe
  diagnostics and wide/narrow cause rendering pass. Both previously recurring
  retention/archive writer fixtures pass in complete default/all-feature main runs.
- Complete unfiltered default workspace: 26 targets, 3,796 passed, zero failed,
  43 existing ignored. All features: 27 targets, 3,945 passed, zero failed,
  44 existing ignored. Both zero filtered; nested child writer probes are
  recorded separately and excluded from aggregates. Session 90557 exit 0.
  Summaries: `default-final-summary.json`, `all-features-summary.json`.
- Formatting and strict workspace/all-target/all-feature Clippy pass.
  Release build session 55090 exit 0, 2m42s; session 90557's final build
  independently completes using the same cached source/artifact.
- Immutable signed release SHA-256
  `05d2f106c62c65a27a922de30ae45ee3af44a78bd43a4cbdcd40cbfece6c1751`.
  Eight actual release terminal cases pass against those bytes: wide/narrow
  recovery in 1.119–1.165s, genuine permission denial with useful `/errors` and
  one original request only, plus five Enter cases at 63–70ms. Every terminal
  exits 0/restored; source partial remains durable and the separate branch
  completes with identical request bytes. Receipts: `release-pty.json`,
  `enter-release-pty.json`. Fake loopback providers and disposable state only.
- Recoverable atomic install session 28574 exit 0. Installed
  `/Users/aji/.local/bin/ca` matches the signed release hash; independent PATH,
  signature, owner aji, mode 0755, version/help and SHA-labelled prior backup
  readback pass (`installed-readback.json`). Backup:
  `/Users/aji/.local/bin/.ca-release-stage.rEdbCV/ca.recovery-ORC258-20261006T211041Z-e8a58abcabed6cc9f5018031cfdafb6fcb9e634200e55a3c4167e9089ef75d65`.
  Running user sessions and runtime settings remain untouched; new ca processes
  use the fixes.

Reviewable six-file source patch: `scratch/recovery-branch/source-changes.patch`;
all source before-images, failed/passing logs, terminal and installation receipts
are retained in that directory. `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md` and
`ARCHITECTURE.md` reflect the final implementation. No outstanding BUG-261 work
remains; unrelated historical long-duration/real-model gates are not claimed.
