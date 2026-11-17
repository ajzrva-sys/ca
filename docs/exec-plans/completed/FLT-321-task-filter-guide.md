# FLT-321 — Guide filtering from task submission

Status: COMPLETE. Signed CA 0.1.82 installed (2026-11-07).

## Goal

Opening ca and submitting a task with a URL or @file offers a local, prefilled filter review before provider submission. Matched cyber terms are suggestions, not a claim about any provider banned-word list.

## Evidence and Current Behavior

CA 0.1.81 is installed. The normal composer expands files then calls continue_tui_submission before hooks and canonical append. /filter opens a Files/Rules/Preview/Settings overview without task suggestions. Profile edits apply only after restart; the process filter pins its first enabled snapshot. These source facts establish the missing guidance and immediate-application dependency. This is a CA extension; no Cursor parity claim.

## Scope and Invariants

Change src/main.rs, src/app.rs, src/provider_filter.rs, src/tui/privacy.rs and a guide module, focused tests, usage/harness docs and release version. Preserve oracle/reference trees, existing stops/aliases/approval boundaries and runtime configuration/session files. Guide state is transient. Apply adds protections to the current process and does not persist settings. Cancel returns the draft without sending. Continued work goes through existing hooks and submission handling exactly once. Queued work keeps its ordering.

## Milestones and Acceptance

- [x] Trace current entry point and identify immediate-application dependency.
- [x] Implement prefilled editable suggestions, URL toggle and clear apply/unchanged/edit actions at wide and narrow widths.
- [x] Integrate held submissions and queue handling before provider work; plain tasks without candidates remain direct.
- [x] Verify additive filtering, inherited trips/aliases, no-send cancellation and exact-once continuation with focused tests and captured local provider traffic.
- [x] Frozen source independently reviewed: b05088f26dd41fa52ce5ac55986a433ef219b5cf5fb35e286a1694612c90c33d. All 111 focused all-feature tests, formatting, strict lint and the single versioned release build passed.
- [x] Qualify terminal entry, install recoverably with signing, read back installed entry and preserve protected work.

## Validation

Use scripts/check-change.py with focused guide, owner-submission and process-overlay selectors; --all-features --release after stable source/version bump. Use disposable PTY state and a loopback provider, wide/narrow layouts and two pending preview updates with long history. Full workspace matrices and external-provider refusal outcomes are not claimed by focused coverage. Preserve all failed receipts.

## Recovery and Discoveries

The root is not a Git checkout; baseline copies are under scratch/FLT-321/baseline. Root .agent/PLANS.md is absent; the existing ca-hrs-252/.agent/PLANS.md supplies the plan format. Candidate installation uses tools/bug257_install.sh with timestamped recovery bytes and signed hash/version/help checks. Do not mutate running sessions or runtime defaults.

## Verification attempts

- Initial prepare-only receipt succeeded. The first test launch failed before Cargo because the new root target did not yet exist and its prepared symlink was dangling; creating target repaired setup without changing the verifier.
- Two cold compile runs were deliberately superseded during independent review, before test completion. Their failed/interrupted reports remain in scratch/check-change/runs/20261107T144409Z-83022 and 20261107T144505Z-95852. They are not passing evidence.
- Review caught and repaired case-variant duplicate suggestions, URL-host wording, queue cancel save/retry, held background completion admission, and preservation of legacy stop rules. Queued MCP origin preservation was finalized and independently reviewed before the final source freeze.

- The first complete compile attempt failed on child-module method visibility. Three methods used by the parent were changed to pub(super); the failed report is retained. The rerun passed 111 focused all-feature tests and formatting; strict lint and the single release build passed. Final report: scratch/check-change/runs/20261107T144838Z-10705/report.json.

- Final source checks all passed. One all-feature release build completed in 274.437 seconds; signed candidate SHA-256 is 9518ca2ba79334d1752b42ca482d8853ebd7a2444408cc7587c5191c0cb778ff (unsigned build receipt hash b5d35eff96604abc1aa98a58aac5c5c141466a4ddb8b938a83cc2431f1536339).
- Native candidate walkthrough confirmed wide/narrow prefilled review, no-send cancel, exact-once filtered request, unchanged original bytes and 192 retained history messages. Initial script receipt captured partial screen frames; that receipt alone is not accepted as preview evidence. A stable-frame rerun displays request 1/FIRST-PENDING and request 2/SECOND-PENDING at the top, but its assertion incorrectly forbids FIRST-PENDING anywhere in retained history. Raw failed receipt/screens remain under candidate-run2; independent review confirmed that the older task is legitimate history and the newest task must lead the content panel. The scratch validator now waits for a complete frame and asserts the expected pending generation plus the first content row; it preserves the old task below the new one. The corrected same-artifact rerun passed in candidate-run3. No application source change is indicated by those frames.

## Completion Record

Entering a task with a URL, @file or matching cyber/privacy text opens a local, prefilled review before hooks, canonical user-message append or provider work. Terms can be toggled, added and edited. Enter adds the selected protections immediately and continues once; U retains existing filters and continues; Esc restores the task without sending. Ordinary tasks without candidates remain direct. Existing /filter controls remain available, and the overview points to the task-first path. Selected terms match regardless of ASCII case; URL hosts are masked while paths/query/fragment are preserved. Original files and saved filter settings are unchanged.

Changed source: src/main.rs, src/app.rs, src/provider_filter.rs, src/tui/privacy.rs, src/tui/privacy/guide.rs. Cargo.toml/Cargo.lock advanced to 0.1.82. Usage: docs/CONFIGURATION.md. Harness: CONTINUITY.md, NEXT_STEPS.md, TESTING.md, ARCHITECTURE.md, PARITY.md, docs/.INDEX.md and this plan.

Final focused command: python3 scripts/check-change.py --all-features --selector task_filter_guide --selector filter_guide_ --selector session_suggestions --selector tui::privacy::tests --selector provider_filter::tests --selector privacy_block_keeps_background --release. All 111 tests, format, strict lint and release passed; source_stable=true. Report: scratch/check-change/runs/20261107T144838Z-10705/report.json. Background build log: scratch/FLT-321/final-check2.log. Independent final-source review approved the exact digest.

The signed candidate passed real 140x40 and 60x24 terminal entry, editable prefill, no-send cancellation, exact-once filtered HTTP body, unchanged source bytes, U unchanged, terminal restoration and two visibly fresh pending requests with all 192 representative history messages. Stable screenshots show request 1 / FIRST-PENDING and request 2 / SECOND-PENDING as the first content row. Corrected qualification: scratch/FLT-321/candidate-run3/receipt.json; prior attempts remain retained.

Installed at /Users/aji/.local/bin/ca, SHA-256 9518ca2ba79334d1752b42ca482d8853ebd7a2444408cc7587c5191c0cb778ff. Candidate/staged/installed bytes, owner, executable mode, signature, version and help passed. The installed command passed the same terminal walkthrough: scratch/FLT-321/installed-readback/receipt.json. Install logs: scratch/FLT-321/install-preflight.log and scratch/FLT-321/install.log. All 4,457 protected configuration/data/session metadata entries were identical around installation and readback: scratch/FLT-321/protected-comparison.json. Running sessions were left alone.

Recovery: /Users/aji/.local/bin/.ca-release-stage.Zt4BvM/ca.recovery-ORC258-20261107T145840Z-4c50f5e2a646948e5b7e306c4f13ef55dc5f39c02ec2b147682b04dbf1cdb73c.

The full workspace gate and external-provider cyber/refusal outcomes were not run. Suggestions are matched local terms, not an exhaustive detector or a known provider banned-word list. No remaining FLT-321 work.

