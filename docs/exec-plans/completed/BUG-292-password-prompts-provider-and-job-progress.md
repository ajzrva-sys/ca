# BUG-292 — Password prompts, DeepSeek continuation, and job progress

Status: completed and installed as signed ca 0.1.27 (2026-10-15).

## User intent and decisions

Implement the approved plan: ordinary self-installation and Desktop exports use
the user's permissions; approved outside-workspace folders last only for the
current live session; genuinely privileged commands reuse normal sudo expiry.
Preserve DeepSeek reasoning on all completed assistant replies, expose bounded
rejection detail, and show live managed-job output/timing without model polling.

## Grounded baseline

Source inventory: `scratch/BUG-292/baseline-source.json`; before copies:
`scratch/BUG-292/before/`. Installed ca is 0.1.26 in the user-owned
`/Users/aji/.local/bin/ca`. No `.git` or `.agent/PLANS.md` exists in this source
directory; this plan follows the existing ExecPlan structure without repairing
that unrelated documentation discrepancy.

Existing VAL-288 gate failures are independent of these fixes. Preserve raw
results and compare failure identities; do not weaken validators or snapshots.

## Implementation and ownership

- Host integration: transient folder-access tool and approvals, effective write
  roots, lifecycle clearing, cached privilege dispatch, process-local error
  details, and bounded job preview/timer refresh.
- Privilege worker: normal timestamp probe and secret-free cached invocation;
  remove forced invalidation while preserving owned-process cleanup.
- Installer worker: ordinary build-source transaction, signing, recovery,
  readback/rollback, and blocking-I/O isolation.
- Provider worker: completed DeepSeek reasoning capture/replay, source isolation,
  bounded safe HTTP excerpts, and credentialless conversation regressions.

Changed code is limited to `src/{main,app,panes,headless,interaction,workspace,
write_access,privilege,binary_install,turn_recovery}.rs`,
`src/agent/{registry,tools,subagent,provider,client,client_tests}.rs`,
`src/tui/{render,render_tests}.rs`, and `tools/bug292_pty.py`. The new folder
grant is process-local, separate from persisted workspace roots, revalidates
folder identity, honors explicit deny/read-only rules, and cannot be widened by
child agents. Blocking install/sign/readback work runs outside the async loop.
Signed-download validation in `src/self_update.rs` is unchanged.

## Acceptance and validation

Use temporary directories, disposable profiles and loopback providers. Test one
folder approval for native and sandbox writes, denial and session expiry,
bounded child authority, cached/expired/rejected sudo authentication, installer
signing/recovery/rollback, DeepSeek multi-turn reasoning and provider isolation,
and quiet/mixed-output job rendering at wide/narrow/short sizes. Keep pending
tool results pending and preserve existing bounded-await behavior.

Run focused checks during repair and `./scripts/verify.sh` after stable source;
inspect report/evaluation and logs. Three automatic repairs per distinct failure
maximum. Required full gate failures remain failures, with prior identities
classified separately. Update harness documents with actual results.

## Release and recovery

After validation, advance the root package patch once and launch the required
workspace/all-feature release build asynchronously. Preserve the installed
binary under its old SHA and timestamp, sign and verify the candidate/staged
artifact, atomically replace, then verify installed hash/mode/owner/signature,
version and help. Restore the recovery copy on post-install failure. Leave
running sessions, runtime configuration, production conversation data, and
oracle/reference evidence intact.

## Progress and evidence

- [x] Confirmed approved choices and captured source baseline.
- [x] Focused regression tests and minimal implementation.
- [x] Completion gate and independent review (raw gate remains baseline-failed).
- [x] Signed release installation/readback and final handoff.

Validated source: `017c7517a42f21bcc0d7d54c24c5b9f3f484739b187869c63480d9a3320317f4`. The complete V5 gate
finished on unchanged source and unchanged criteria. Preflight, 117 workflow
contracts, formatting and the all-feature release build pass. Default workspace:
4,029 passed / 157 failed / 43 ignored, 29 targets. All features: 4,185 passed /
157 failed / 44 ignored, 30 targets. Exact failed identities and all 60 strict-lint
diagnostics match the retained baseline, with zero new or missing. Both matrices
have zero outer filters. The official runtime matches its existing seven passes,
Reviewer deadline failure and dependent resize not-run; child exit and terminal
restoration pass. Raw report and evaluator remain **failed**, not green.

