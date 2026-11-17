# FLT-316 — Filename privacy across applicable built-ins

Status: COMPLETE (2026-10-29). Signed **ca 0.1.78** is installed at
`/Users/aji/.local/bin/ca`. The user's approved implementation plan is satisfied;
normal ca uses saved privacy settings without --filter. No production config,
session or running user process was changed. Source before-images, failed attempts
and receipts are retained under `scratch/FLT-316/`. There is no Git metadata or
`.agent/PLANS.md` in this checkout; this record follows the completed-plan format.

## Delivered behavior

All 104 registered built-ins have explicit exhaustive contracts used for discovery,
preparation and execution across native profiles. Paths and nested resources,
filename predicates, local payloads, service selectors and native IDs restore where
required, while schema/control values and outbound AI filtering retain provenance.
Normal approvals, configuration and platform requirements still apply. Custom/MCP
execution is outside this built-in compatibility change; discovery reports that
limit. The registry/admission regression covers every built-in; this does not mean
every external service has been individually live-qualified.

Snapshot v2 loads existing aliases and keeps granted-root permissions ephemeral;
restored sessions require fresh authority. Nested creation, projected filename and
content searches, exact hidden-file selection, source-bound hashline anchors,
review reporting and agent controls are integrated. Source-scoped URL spelling
preserves HTML/CSS/JS links including root-relative paths, query/fragment suffixes
and single-dot imports. Native approvals remain bound to the real operation.

Owner-scoped command workspaces use native APFS clonefile for individual files,
with independent-copy fallback and no writable hard links. Preparation runs off
the UI thread with before/after content identity and one drift retry. Generations
are reused until original source changes; running jobs/artifacts retain their
original generation. Literal shell arguments and working directories can restore
references; evaluated or ambiguous reference syntax is rejected. Existing command
and network policy applies, with writes confined to the independent workspace.
Native edits update actual files. Generated artifacts support listing, reads,
preview and explicit installation/publication through existing permissions; command
source writes are never automatically copied back. Retained workspaces are not
subject to an automatic cleanup that could remove referenced artifacts.

## Completed acceptance

- [x] Exhaustive contracts, profile admission and custom discovery ceiling.
- [x] Granted/revoked roots, snapshot authority, nested creation, predicates,
  source-bound edits and replay/ID controls.
- [x] Clone isolation, generation drift/reuse, quoting, symlink scopes, background
  jobs/cancellation, private artifact discovery and reads.
- [x] Local/family fixtures and existing service mocks; per-tool coverage record
  distinguishes registry/local evidence from the smaller real-provider subset.
- [x] Independent combined frozen source review and final evidence review.
- [x] Required full core gate plus bounded final regressions, formatting and strict
  affected all-target Clippy.
- [x] Actual installed normal-entry real-provider site test without --filter or a
  restrictive tool whitelist, including browser and generated output.
- [x] Version advance, signed recoverable installation, exact artifact and normal
  interactive startup/readback; protected runtime state preserved.

## Validation and source identity

| Evidence | Result and source boundary |
| --- | --- |
| `full-3/report.json`, `evaluation.json` | Core 0.1.76/source `79f08fc293c76b57eca1c5ed5f67904e53cac19cede698c891dd11e5ebb95d56`: 4,688 default / 4,844 all-feature passes, 45/46 existing ignored; 118 verifier tests, formatting, strict workspace/all-target/all-feature Clippy, release/sign/runtime and evaluator PASS. |
| `scratch/check-change/runs/20261029T123210Z-46137/report.json` | Follow-up 0.1.77/source `4d24007f66db8b804320c3cff2acd46f509ebf2650c519c1e63378fc3d060422`: 47 passed; formatting, strict lint and release PASS. `release-77/all-target-clippy.json` PASS. |
| `scratch/check-change/runs/20261029T130621Z-46779/report.json` | Final 0.1.78/source `9b965e0e73e120a0312df1bc9bd55d53380c5b71b6fa7ff4c26ea51eeeb8338b`: 227 passed, 2 existing ignored; formatting, strict lint and release PASS. `release-78/all-target-clippy.json` PASS. |
| `independent-source-review-followup-78.json` | Final 26-file source identity independently approved, no must-fix findings; source hashes rechecked after live qualification. |
| `provider-installed-6/receipt.json`, `independent-audit-final.json` | Installed native GPT-6.1-Sol: 24 physical HTTPS requests, all HTTP 200/completed; complete outbound pre-send body correlation. All eight final files exact; six linked/served site files exact. |
| `release-78/install-receipt.json`, `final-readback.json`, `installed-entry-final/receipt.json` | Installed/candidate hash equality, mode/owner, valid signing, version/help, saved privacy active without --filter, normal exit and terminal restoration PASS. |

