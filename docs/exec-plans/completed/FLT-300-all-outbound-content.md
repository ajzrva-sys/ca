# FLT-300 — Retrieved-content filtering and continuation repair

Status: complete and installed as signed ca 0.1.43. User-approved scope completed; actual evidence below.

## Goal and accepted decisions

Apply Latin/word substitution to all retrieved data, including every tool result, attachments, recalled/context evidence, and words restored from provider stand-ins. Preserve authored prompts, instruction files/skills/preferences, and tool arguments subject to existing hide/stop rules. Replace images with `[image]` whenever substitution is active. Preserve original local text, files, provider-issued native replay and encrypted reasoning.

Optional canonical `retrieved_spans` distinguishes unknown legacy origin (`None`) from known authored text (empty) and sourced UTF-8 byte ranges. Carry it through normal save/resume and request projections without provider-wire metadata. Unknown old attachment sections require explicit reattachment; exact fresh attachment references supersede matching old sections in the outgoing copy only. No bulk session migration or production configuration changes.

Responses replay accepts exact raw equality, then existing raw-argument normalization and semantic JSON equality, with at most one restoration of raw values/text. Call identity, order, counts, route and size validation remain strict.

## Baseline and evidence

Installed version at start is signed ca 0.1.42. Baseline snapshots are in `scratch/FLT-300/before/` (unfinished earlier attempt) and `implementation-before/` (start of approved implementation). Workspace has no `.git` or `.agent/PLANS.md`; preserve bounded before-images and manifests.

Copied installed binary reproduces the exact failure in normal CLI and TUI: a successful restored-path local read is followed by a pre-send canonical/raw Responses tool-call mismatch. Evidence: `native-baseline-v2/cli/` and `native-baseline-v3-tui/tui/`. Separately, original-argument runs complete two requests but transmit synthetic file and image canaries unchanged: `native-baseline-filter-only-v1/receipt.json`. Fixture setup failures remain retained and are not application passes. Original configuration/source/binary identities and terminal modes are preserved.

## Work and validation

1. Replace unfinished whole-request encoding with result-only traversal and explicit sourced spans; preserve protocol/argument/schema structure and source-level stop checks.
2. Preserve restored assistant origins, typed images, source annotations through assembly/context/history and native session persistence. Include direct evidence-bearing provider requests so retrieved data cannot escape through host wrappers.
3. Repair Responses canonical replay equivalence and remove unfinished blanket native-replay rejection.
4. Freeze combined source, obtain independent review, run focused checks then unchanged `scripts/verify.sh` with compatible BUG-299 baseline and inspect raw reports/evaluation/logs. Preserve all failed attempts; no acceptance exclusions.
5. On passing qualification bump root patch once, background-build workspace/all-feature release, recoverably sign/atomically install, and prove installed CLI/TUI behavior against synthetic loopback data. Record candidate/staged/installed identity, version/help/signature/mode/owner, recovery and state preservation.

## Progress

- Original filter and stream failures reproduced independently through real CLI and terminal entry points using disposable synthetic fixtures; no real providers or production files.
- Retrieved-source annotations and targeted filter/Responses fixes implemented; all-feature test compilation passed before final integration follow-ups. Focused HTTP tests cover Chat, Responses, Anthropic and Google, plus actual headless/child tool loops.
- Final combined independent review, focused/full v8 qualification and recoverable signed ca 0.1.43 installation are complete; actual receipts are recorded below.

## Completion evidence

Complete: signed ca 0.1.43 is installed recoverably at `/Users/aji/.local/bin/ca`. All five implementation/qualification/install steps above are complete. The final assessment is `scratch/FLT-300/final-assessment.json`.

