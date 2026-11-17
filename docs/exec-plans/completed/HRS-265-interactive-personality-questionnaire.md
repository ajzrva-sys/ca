# HRS-265 — Interactive personality questionnaire

Status: COMPLETE and installed (2026-10-07)

## Goal

The user rejects manual Markdown editing and requests a far more advanced,
user-friendly interactive questionnaire. Deliver guided choices and custom
answers directly inside CA, with quick setup, a complete walkthrough, section
editing, progress, saved-answer resume, and a review before activation.

## Acceptance

- `/personality` and `/questionnaire` open the native CA interface.
- `ca harness questionnaire setup` opens the same interface without a provider;
  default terminal invocation starts interactive setup, while nonterminal/JSON
  inspection remains compatible.
- Forty-eight preference questions cover twelve behavior areas, with explained
  choices, custom text, skip/back, additional instructions, and a generated
  preview. Unanswered questions do not invent human preferences.
- Save publishes steering automatically; cancel performs no write. Existing
  human preferences are preserved, exact revisions prevent lost changes, and
  saved answers reopen correctly. Profiles remain isolated.
- File operations stay outside the event loop. Terminal restoration, narrow
  layouts, actual keyboard paths, and the resulting provider prompt are verified.
- Preserve the existing evidence audit, continuous watch, and turn/retry steering
  boundaries. Complete required validation and recoverable signed installation.

## Approach

Store/catalog, native interface, and actual terminal validation have disjoint
agent ownership. Parent owns command integration and async orchestration.
Generated `personality.md` remains the prompt integration artifact, with hidden
structured answers and human-readable active preferences in one atomic file.
Users answer the interface; editing that artifact is optional.

## Evidence

The installed previous artifact rejects `setup` with exit 2 and restores the
terminal. Failure receipt: `scratch/questionnaire/interactive-first-failed.json`.
Initial repository changes from HRS-264 are retained. The checkout has Git
metadata; the older memory note about missing Git is stale. No oracle is needed
for this CA-specific feature. `.agent/PLANS.md` remains absent.


## Implementation progress

- Typed preferences/catalog and private atomic storage are complete; eight
  focused storage tests pass. Save rejects stale revisions and preserves prior
  instructions and recovery bytes. Existing real profile answers are untouched.
- Native and standalone editor are complete; nine focused UI tests pass,
  including all 48 questions and all three explanation choices at 60×20.
  The first Unicode snapshot failure was retained; only wide-cell snapshot
  reconstruction changed, retaining the exact visible-text assertion.
- Main/command/CLI integration is connected. The standalone setup is provider
  free. The native editor preserves the composer, blocks unrelated input, and
  routes asynchronous file work by pane and editor generation.
- Independent review found background-completion continuations could start a
  turn behind the editor. Added narrow admission guards and an unconditional
  child-result Tick retry. Regression drives a final snapshot while editing,
  closes the editor, and resumes on a Tick with no new snapshot. It passes;
  its failure-first receipt remains retained.
- All seven actual debug PTY cases pass, including the full 48-question flow,
  12-section menu, Unicode editing/review at 60×20, narrow resize, save/reopen,
  profile isolation, concurrent-edit preservation, and native setup followed
  by a captured next provider request. All nine terminals restore termios.
  The first false fixture exit failures are preserved separately.
- Final focused checks pass all 25 tests. Complete default/all-feature
  matrices, current format/strict lint, signed release PTYs and install pass;
  the exact cursor-only source qualification is recorded below.
- User-facing docs now instruct `/personality` or `ca personality`; manual
  Markdown editing is no longer the setup flow.


## Final gate repair

The first complete default workspace run has 3,850 passes, one failure and 43
existing ignored across 27 targets. The sole failure is the expected command
menu total changing from 114 to 115 after adding `/personality`. Independent
comparison finds no other changed fixture body bytes. Updated only the two
wide/narrow totals; the focused palette test passes. The complete failed log,
summary and generated pending snapshot remain retained. No Rust behavior was
changed for this fixture repair. Final matrices below run against the updated snapshot and otherwise
frozen source.