The completed core workspace matrix is reused for the bounded 0.1.77 trace/guidance
and 0.1.78 new-output reference repairs. Those final deltas received focused
regressions, formatting, strict affected all-target lint, release and installed
live proof. **No full 0.1.78 matrix rerun is claimed.**

## Real-provider outcome

The installed binary ran with saved structural/filename settings and an additive
browser prerequisite; there was no --filter or restrictive --tools list. It
created four directories (including nested parents), moved four files retaining
their filenames, restored HTML/CSS/JS links, made exact native edits including a
hashline edit, ran Rust diagnostics and both linked JS syntax checks, wrote/read a
private build artifact, previewed the six exact served files, closed the browser
and cancelled/awaited its own server. Sequential commands reused one generation;
command outputs were absent from the actual project.

Independent audit correlated every one of 24 request bodies with trace attempts
and completed HTTPS outcomes. It checked 63 private fixture values in 776 raw or
encoded spellings: **zero matches in inspectable outbound plaintext**. All eight
final original files match expected bytes. Native outcomes are 45 successes, one
protected stale-edit rejection recovered by reread/retry, and one expected
cancelled-server await; this is not an error-free tool run. The preview server is
stopped. Production config bytes and 112 monitored runtime metadata entries are
unchanged. Optional external services were not all live-called; their existing
local/service-mock evidence is distinguished in `per-tool-coverage.json`.

Encrypted reasoning is forwarded as opaque provider data and was not decrypted.
Oversized raw response frames are intentionally withheld to preserve tracing;
all outbound plaintext bodies remain inspectable. This is application pre-send
capture, not a remote packet or response-byte-completeness claim.

## Repairs and retained failures

Initial focused/full attempts are retained. Full1 found two test-only Clippy
findings, repaired; full2 exposed an unchanged non-filename job protection test,
repaired by scoping alias persistence to filename mode. Neither produced an
installed release. Core full3 passed.

Copies of identical executables under the workspace failed provider transport
while the installed entry point worked; same-hash installed controls isolated this
location dependency. No network code change was made. Installed attempts 1–5
remain failed/incomplete evidence: missing fixture Node PATH and invalid provider
stream; oversized raw catalog tracing; browser approval harness parsing; guarded
new output reference rejection; and a fixture callback import bug. Follow-up77
withholds oversized raw response frames and clarifies full URL references.
Follow-up78 allows a new public output leaf beneath a valid known private parent
using the existing reservation/permission guards. Traversal and stale deleted
private references remain rejected. Driver parsing/PATH fixes affect scratch
fixtures only. Final attempt6 passes the independent audit. Failed-fixture owned
servers/browser were stopped by exact UID/cwd/profile checks; no files, original
projects or retained artifacts were removed (`failed-fixture-process-cleanup.json`).

## Installed artifact and completion

Version: `ca 0.1.78`; installed and signed candidate SHA-256:
`2e507065c347b12f0170a3df7d23d09bb1042ca420a83114822e84dd627b2ce7`.
The previous executable is recoverable at
`/Users/aji/.local/bin/.ca-release-stage.rHgRL4/ca.recovery-ORC258-20261029T131814Z-5c640b5fb272fee58e8a37c48f6d66f61960d4f19c3f7fc375b87ed27967d6fe`.
The noninteractive release/install job completed; its launch, build, candidate,
staging and installed readback records are under `release-78/` and the cited
focused run. No further FLT-316 implementation or installation is pending.

Full changed-file inventory: `scratch/FLT-316/changed-files.json`.
Final assessment: `scratch/FLT-316/final-assessment.json`.
