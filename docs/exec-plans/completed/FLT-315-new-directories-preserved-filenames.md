# FLT-315 — New directories and filename-preserving website moves

Status: COMPLETE — signed ca 0.1.75 installed; real provider passes without `--filter` (2026-10-29).

## Outcome

Normal interactive installed entry used real native `openai-codex/gpt-6.1-sol`
and saved structural/filename privacy, with legacy Latin as a control. Startup
shows Selected values only before input. The disposable profile mirrors the
verified saved privacy switches; production configuration is checked only by
opaque digest, and ca alone resolves authentication.

The provider created site, pages, styles and scripts under a private deployment
parent. All four destinations were absent in the recorded pre-launch filesystem.
Four native create_directory calls created them; three native selections and
moves transferred 1 HTML, 1 CSS and 2 JS files, retaining all four original
private basenames. No guessed filenames or copy/delete substitutions were used.

All six final files match exact expected bytes; four old source paths are gone.
All six HTML/CSS/JS links/imports resolve correctly, preserving stylesheet query
and fragment, root-relative SVG, correctly rebased PNG/CSS assets and exactly one
relative ./ on the module import. Entry/shared JS, SVG and PNG hashes remain
unchanged; all other changes are necessary HTML/CSS URL spellings. No aliases
remain in local source. All three edited sources were re-read after their final
edit; the provider completed normally and restored the terminal.

## Implementation and proof

- Structural filename mode admits the existing create_directory/select_files/
  move_files tools. Canonical directory references bridge to host-owned folder
  capabilities for batch fields; real-path workspace checks, approvals,
  fingerprints, frozen selections and native journals retain authority. Ordinary
  read/list tools still require canonical filename references. Legacy task-name
  remapping remains disabled in filename mode, preserving one path vocabulary.
- Directory creation uses one public leaf under an issued parent. Newly disclosed
  root-directory spellings, including accepted ./leaf arguments, remain stable;
  absolute/private paths remain aliased and existing protected names stay hidden.
- Newly computed JS module targets have explicit relative syntax, without
  doubling a caller-supplied ./ around a canonical reference. Original source
  spelling records are unchanged.
- Restored web edits are classified on the existing pre-commit blocking syntax
  validation worker, before a native preview is published. Later reads no longer
  retrofit full URLs into an earlier preview. Prospective classification does
  not issue a new edit/read view. Replay guards, session schemas, transport,
  permission and undo/journal behavior are unchanged.
- Final focused checks: 357 all-feature privacy/filter/replay/workspace/approval/
  mutation-receipt tests pass, two existing opt-in probes ignored; formatting,
  strict binary Clippy and source stability pass. Report:
  `scratch/check-change/runs/20261029T081718Z-72564/report.json`. Source review:
  `scratch/FLT-315/independent-source-review-third.json`; hashes:
  `scratch/FLT-315/source-review-third.json`. No full workspace matrices rerun;
  unchanged native-engine gate evidence is reused, with focused coverage stated.

## Actual final provider evidence

`scratch/FLT-315/provider-installed-3/receipt.json`: 150.408 seconds, 46 logical
and physical HTTPS requests, all HTTP200/completed, no transport/privacy errors.
All 45 native calls succeed: 13 reads, 19 listings, four directory creates,
three selections, three moves, one write and two edits. Exact arguments/results
replay, stable native result projections and all post-edit reads are checked.

One 5,578,727-byte trace contains 4,939 contiguous events. Every physical body
matches the final application pre-send trace. Independent audit checks 57 private
old/new directory/file/full-path/data values and 702 raw/encoded variants: zero
matches. Parent scan separately checks 661 variants. No plaintext/stream-byte
omissions or trace drops occur. Exact file/link/directory checks:
`provider-installed-3/restoration-audit.json`. Final independent audit:
`scratch/FLT-315/independent-audit-final.json`.

Twelve encrypted reasoning replay fields in requests 35–46 remain opaque. Ca
forwards provider-returned encrypted state without a decryption step; its audit
checks plaintext supplied to the provider, not the encrypted state. Captures are
at the application boundary, not remote packets. PNG pixels/browser rendering,
other providers and cross-version session resume are not qualified by this
synthetic --no-session workflow.

## Retained failures and installation

Installed 0.1.72 baseline reported unavailable native tools after one request and
made no changes. Failure-first tests reproduce the tool/module-link gaps; review
also found and reproduced exact ./root-directory argument history changes.
Installed 0.1.73 completed the disk moves correctly, then blocked logical 34 after
33 HTTP200/completed sends: a corrected HTML read changed its earlier generic
write preview. Installed 0.1.74 reached Done with all targets correct, but exact
JS contained ././; one pre-response transport failure recovered (44 completed
plus one failed attempt). Its older all-success harness could not accept that
completion and was stopped only after all attempts ended and Task Done; terminal
restoration is not claimed for that stopped harness. Both remain failed receipts
and independent audits. No completed operations were replayed on either fixture.

The corrected harness retains recovered pre-response transport failures and
requires a later normally completed final request. Contract checks against the
actual traces reject privacy failures, a failed latest request and missing
responses. Final run had no recovery events. An initial module-prefix local
regression reused a reserved uncreated path and hit its stale-source guard; the
corrected regression uses an existing read-bound module as in the real failure.
All failed source/helper/runtime evidence remains retained.

Each distinct repaired source was built once: 0.1.73 (178.576s, session 40828),
0.1.74 (175.750s, session 80565), final 0.1.75 (182.413s, session 18220). Candidate,
staged and installed hashes/signatures, mode/owner, version/help and installed
real workflow pass. Final build/install log:
`scratch/FLT-315/release-install-75.log`; receipt:
`scratch/FLT-315/release-75/install-receipt.json`. Final signed SHA-256:
`e64547efd81f1386e1026c1609f3e256655f2230388c979fc1b7fa0e331b2f0c`.
Recovery:
`/Users/aji/.local/bin/.ca-release-stage.brNXjU/ca.recovery-ORC258-20261029T082341Z-2f696bf969df22646d699016d8206ff0b03a1a239ba4827ef964863de7d79667`.
Earlier recoveries remain recorded in the earlier install receipts.

All 112 production ca config/session metadata entries match across the entire
task. Actual configuration bytes retain the verified default digest. Running
user sessions were left alone. Reviewed source and installed binary remain
unchanged through final provider qualification and readback.

Changed product files: src/privacy/{mod,filenames,tasks,site_tests}.rs,
src/agent/tools.rs, root Cargo.toml/Cargo.lock. Documentation:
docs/CONFIGURATION.md, this plan, CONTINUITY.md, NEXT_STEPS.md, TESTING.md and
docs/.INDEX.md. No oracle/reference or production configuration/session edits.
Final index: `scratch/FLT-315/final-assessment.json`. No remaining FLT-315 work.