A later complete default run has 3,850 passes and one failure confined to the
preexisting protected-baseline CLI fixture (`File exists`, errno 17). The
unchanged focused target passes all three cases. Its shared PID/wall-clock
naming has a collision path under parallel tests; exact colliding clock values
were not recorded. Added an atomic sequence to fixture names and acquire
cleanup ownership only after directory creation succeeds. No production logic
or dependency changes. The repaired focused target passes at four threads; the subsequent
complete parallel attempt and final sequential matrices are recorded below.
First failure logs remain retained.


A subsequent four-thread main target hits the previously recorded legacy
migration fixture with WouldBlock instead of InvalidInput (3,000 passes, one
failure, 42 ignored). The unchanged focused test passes. Stopped that already
failed parallel matrix, preserving its incomplete log and stop receipt. Reuse
the prior repository procedure: complete sequential default/all-feature suites,
with no exclusions, assertion changes or session-source edits. The 25 new-flow
focused checks remain passing at four threads. Sources stayed frozen during those suites.


## Completion evidence

Complete sequential workspace suites pass without exclusions: default has
3,851 passed, zero failed, 43 existing ignored across 27 targets; all features
has 4,000 passed, zero failed, 44 existing ignored across 28 targets. Both have
zero filtered cases. The nested child writer probe is excluded from aggregate
counts, with complete per-target declarations independently checked.

Those matrices bind 1,079 inputs at digest
`723c4579d35bac718a53ca78734c134867b5e1c4e1ce5f7f433650dcd4891f29`. Their original strict Clippy gate then
fails on `.last()` over a double-ended grapheme iterator. Changed exactly one
call to `.next_back()` in the cursor helper. Reconstructing the former bytes
reproduces the prior file hash; all other inputs and the file set match.
Independent review confirms identical prior-grapheme results. Current 25
all-feature focused checks, format and strict all-target/all-feature workspace
Clippy pass at digest `98b21b1390b7687bb4468cef9a05e877bf6b786d3392f7cd0c77b24286085c19`. Full matrices remain
explicitly bound to the pre-optimization source; no broader rerun is claimed.
`cursor-followup.json`, both manifests and `matrix-audit.json` record this
qualification. The original failed lint receipt and every earlier failure are
retained.

Background session 30772 completes the optimized all-feature workspace release,
ad-hoc signing, seven interactive and four original signed-release terminal
cases, and recoverable installation. All 11 cases pass against SHA-256
`d289ba4251b814e13cc67434c2bc95b2ec2586127e45122883ab834cef2a9df7`. The nine interactive terminal processes and four original processes
exit as expected and restore termios. Keyboard/frame and captured loopback
provider evidence covers the actual save/reopen/native-to-next-turn paths;
paid-model personality effectiveness and broad behavioral improvement remain
unverified.

Installed `/Users/aji/.local/bin/ca` matches the signed candidate. Private
same-filesystem staging, atomic replacement, post-copy signature, owner/mode,
hash, version/help and prior-binary recovery hash pass. Recovery:
`/Users/aji/.local/bin/.ca-release-stage.3Nafr5/ca.recovery-ORC258-20261007T224646Z-077fec4e283c74cffe9971d506052da6aa8ea9f9ec2ae8e8d987d42a91088eb2`. The real personality document's bytes, mode and owner
are unchanged. Existing sessions were not interrupted or launched. Reopen CA
for native `/personality`, or use `ca personality` immediately.

Receipts and logs: `scratch/personality-interactive/release-install.json`,
`release-build.log`, `interactive-release.json`, `tui-release.json`,
`install.log`, `final-validation.json` and the preserved earlier attempts.
No required implementation work remains for HRS-265.
