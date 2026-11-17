# BUG-266 — Quick personality setup progress

Status: COMPLETE and installed (2026-10-07)

The user reports being forced through 48 questions after selecting Quick setup.
Reproduce in the installed executable and repair the actual path, preserving
saved choices and keeping all file writes behind explicit review/save.

## Current evidence

Installed SHA-256 `d289ba4251b814e13cc67434c2bc95b2ec2586127e45122883ab834cef2a9df7`.
Six fresh actual terminal cases (standalone and native, three selection methods)
show questions 1 through 8 and then Review. No quick-to-full transition is
reproduced. However, Review displays `8/48 answered` and the generic
`Walkthrough complete`, obscuring that the other questions are optional.
The question footer also omits the existing Ctrl-S review shortcut.
Both installed receipts are retained under `scratch/personality-quick-repro/`.
The precise route in the reported session remains unconfirmed; requested the
opening command and displayed walkthrough label without blocking the repair.

## Acceptance and scope

- Quick setup contains exactly eight questions and transitions to review after
  question eight, with its own answered count and explicit completion message.
- The other 40 questions are visibly optional. Full walkthrough and individual
  sections retain their own counts and selection behavior.
- Review/save is visible during questions. Two answers can be saved and later
  resumed at question three; no complete-questionnaire requirement is added.
- Default/all-feature focused tests, applicable package tests, format, strict
  lint, actual release terminal checks and signed recoverable install pass.
- Preserve the user's existing profile answers and running sessions. Use
  disposable terminal profiles for every interaction test.

## Plan

Add failure-first render/state and actual terminal regressions. Keep the review
walkthrough scope for its header, replace ambiguous global progress, and show
the existing save-at-any-point shortcut. Verify stopping and partial resumption,
then build and install according to the standing release rule.


## Completion

Added retained review scope and walkthrough-specific answered counts. Quick
setup shows eight questions in both the question and review views, then says
the other 40 are optional. Global menus show an answered preference count
without implying a 48-question requirement. Question footers expose Ctrl-S
review and saving at any point. Storage and prompt assembly are unchanged.

Two new render/state tests fail first, then all 27 default and all-feature
personality checks pass. The complete default CA package passes 3,071 tests,
zero failed, 42 existing ignored, zero filtered across six targets. Declared
per-target counts match; nested child writer-probe results are excluded.
Formatting and strict all-target/all-feature workspace Clippy pass. Only
`src/tui/personality.rs` changes among 1,079 compiled inputs relative to the
prior installed source. Current gates bind digest `4d45189488dd86912243abbdcc3d968504b19dec9183a6ab36895e4f56fad761`.
Prior full dependency workspace matrices remain separately recorded; they
were not rerun or relabeled current. Debug linkers emit a
compact-unwind size warning in retained logs; tests complete successfully and
strict lint passes.

The new actual debug case passes five disposable terminal paths: eight quick
answers, reopening all eight, saving only two, reopening at question three,
and native `/personality` resuming those two and completing only six more.
All preserve exact answers, restore the terminal, and send no model request
while editing. The installed old artifact fails the new counter assertion;
that receipt remains retained separately.

Background release/install session 9044 exits 0 after all-feature workspace
release build, signing, all eight interactive and four original terminal cases,
and recoverable installation. All 14 interactive and four original
terminal processes exit as expected and restore termios. Candidate and installed
SHA-256: `53dcc22ab781b93c251b56e487bc80d56d5459b26df40ea03be34255ffdf8ac0`. Post-copy signature, mode/owner,
hash, version/help and prior-binary recovery hash pass. Recovery:
`/Users/aji/.local/bin/.ca-release-stage.L6IPlB/ca.recovery-ORC258-20261007T231232Z-d289ba4251b814e13cc67434c2bc95b2ec2586127e45122883ab834cef2a9df7`. The real personality file's bytes, mode and owner
remain unchanged; running user sessions are not interrupted or launched.
Reopen CA or run `ca personality` to use the updated display.

Receipts: `scratch/personality-quick-fix/validation.json`,
`default-package-summary.json`, `quick-debug.json`, `interactive-release.json`,
`tui-release.json`, `release-install.json` and associated logs. The exact route
behind the original 48-question report remains unconfirmed; current bounded
native/standalone Quick stopping and partial-save paths are verified. No
paid-model efficacy or unrelated broad behavior improvement is claimed.
No required implementation work remains for BUG-266.
