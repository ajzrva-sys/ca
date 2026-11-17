# FLT-307 — Interactive private file tasks

Status: COMPLETE for the qualified macOS/APFS workflow. Signed ca 0.1.65 installed; final evidence below.

## Contract

The normal ca conversation binds quoted and @ paths locally before its first
provider request. Providers receive stable anonymous folder/file/selection
references. All participating names and paths are private. Rust enumerates and
executes a frozen batch through the existing native automation engine. Preserve
relative subfolders, skip collisions, support local/mounted and cross-filesystem
moves, preserve symlinks/native names and existing permission checks. No separate
user command or service. Simple batches do not fan out to model workers.

One host broker owns mappings and private journals. Direct helpers and contained
automation/repository workers share that binding; a missing binding fails closed
at transmission. Persist intent, reconcile filesystem identities after crashes
or receipt-save failure, never replay a completed move. Cancellation parks the
remainder at a recoverable boundary. Existing local UI shows useful real paths;
provider previews stay anonymous and progress stays responsive.

## Work and evidence

- [x] Preserve baseline/source before-images in scratch/FLT-307/before.
- [x] Add versioned private resources, selections and durable move executor.
- [x] Bind the interactive prompt and add select_files/move_files with typed
      authority, batch approval, progress and recovery.
- [x] Share the broker with contained workers and repository model clients.
- [x] Test >=1,200 adversarial nested/native-name files, same/cross filesystem,
      collisions, symlinks, changed sources, save faults, crash/restart/cancel.
- [x] Real-provider move plus contained model worker; inspect all physical
      requests for private and encoded spellings, independently verify files.
- [x] Normal wide/narrow interactive pending progress with long history and
      two visibly distinct updates.
- [x] Freeze combined source, obtain independent review, focused checks and
      unchanged complete workspace gate. Preserve failures and source identity.
- [x] Bump root patch once; reuse one release artifact, recoverable signed
      atomic ca-only install, installed interactive/real-provider readback.

## Limits and decisions

User configuration, existing sessions and running processes remain preserved.
Tests use disposable application state and synthetic files. Real authentication
is resolved only by ca runtime; drivers never read credential values. No Git
metadata or .agent/PLANS.md is present; use before-images and source inventories.
This is shared protocol/persistence work, so the complete gate is required.

## Progress

2026-10-27: Source tracing confirms native automation has durable run/resource
ownership, but no file-move operation. Text undo is bounded and unsuitable for
binary batches. Direct helpers share privacy in process; contained/repository
clients need explicit broker admission and restoration before requests.

2026-10-28: Focused application checks pass, including first-request binding,
relative/escaped/@ file resources, frozen replay, shared IPC worker restoration,
private metadata protection, member read-only/persistent permission ceilings,
and native-control provenance. Independent read-only review approved combined
source f285705a1adbdf4d4f01692fc53b36acd31503c78aad73fc0113cae0f12f100a;
review receipt lists subsequent lint-only corrections and empty-binding skip.
Engine check-7: 28 passed/1 explicit cross-FS check ignored, including 1,203
actual nested native entries, real process crash/restart and cancellation.
Cross-FS check-8 explicitly passed on distinct native APFS volumes. APFS rejects
invalid UTF-8 filenames with EILSEQ; native byte serialization is covered, actual
invalid-byte movement requires a filesystem supporting those names. Existing
Docker daemon is unavailable; no VM/service was started.
Wide normal interactive check moved 1,203/1,204 PDFs, preserved its collision and
unselected file, used one batch/three physical requests with no private spellings,
and showed counts 1 -> 4 while pending with 192 prior messages. Its test driver
failed terminal restoration by terminating after an active-turn Ctrl+C; the
follow-up uses normal Ctrl+D exit. Do not count this partial receipt as passing.
No real provider FLT-307 run or installation has occurred yet.

2026-10-28 live qualification: final focused check-9 (29 tests), formatting and
strict workspace/all-target/all-feature lint pass. Narrow normal interactive
`native-narrow-1` passes all checks with 1,204 PDFs: 1,203 moved/one collision,
untouched TXT, two pending updates, three physical requests, no private raw or
encoded names, one native operation, normal exit/termios restoration. Real run 1
has the same file/privacy results but correctly FAILS contained-worker coverage:
spawn_agent rejected an explicit empty tool list. Permanent repair now admits
an explicit [] with zero tools; invalid/non-string/unavailable grants still
fail. Focused check-10 passes 13 tests including no-file/no-command authority and
existing preset/grant restrictions; signed debug candidate-2 runs real run 2.
An automatic approval reviewer rejected a scripted test insertion over perceived
truncation risk; the test was added with a bounded patch instead. No source was
lost and no user permission was needed for the safer authorized edit.

Real-provider run 2 PASS (`real-provider-2/receipt.json`): six physical native
openai-codex/gpt-6.1-sol requests, one from the contained model reviewer; zero raw
or encoded private spellings, no trace omissions, 1,203/1,204 PDFs moved/one
collision, protected TXT unchanged, normal terminal restoration. Independent
host check additionally confirms exact selection/destination refs in that child,
privacy-context native admission, one model move call and one native engine
action. Final reviewed pre-version source a737924931e5389b0030ecd6839648e440b80a746f03b8e794cf001e43524fc2.
Version bumped exactly once to 0.1.65; frozen source
b90c1fd8e17ac3eed78e68df6cfd2c61d8b9f658cab9fb04e6c2e63a1c466345.
Unchanged complete gate running in background session 72394, log
`scratch/FLT-307/full-gate.log`. All 102 production config/session metadata entries
remain unchanged before installation. Release candidate and installed real tests
will reuse the gate's single signed release artifact; prior ca remains installed.

