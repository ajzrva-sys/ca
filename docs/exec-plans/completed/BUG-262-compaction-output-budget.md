# BUG-262 — Compaction output budget and bounded recovery

Status: COMPLETE — installed and verified
Owner: Codex, 2026-10-06

## Goal and evidence

The user reports `Compaction failed: provider response was incomplete` and asks
for a fix. The screenshot is failure evidence. Its generic error cannot identify
the exact provider cause. Source inspection proves that checkpoint generation
caps total output at 4096 tokens, the same as the validated visible summary,
without reserving room for reasoning. The initial prompt also omits its limits.

## Scope and invariants

Separate the provider generation budget from the 4096-token/32-KiB summary limit.
Recognize only explicit output-limit termination for one fresh retry from the
same source. Keep strict complete-response validation, tool-free generation,
one quality repair, session ownership, cancellation generations and prior
checkpoint preservation. Account for each physical request separately. No
runtime config/session, live provider, oracle or reference mutations.

## Milestones

- [x] Capture failure-first prompt/decoder regressions and installed-terminal failure.
- [x] Add 16384-token initial generation budget and one 32768-token retry shared
  across quality repair; preserve exact source prompt and reject partial output.
- [x] Safely distinguish Responses max_output_tokens from filter/unknown causes.
- [x] Run focused/default/all-feature workspace, formatting and strict lint gates;
  signed release terminal cases cover chat/Responses, exhaustion and cancellation.
- [x] Run authorized background release/install, preserve recovery artifact,
  verify installed bytes/signature/version/help and record durable evidence.

## Validation and recovery

Before-images, logs, patches and terminal receipts live in `scratch/compaction/`.
This checkout has no Git metadata and `.agent/PLANS.md` is absent, as in the
previous delivery. No Git history or provider parity claim will be fabricated.
Installation follows the repository's standing post-task release authorization.

## Implementation and evidence

The installed signed release reproduces the exact screenshot in both Chat
Completions and Responses with one 4096-token request and no retry. Both terminal
sessions preserve the existing checkpoint/history and exit zero with restored
termios (`old-installed-pty.json`). Prompt and exact-cause decoder regressions
fail before the repair. Four new budget/prompt regressions, all 42 context tests
and seven focused Responses decoder tests pass during implementation.

`generate_provider_checkpoint_with` shares its one token-limit retry across the
existing single quality repair, clones the current source/repair prompt exactly,
and rejects partial output. The checkpoint client uses one physical transport
attempt so the maximum three generator calls also bound actual POSTs and usage
spans. Finish-event usage is observed before chat Length assembly fails. Existing
Responses incomplete metadata accounting stays unchanged; unknown usage is not
invented. Summary validation, canonical history, writer ownership and cancellation
remain intact. Final combined BUG-262/BUG-263 gates pass.

The signed all-feature release candidate SHA is
`f20a97ab23bdec8e01c5f656bacd872cec5488181e73afc78c4d545831971fa0`.
Both complete real-terminal compaction protocol cases pass (session 53296,
exit zero, terminal state restored): bounded retry and quality-repair budgets,
checkpoint/history preservation on exhaustion/filter/refusal/unknown/EOF/HTTP,
one physical POST for HTTP 503, usage spans and Escape cancellation. Receipt:
`scratch/compaction/release-pty.json`. The signed candidate's bytes also match
the verified installed executable.

## Final validation and installation

The full default workspace passes 3,809 tests across 26 targets (zero failures,
43 existing ignored, zero filtered). The full all-feature workspace passes 3,958
across 27 targets (zero failures, 44 existing ignored, zero filtered). The first
four-thread all-feature run failed one unchanged oversized legacy-migration
fixture with WouldBlock rather than InvalidInput. Its cold 25-ms identity-scan
deadline is the inferred cause; the focused rerun and complete one-thread rerun
pass without source changes or exclusions. Preserve the failed log/summary and
focused rerun under `scratch/compaction/`; no failed case is counted as a pass.

Formatting, strict workspace/all-target/all-feature Clippy and all-feature
workspace release build pass (final validation session 26568 exit zero;
parallel lint/release session 73704 exit zero). Complete source/build/test input
inventory: 1,073 files, SHA
`75c9905f9ab0f2b3942f8ea2342bf6f51c6b4df4327bf9f6f163474b9167fe69`.
Six existing files omitted from the initial inventory were added after each
matched the prior validated bytes. The original incomplete inventory and
candidate metadata remain preserved; `input-inventory-completion.json` records
the correction. Source and binary are unchanged, and the complete inventory
covers every actual build root and protected source file.

Authorized background build/install session 1017 exits zero. The installer
preserves the prior SHA-labelled executable, stages on the destination
filesystem, atomically replaces `/Users/aji/.local/bin/ca`, and signs it there.
Independent PATH/hash/signature/owner/0755 mode/version/help/rollback and terminal
receipt readback passes in `scratch/compaction/installed-readback.json`.
Installed SHA is the signed candidate SHA above; prior SHA is
`05d2f106c62c65a27a922de30ae45ee3af44a78bd43a4cbdcd40cbfece6c1751`.
The recovery path is recorded in that receipt and `install.log`. Running user
sessions and settings were left untouched. Restart ca to load the fixes.

Fake loopback providers and disposable sessions establish the actual local
terminal paths. No production provider request or claim about the screenshot's
exact provider cause is made. Durable gate commands, log hashes, complete
workspace summaries and before-image review patches accompany these receipts.

`PARITY.md`'s non-destructive model-generated checkpoint contract and central
restrictive tool resolver remain satisfied. These ca-owned repairs claim no
new oracle runtime parity; oracle/reference trees were not modified.