- Full v8 gate/evaluator pass; default 4363 passed / 0 failed / 43 ignored; 29 targets; 0 filtered; all features 4519 passed / 0 failed / 44 ignored; 30 targets; 0 filtered; nine native terminal cases pass.
- Focused 307/0 and independent combined reviews pass. Candidate and installed normal CLI/TUI fixtures each complete the two-request restored-path tool loop: original retrieved file/image canaries are absent from outgoing data, [image] is present, and fixture source/configuration and terminal state are preserved.
- Reviewed root `4252209e8578a5d38410d309d7f3b13c142d88939f092cb7f701e091339ecd00`; tested gate `969994236cdb21bd35b96fdd8d81aeb122d6f18717745473b9ffb960d53a8791`; installed release `de1e2c8542d6c5f2b58f4b095e34ec764be3031a599d6eba362c8bbcf842a1fd`. Root patch bump occurred once; only Cargo.toml/Cargo.lock differ from tested inputs.
- Release job session_id=91240; build log `scratch/FLT-300/release/build.log`. Installed SHA-256 `55ee2ec7da78d0c887818b8f6410c1a9b7241379ba7807c01f273dfe9e450346`, mode `0o755`, uid/gid `501/20`. Candidate/staged/installed signature and version/help readback pass. Recovery copy: `/Users/aji/.local/bin/ca.recovery-FLT-300-20261023T221144Z-3dc6951ac550535a5d47a9562155b2de0621768ded4d9a94b0cf4025a9509ce1` (prior SHA-256 `3dc6951ac550535a5d47a9562155b2de0621768ded4d9a94b0cf4025a9509ce1`).
- Gate/evaluation: `scratch/FLT-300/gate-v8/report.json` and `scratch/FLT-300/gate-v8/evaluation.json`. Candidate/installed CLI/TUI: `scratch/FLT-300/release/candidate-runtime/receipt.json` and `scratch/FLT-300/release/installed-runtime/receipt.json`. Install readback: `scratch/FLT-300/release/installed-readback.json`.

Retained failures: v7 stopped at strict lint; initial v8 focused execution hit the unchanged three-second checkpoint-ownership deadline, then the same compiled suite passed all 307 tests without source or deadline changes. The failed preliminary default-debug provenance assertion and accurately scoped all-feature debug acceptance remain preserved; release candidate and installed proofs are authoritative.

At implementation/install closeout, external-provider and remote-platform behavior had not been exercised. The later authorized real-provider follow-up below qualifies one native Responses route. No new filter policy, production configuration or session migration is introduced.

## Installed real-provider follow-up (2026-10-23)

The unchanged signed installed ca 0.1.43 completed a synthetic-only TUI task against `openai-codex/gpt-6.1-sol`. Nine main-agent and six subagent requests all returned HTTP 200/completed. Both paths read their own text file, called `view_image`, read HTML carrying original PNG data URIs and continued; the parent also reread its file after the child completed. All five image calls succeeded. The parent and child emitted distinct final markers in real provider events and the terminal.

Every outgoing attempt body was checked for raw fixture canaries, numeric/path/Unicode/URL values and original image encodings. Latin substitutions, readable authored controls and literal `[image]` placeholders provide positive evidence; typed image structures and source-field trace omissions/media redactions are absent. A separate installed filter-off loopback control shows that the same tracer preserves the text canaries/original HTML image encodings and typed-image structures, so real-run absence is not inferred solely from the trace's media sanitizer. Detailed scope, identities and independent review are in `scratch/FLT-300/live-provider-v1/assessment.json`.

Retained caveats: overlapping image filename hide pairs add a prefix again in outgoing native replay, but every image call and continuation succeeds; exact native argument spelling preservation is not claimed for those pairs. The scratch driver checked nonexistent parent request kind `main` instead of `main_turn`/`tool_continuation`; its original false completion flag remains unchanged. Independent genuine trace events and terminal output prove completion before stopping only that driver; ca exits zero and restores terminal state. See `completion-observer-correction.json` in the same evidence directory.

The follow-up uses a disposable HOME/workspace and native authentication; no driver reads/copies credential values. Installed hash/signature/version, synthetic fixtures/configuration and production config metadata remain unchanged, with no production session or running user process modified. Other real-provider adapters and remote platforms remain unverified. This is a test/documentation follow-up: no product source change, release bump, rebuild or replacement.

The later [native overlapping-alias replay repair](FLT-300-overlapping-alias-replay.md) supersedes that native replay caveat: signed 0.1.44 is installed, the unchanged full gate/evaluator passes, and an installed real-provider main/child run preserves all 34 native call occurrences exactly with 192 raw-canary checks finding zero leaks. Ordinary summary hide projection remains separately bounded in that follow-up; the initial 0.1.43 observations and receipts above are retained.