All 193 final focused tests pass (zero failed/ignored), with source/executable
binding in `scratch/BUG-292/focused-final-v5/receipt.json`. Earlier compile,
canonical-path, rollback-fixture and old catalog-count failures remain recorded.
The rollback assertion now verifies exact original uid/gid/mode, and the strict
catalog count includes exactly one new tool with explicit membership. Production
behavior was unchanged by those final two test corrections. V1–V4 interruptions
and any shutdown-induced test failures are retained as incomplete/failed runs.
No established snapshot, criterion or failure exclusion was relaxed.

Final evidence: `scratch/verify/BUG-292-final-v5/`,
`scratch/BUG-292/matrix-baseline-comparison-v5.json`, and
`scratch/BUG-292/gate-final-assessment-v5.json`. The separate assessment records
all baseline failures and `full_verification_passed=false`. VAL-288 remains the
unrelated baseline-repair task.

`scratch/BUG-292/debug-pty-final/receipt.json` passes actual one-approval native
and Seatbelt Desktop exports, mixed job output, advancing elapsed time without
provider polling, zero password prompts, unchanged disposable configuration,
clean child exit and restored terminal settings. The debug artifact is
`a1937414d0df53269aaab813372f6dc14c9e512479a7758369f0cf3e162abc43`.

The same build successfully replaces its own disposable executable without a
password or terminating its running process. Its exact signed hash, old-hash
recovery, permissions, signature, version and help are verified in
`scratch/BUG-292/self-update-debug-v3/supplemental-readback.json`. The raw V3
fixture remains failed: it incorrectly demanded preservation of an incoming
signing identifier. That condition was unrelated to required integrity and is
removed only from the scratch fixture for future runs. Earlier V1 exposed and
led to a real inherited-group fix; V2's 90-second timeout during hashing of a
377 MB debug executable is retained. The corrected release fixture below supersedes that fixture limitation.

## Installed release

The root package advanced exactly once from 0.1.26 to 0.1.27. Background job
**13322** exited zero: `scratch/BUG-292/release-install.log` and
`release/transaction.log`. The signed release is installed at
`/Users/aji/.local/bin/ca`; mode 0755, uid 501/gid 20. Candidate, same-filesystem
stage and installed SHA-256 all match:
`80cfdddb371e2150fd2c7f689495cbd9b9932d521dee6a9f3977ba45b119bc17`. Actual signature, `ca 0.1.27`, and complete help pass.

Recovery: `/Users/aji/.local/bin/ca.recovery-BUG-292-20261016T012846.008616Z-2506ada15e6a09104faa9559e134e3d713ad82c9c952bd65c9c62f380296673e`.
Its bytes and filename match the old hash `2506ada15e6a09104faa9559e134e3d713ad82c9c952bd65c9c62f380296673e`.
Atomic replacement completed; rollback was not required. Running sessions,
production runtime configuration/conversations and oracle/reference contents
were left intact. The tested source differs from release source
`1e088141231e923e328bcb124a069fcb65ce0b919929a3b3d518324a35d01035` only by the exact root package
version fields in Cargo.toml/Cargo.lock; `release/source-binding.json` verifies it.

The exact release passes native self-replacement (two loopback requests, zero
password prompts, unchanged config/source/artifact, same PID, verified signed
old-hash recovery, clean exit/restoration). Both candidate and installed binaries
pass the six-request export/progress terminal workflow: one folder confirmation,
native plus actual Seatbelt writes, advancing timer, stdout/stderr, no model
polling, and intact composer controls. Receipts are
`release/{candidate-self-update-pty,candidate-pty,installed-pty}/receipt.json`.
Manual readback of the installed progress frame confirms all five preview rows
and Esc/Enter/Alt+Enter controls remain visible. Transaction receipt:
`scratch/BUG-292/release/installed-readback.json`.

Start a fresh ca process to load the release. Existing DeepSeek conversations
whose reasoning was already discarded can require `/new`; the screenshot's
exact server reason remains unproven, and no live paid-provider success is
claimed. No work remains for BUG-292; existing baseline repair stays VAL-288.
