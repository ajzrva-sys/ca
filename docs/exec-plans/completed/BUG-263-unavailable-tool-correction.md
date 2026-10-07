# BUG-263 — Actionable unavailable-tool correction

Status: COMPLETE — installed and verified
Owner: Codex, 2026-10-06

## Goal and evidence

The user reports `Provider attempted unavailable tool: run_command. Adjust the
tool selection and use /retry` and requests a fix. Current Main and Lazy core
catalogs include run_command. Ask/Plan/custom/explicit tool restrictions can
exclude it. The exact footer occurs after a repeated mismatch (or the first in
a workflow). The screenshot lacks mode and preceding request; user context is
requested asynchronously, without assuming the cause.

The existing one-correction response gives no actual available names or
discovery guidance. The footer blames tool selection even when the model simply
ignored the exact advertised catalog. Correct those concrete feedback defects.

## Scope and invariants

Bounded exact-catalog correction and actionable mode-specific terminal feedback
in main.rs, focused regressions and disposable terminal fixtures. Keep one
correction maximum, exact advertised-set authority, tool-free Ask behavior,
workflow boundaries, frozen /retry schemas, approvals and execution exclusions.
No mode/config/session changes in the user's running application. Normal Agent
requests must still advertise and execute run_command through normal authority.

## Milestones

- [x] Characterize normal Agent, restricted-mode correction and repeated failure
  with the installed signed executable in a disposable actual terminal.
- [x] Supply safe sorted bounded exact available names and conditional discovery
  guidance; keep legacy correction markers recognized without widening authority.
- [x] Verify a compliant provider continues after correction, repeat fails bounded,
  no unavailable command executes, frozen retry stays restricted and a new Agent
  request refreshes its catalog.
- [x] Run combined BUG-262/BUG-263 completion gates and signed terminal checks,
  install recoverably under standing authorization, and record final evidence.

## Validation and recovery

Evidence and before-images: scratch/unavailable-tool/. Its main.rs before-image
includes the preceding BUG-262 compaction repair; scratch/compaction/before/
retains the original source. The initial combined compaction default run was
interrupted deliberately to integrate this second authorized fix; it is not a
passed full workspace gate. Final validation runs both fixes together.

## Implementation and evidence

The installed signed release naturally reproduces the exact screenshot after
two Ask-mode run_command calls with empty schemas; the command never executes.
A provider conditioned on receiving an explicit empty-catalog correction also
reproduces that failure (`old-catalog-pty.json`). Installed normal Agent mode
already advertises and executes a harmless command, and frozen Ask /retry stays
restricted. This establishes the feedback defect without claiming that the
cropped user screenshot proves its actual mode or model route.

Three catalog-feedback tests fail before the repair. Six new feedback/bounds/
marker/guidance regressions plus existing correction/execution tests and an
actual deterministic 100/60-column render test pass (11 focused cases). Generated
and legacy markers require Error status and remain scoped to the current user
turn. Sorted complete JSON names occupy at most 4096 bytes; omitted names refer
to the actual schemas. Ask/Plan advice leads the footer, and Plan explicitly
says Tab until Agent. No tool set, mode setting or execution approval is widened.

All four signed all-feature release terminal scenarios pass (session 22260,
exit zero, terminal state restored), bound to candidate SHA
`f20a97ab23bdec8e01c5f656bacd872cec5488181e73afc78c4d545831971fa0`.
The exact empty-catalog correction lets a compliant provider complete; repeated
misuse is bounded without executing an unavailable command. Frozen Ask retry
retains empty tools; new Agent submission advertises and executes run_command.
Actual 60-column Plan guidance and the Plan→Ask→Debug→Agent three-Tab transition
pass. Receipt: `scratch/unavailable-tool/release-pty.json`. Its exact signed
candidate bytes match the independently verified installed executable.

## Final validation and installation

Full default/all-feature workspace tests pass: 3,809/3,958 tests across 26/27
targets, zero failures or filtered cases, 43/44 existing ignored tests. The first
all-feature attempt's unchanged oversized legacy-migration fixture failure is
preserved separately. Focused and complete one-thread reruns pass with unchanged
source; the 25-ms cold identity-scan deadline cause remains inferred.
No package, target or test exclusions are used. Formatting, strict all-target/
all-feature workspace Clippy and all-feature release build pass. Final gate
session 26568 exits zero; detailed commands/log hashes are in
`scratch/compaction/validation-gates.json` and complete workspace summaries.

All six signed release terminal cases for both fixes pass, with exit zero and
restored terminal state (compaction session 53296, unavailable-tool session
22260). Fake loopback providers, disposable homes and exact catalogs verify the
real local interaction paths; no production provider/config/session mutation
or inference of the cropped screenshot's actual mode is needed.

The complete 1,073-input inventory SHA is
`75c9905f9ab0f2b3942f8ea2342bf6f51c6b4df4327bf9f6f163474b9167fe69`.
An explicit completion receipt preserves the initial six-file inventory omission
and verifies those existing files against the prior validated bytes; source and
candidate are unchanged. All actual build roots and protected source files are
covered. Authorized background build/install session 1017 exits zero and
installs the candidate above at `/Users/aji/.local/bin/ca` with a verified
SHA-labelled rollback copy. Independent PATH/hash/signature/owner/0755 mode/
version/help/rollback and matching terminal receipts pass in
`scratch/compaction/installed-readback.json`. Running sessions/settings remain
untouched. Restart ca to load both fixes.

`PARITY.md`'s central restrictive tool resolver remains authoritative, including
custom/Ask/Plan restrictions. Better model feedback and footer guidance grant
no extra tools or approval authority and claim no oracle runtime parity.
