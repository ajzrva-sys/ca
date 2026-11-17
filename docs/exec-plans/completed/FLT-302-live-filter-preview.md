# FLT-302: latest filtered content during work

Status: complete; installed ca 0.1.48 (2026-10-24)

## Acceptance and diagnosis

The normal entry point is installed ca's FILTER ON / TO PROVIDER side pane.
Each request is already published before HTTP and copied into the pane on draw,
but the preview starts with old fetched pages/history and resets to that start.
Requests above 512 KiB are sliced before JSON parsing, losing the latest tail.
Show newest request content first while a reply is pending, retain each item's
text order, keep instructions last and preserve filtering, manual scrolling,
narrow-terminal behavior, configuration, canonical history and running sessions.

## Work

- [x] Inspect publication, rendering, input and existing acceptance tests.
- [x] Add failure-first latest-content and large-request regressions.
- [x] Repair preview ordering and add a current-request footer.
- [x] Freeze/review combined changes and pass focused/default checks; remaining full gate stopped by explicit user instruction.
- [x] Bump once, build in background, install recoverably and verify installed TUI.

## Verification

Use disposable source/application state and credentialless loopback requests.
Reuse the current passing CTX-301 fullv3 baseline with unchanged gate criteria.
Keep failed evidence. Tests cover multiple requests, old/new fetched and ordinary
results, preserved block order, a request larger than 512 KiB, scrolling and
80/100-column renders plus narrow hiding. A delayed terminal fixture must show
the new filtered result before completion. Full gate is scripts/verify.sh.
Evidence: scratch/FLT-302/. This checkout has no .git or .agent/PLANS.md;
before-images and source inventory hashes identify exact changes instead.
This is a ca preview behavior repair; no Cursor parity claim or oracle edit.

## Permanent acceptance correction requested by the user

The original FLT-274 plan explicitly required fetched-page-first preview order
(completed plan lines 79–82); FLT-287 required wrapped rows to be reachable by
scrolling (completed plan line 17). Preserved original tests asserted
page < user < file < system. The installed PTY receipt checked narrow hiding,
request filtering/restoration and sticky stop behavior, without a second-update
visibility check. This is evidence of an inherited acceptance gap, not evidence
that a prior agent recognized and deliberately ignored the defect.

AGENTS.md now requires visible second-update acceptance before completion with
representative long history, and current-scope repair of known workflow blockers.
The user's explicit request for permanent logic correction is also saved under
memories_v2/extensions/ad_hoc/notes/2026-10-24-live-progress-visible-freshness.md.

## User-directed installation before the remaining full gate

The 69 focused tests pass on frozen source `4d0df88b976a5267ab512e12b15d7004ea812772f1491db2863becc7e0848bcf`
(copy `23be077bfd52600c4e9f9c45f41ccc1bca1d45548aab7133d959772e7986343a`).
Independent combined source review approves. Preflight, 118 verifier tests,
formatting, strict workspace/all-target/all-feature lint and complete default
workspace tests pass. The old installed binary's genuine withheld-response
fixture completes its two requests but fails visible freshness as expected.

The user then explicitly ordered immediate installation. The remaining full
gate was interrupted in all-features testing; the final full-gate and evaluator
status is infrastructure_failed/incomplete. Interruption-related test output is
retained. Neither the unfinished all-features matrix nor gate release/runtime
stages is claimed passed. This explicit user direction overrides waiting for
that remaining gate. Do not resume optional verification automatically.

One root patch bump advances 0.1.47 to 0.1.48. Release source digest
`e119ef8606348dbd11ad9dab46941b5c8c63fea59b78f67f3c95c359f533d706` differs
only in the exact Cargo.toml/Cargo.lock version patch. Tool-managed background
installation job 99118 builds the release and performs signed recoverable
replacement plus candidate/installed version/help and pending-TUI readback.
Actual installation result follows; it is not yet claimed complete.

## Final result and procedure repair

## FLT-302 — installed ca 0.1.48; prompt testing/install procedure (2026-10-24)

The provider-copy pane shows newest messages/results first, keeps system
instructions last, preserves the newest tail in large requests, and displays
request generation/working state. Installed pending-response TUI proof passes:
filtered result is visible in the first eight pane rows before response 2 is
released. Prior 0.1.47 is recoverable; signature/hash/mode/owner/version/help
checks pass. Production configuration, saved sessions and running sessions
were not operated on.

69 focused tests, independent review, formatting, strict workspace lint and
the complete default suite pass (4,401 passed, 0 failed, 43 ignored, 29 targets).
The user explicitly stopped the remaining all-features/full-gate work and
ordered installation. That gate/evaluator is incomplete/infrastructure-failed,
not passed; all evidence remains. Background release/install job 99118 finished.

Earlier acceptance preserved fetched-page-first order and tested scroll
reachability, missing live visibility. AGENTS.md and the user-requested memory
note now require a visible second update during pending work with long history.

The user also requested faster testing/installation. scripts/check-change.py
now provides a locked stable disposable source slot, private stable Cargo cache,
changed-file-only copying, fresh application state, explicit executed-test
coverage, focused lint/tests and one optional ca-only all-feature release build.
The full verifier is unchanged and available when needed, rather than mandatory
before every small repair. Reuse the exact built artifact for installation;
serialize Cargo work and avoid a second build. Six helper contracts and
independent review pass. Actual preparation over the full source inventory:
32.358s for initial private-cache seeding, 0.296s warm with zero source files
copied. This measures preparation, not a cold/warm Rust compilation benchmark.
No Rust build/version bump was repeated for these procedure-only changes.

Evidence: scratch/FLT-302/release/receipt.json, installed-runtime/receipt.json,
gate-full1/report.json, gate-full1/evaluation.json, focused-launch-final1.json,
procedure-review.json and scratch/check-change/runs/. See the completed
[plan](docs/exec-plans/completed/FLT-302-live-filter-preview.md).


Changed product files: src/provider_filter.rs, src/tui/provider_pane.rs, src/tui/render.rs and src/tui/render_tests.rs. Procedure files: scripts/check-change.py, tools/test_check_change.py, AGENTS.md and docs/VERIFICATION.md. Harness/index updates record the exact completion and remaining coverage. Signed installed SHA-256: 8de5f736f218acc6c206697273b0f0bc8359e29c7e5ecdf304bb1b395178978b. Recovery: /Users/aji/.local/bin/ca.recovery-FLT-302-20261024T131246Z-f6e58fa68712717f609dfc2834dacbe0eb5288023c0ec77469c355bb849a2963.