Full gate 1 was stopped before any release build after five exact registry/profile
expectations failed because they omitted the two new approved tools. Focused
reproduction retains all assertion diagnostics in contract-failures/failures.log.
Updates retain every prior count/uniqueness/schema/profile assertion, adding only
select_files/move_files and valid examples; independent review explicitly
confirmed no weaker criteria or production change. Focused contract-check PASS:
18 tests. Final source 69b848b300e0e28478db55cc9e4200ee6c2dc865be99bac047cc1659d42b0733,
version still 0.1.65. Complete unchanged gate 2 now running; log
scratch/FLT-307/full-2-gate.log. The first gate/partial report remains unqualified.
No release binary has yet been built or installed.

Gate 2 default workspace matrix PASS (886.099 seconds); no failures observed.
All-features matrix is running. Required release build has not yet occurred.

Gate 2 all-features workspace matrix PASS (891.607 seconds). The one intended
release build has started. Both matrices, complete format/strict lint and workflow
contracts passed on the final reviewed source. Installation is still pending.

Gate 2 all actual stages passed, including release build/sign/runtime, but the
unchanged evaluator REJECTED the new ignored cross-filesystem probe relative to
the FLT-306 baseline. Report/evaluation remain failed; no install occurred.
Independent review required making that probe mandatory, retaining every device,
8 MiB, metadata, symlink and fault-point assertion. It now defaults to canonical
disposable build output and isolated TMPDIR, asserts distinct devices and fails
if unavailable. No evaluator/production changes. Mandatory direct probe PASS
in mandatory-cross-check-3 (8.63s). Earlier fallback traversal failure and stale
Cargo mtime artifact retry are retained; only disposable source mtime was touched
to force exact-source recompilation. Final source
2ca2f1618844d329a1083c4b3e4c7501daa5cd4e8e60105bf838481101b15bad.
Gate 3 now running (full-3-gate.log). Build identity hashes test source, requiring
one necessary replacement candidate after this concrete failure. No second patch
bump, no installed artifact yet; version stays 0.1.65.

Gate 3 default matrix and mandatory actual cross-FS probe passed; stopped before
release after an ordinary workflow defect was reproduced: unfiltered read_file
file_report.txt was interpreted as a private reference (normal-entry exit6).
Reference recognition now requires exact 32-hex ID shape and an active local or
remote private context. Ordinary prefix filenames and UUID-shaped literal names
remain normal when unfiltered; forged IDs remain rejected in private mode.
Focused compatibility/private tests PASS (13), and normal-entry before/after
receipts show failure then exact ordinary file contents. Independent final source
review PASS: 5c3b20ee32c459a47660179c3eef86cd5ae35d4a72154f068398d00e4698e236.
Gate 4 now running on frozen source, unchanged evaluator/baseline, mandatory
cross-FS test. Version stays 0.1.65, installed remains0.1.64. Install wrapper also
requires normal unfiltered filename proof on candidate and installed artifact.

## Final outcome — 2026-10-28

Signed ca 0.1.65 installed recoverably at /Users/aji/.local/bin/ca. The normal
interactive conversation owns private binding and native select/move execution;
no separate command/service or installed backend executable was added. Optional
contained review shares the same broker and makes its own filtered provider call.
Ordinary unfiltered file_/folder_ filenames remain literal.

Final reviewed source: 5c3b20ee32c459a47660179c3eef86cd5ae35d4a72154f068398d00e4698e236.
Final gate 4 and unchanged evaluator/baseline PASS: 4,618 default checks (45
existing ignored probes), 4,774 all-feature checks (46 ignored), format, strict
workspace/all-target/all-feature lint, release, signing and native runtime.
Actual cross-device metadata/symlink/fault coverage runs mandatorily in both
matrices. Same-device, cancellation, crash/restart, saved-receipt failures,
replacement protection and no-overwrite cases pass. No old assertion or baseline
was weakened; interrupted/rejected gates and necessary repairs remain retained.

Candidate/staged/installed SHA-256:
590f3e46d787ef8fa455417b45f911e87db4c321dae6c3336df8ab00a7df6a49.
Candidate and installed normal interactive real-provider tests both PASS: six
physical openai-codex/gpt-6.1-sol requests, one from a contained model reviewer,
shared selection/destination refs, no private raw/encoded names, no omitted or
redacted request bodies, one model move and one native engine action. Each case
moves 1,203 PDFs, preserves its one collision and unselected TXT, and independently
checks file/link contents and metadata. Two visibly different pending counts
replace each other with 192 prior messages. Width-60 loopback and width-140 exact
release/installed real-provider proof pass; terminal restoration passes.
Normal unfiltered filename readback passes on candidate and installed artifact.
All 102 production configuration/session metadata entries remain unchanged, with
no contents read by the guard. Existing user processes were not stopped.

Recovery: /Users/aji/.local/bin/.ca-release-stage.beSF1y/ca.recovery-ORC258-20261028T031512Z-df50db64a728723c3719b562629f62bd10743dc9ac42db2855a2c3dde29c1f0d.
Background gate job 66798 and qualification/install job 51044 completed. Logs:
scratch/FLT-307/full-4-gate.log and qualification-install.log. Full identities,
source files, failures and receipts: scratch/FLT-307/final-assessment.json.

Coverage limit: APFS rejects invalid UTF-8 filename creation with EILSEQ. Native
byte serialization roundtrip passes; actual invalid-byte file movement remains
unverified. Existing ignored probes, other host platforms and long fuzz/soak runs
are not claimed. No required implementation or qualified-host installation work
remains.
