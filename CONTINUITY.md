# Continuity

## 2026-10-23 current FreeBSD delivery

Final native verification input inventory: `c4ca6055a1f8967fef89a0c676fb19dbd5172143d89bf16fb6ead840f492d155`
(1,169 files). Combined macOS full-gate inventory: `69d36efaca22361e14d0281afe7b4f47e483a79dd93c51fc11917e289c01a3d1`
(1,168 files). Differences are the existing public notice generator and the
FreeBSD-only MCP test-shell fixture. The release runtime prefix is byte-identical;
each artifact retains its distinct actual source identity.

Responses metadata, literal `$HOME` detection and DeepSeek schema-URI filtering
are repaired. The filter repair has 62 focused passes and one minimal live
DeepSeek success. Final native MCP/filter tests (22/62) and formatting pass. Native v8 passes all eight stages and evaluator: default 4,297 / 0 / 43 over
29 targets and all features 4,451 / 0 / 44 over 30, zero filtered. The copied/stripped native installation candidate passes all three original
workflows. Native ca `0.1.41` is installed once with verified recovery and
ordinary-user PATH/version/help readbacks, SHA-256
`60563263dde655bcb16fa949189c9c09eda425b43dc324a56a19cce3de0aa074`.
The source snapshot is identified by the input digest above; its associated
commit is identified by GitHub history.
The preceding native v7 failed two MCP environment fixtures with `sysctl`
`ENOMEM` before cleanup assertions; that failure was not reproduced in bounded
diagnostics. Choosing native `/bin/sh` for POSIX fixtures is a portability repair,
not a proven causal fix for Bash/kernel behavior. Runtime/assertions are unchanged.

Codex is installed from its immutable native family (CLI SHA-256
`2e56101de2c8926f1f0bc46d65b7fb53a6f7ee0d632975f553ea0f5c0b466714`)
and pushed to GitHub `main` at `d8df4524bac5123ba470d787b35aacca538efcb2`.
Installed direct and Sol-default jail workflows pass. A live supported-model
request also ran the real sandboxed printf command and returned `FREEBSD_OK`;
the earlier unsupported-model HTTP 400 remains separate evidence. Ten native workflows,
47 jail checks and 174 code-mode units pass; its broader scoped core still has
17 failures (1,263 passed, 11 ignored), not a clean complete suite.

The combined macOS gate passes all eight stages/evaluator: default 4,328 passed,
0 failed, 43 ignored across 29 targets; all features 4,484 / 0 / 44 across 30;
zero filtered. All nine native terminal cases pass. Signed ca `0.1.41` was
installed exactly once (SHA-256
`71fb5770bec9101ffbae59d4a7e941f797723f38d5a03ecfdd0d0c6a1df2fadc`),
with verified recovery, signature, identity, mode/ownership and version/help.
Its normal PATH entry point passes the live DeepSeek no-session prompt with the
filter enabled. The preceding candidate's initial 15-second/no-request terminal
failure remains historical uncertainty; the final gate passes, but that earlier
failure's cause remains unestablished. The coding
benchmark was canceled before any scored native attempts. See
[current publication evidence](docs/PUBLICATION_VALIDATION.md) and
[FreeBSD prerequisites/limits](docs/FREEBSD.md). Everything below is dated history.

Installed native ca also passes the real required-jail provider/tool loop:
run_command executes printf successfully and final output is `FREEBSD_OK`.
The earlier symlinked-auth setup `TooManyLinks` failure remains separate evidence;
the successful isolated-state check used normal existing authentication.

## 2026-10-17 dual MIT/Apache-2.0 licensing

The owner selected MIT OR Apache-2.0 for CA-owned material. The six workspace
packages and thirteen fixture/verifier packages declare those terms; both
canonical license texts are included in every workspace package. Existing MIT
permissions and separate third-party terms are preserved. The October 6
publication evidence remains a historical record.

The dependency notice inventory covers 650 checksum-verified locked registry
archives, with original legal texts, upstream supplements, explicit MPL source
locations, and reproducible offline checking. The notice drift check rejects an
intentional altered text in a disposable source copy. Package and document-link
checks pass. Rust source, Cargo.lock, version and installed executable are
unchanged; no new Rust runtime or binary-linkage result is claimed.

Unresolved: pre-import ownership, upstream Apple SDK binding concerns, syntect
serialized-asset provenance, and 25 packages without verified complete original
primary license texts. Canonical terms supplement declared licenses without
inventing copyright holders. See [licensing scope](docs/LICENSING.md),
[third-party notices](THIRD_PARTY_NOTICES.md), and the
[change record](docs/exec-plans/completed/LIC-2026-10-17-dual-licensing.md).

## 2026-10-06 initial public source publication

Built the complete all-feature release and prepared the public `ajzrva-sys/ca`
initial source import on `main`, with portable README/setup/configuration/usage/
automation/development/troubleshooting guides, 22 command-family help pages,
and five actual PTY TUI screenshots. Complete all-feature workspace rerun:
3,958 passed, zero failed, 44 ignored, no filtered cases or baseline exclusions.
Formatting, strict Clippy, release build, executable signature/version/help,
source inventory and new-guide links pass. The built executable is identical
to the installed `ca`; no replacement was needed.

The first restricted-runner failure is retained separately. No live-provider,
new-platform, eight-hour-soak or real-model-retrieval result is implied.
See `docs/PUBLICATION_VALIDATION.md`, `docs/SCREENSHOTS.md`, and
`docs/exec-plans/completed/PUB-2026-10-06-initial-github-publication.md`.
Local logs: `scratch/publication-20261006/` (excluded from Git).


## BUG-262/BUG-263 compaction and unavailable-tool correction (2026-10-06, installed)

Checkpoint generation now reserves 16,384 output tokens independently of the
unchanged 4,096-token/32-KiB visible summary limit. Explicit Chat Length or
Responses max_output_tokens termination permits one fresh 32,768-token retry,
shared across the existing single quality repair. One-attempt transport bounds
generation to at most three physical POSTs, each recorded separately. Partial
output is rejected; canonical history, prior checkpoint, writer ownership and
cancellation remain preserved. Unknown Responses incomplete usage is not invented.

Unavailable-tool correction now supplies sorted, bounded JSON names from the
exact advertised request catalog, with discovery guidance only when search_tools
is advertised. One correction remains the limit. Ask/Plan recovery actions lead
the footer; frozen /retry retains its original restrictions. Normal Agent
execution and approval authority remain intact. The cropped user screenshots do
not establish the actual mode or provider cause; both reported failure messages
were reproduced against the prior installed signed artifact in disposable TUIs.

Full default workspace tests pass: 26 targets, 3,809 passed, zero failed, 43
existing ignored, zero filtered. Full all-feature workspace tests pass: 27
targets, 3,958 passed, zero failed, 44 existing ignored, zero filtered. The final
all-feature rerun used one test thread and completed in session 26568 exit 0.
Formatting, strict all-target/all-feature workspace Clippy and the all-feature
release build pass; parallel release build session 73704 also completed exit 0.
The first four-thread all-feature attempt had one unchanged legacy-migration
fixture failure; its 25-ms identity-scan deadline is an inferred cause. The
fixture and complete single-thread rerun pass without a session-source change,
exclusions or rewriting the retained failure evidence.

All six signed release terminal cases pass in sessions 53296/22260, including
both compaction protocols, shared retry/quality limits, failed publication and
cancellation, exact-catalog correction, forbidden-command exclusion, frozen
retry and actual mode transitions. Every terminal exits 0 and restores termios.
Complete inventory: 1,073 inputs, SHA-256
`75c9905f9ab0f2b3942f8ea2342bf6f51c6b4df4327bf9f6f163474b9167fe69`.
The initial inventory omitted six unchanged existing inputs; the original
manifest and candidate metadata are retained, with independent prior-byte proof
in `scratch/compaction/input-inventory-completion.json`. Source and executable
bytes did not change when the inventory was completed.

Installed `/Users/aji/.local/bin/ca` SHA-256
`f20a97ab23bdec8e01c5f656bacd872cec5488181e73afc78c4d545831971fa0`.
Background build/install session 1017 completed exit 0. Candidate/staged/installed
hashes, post-copy signature, PATH/owner/mode/version/help and the timestamped
SHA-labelled rollback copy are independently verified in
`scratch/compaction/installed-readback.json`. No running user session or runtime
setting changed; restart ca to load these fixes. This installed artifact
supersedes BUG-261's installed hash; its historical evidence remains below.

Completed plans: `docs/exec-plans/completed/BUG-262-compaction-output-budget.md`
and `docs/exec-plans/completed/BUG-263-unavailable-tool-correction.md`.
Gate summaries, source patches, before-images and terminal/install receipts:
`scratch/compaction/` and `scratch/unavailable-tool/`.

## BUG-261 recovery save ownership (2026-10-06, installed)

Recovery tried to reacquire a writer already held by the active session, naturally
reproducing the screenshot footer against the previous signed release. Failed-turn
completion now saves through its existing owner and updates the committed source
binding before a branch-only worker runs. Cancellation/branch failure cannot stale
the original binding. Local save causes are sanitized, bounded and kept in `/errors`.

The last original-process writer owner now explicitly unlocks its stable file;
a descriptor copied to a pre-exec child no longer delays release. A PID guard
preserves the live parent's authority when a forked child drops copied state.
Writer clone and cross-process exclusion remain intact. Focused failure-first
and final tests verify ownership, source preservation, no-send save failures,
cancellation, frozen requests and temporary-route restoration.

Full default workspace tests pass: 3,796 passed, zero failed, 43 existing ignored,
zero filtered. Full all-feature workspace tests pass: 3,945 passed, zero failed,
44 existing ignored, zero filtered (session 90557 exit 0). Formatting, strict all-target/
all-feature workspace Clippy and the all-feature release build pass.
All eight signed release terminal cases pass: wide/narrow automatic recovery,
a genuine permission denial with useful `/errors` and no replacement, plus all
five Enter cases. Automatic recovery takes 1.119–1.165s; Enter interruption
63–70ms. Frozen HTTP bytes match, original partial output remains durable, new
branch completes, and all terminals exit 0/restored.

Installed `/Users/aji/.local/bin/ca` SHA `05d2f106c62c65a27a922de30ae45ee3af44a78bd43a4cbdcd40cbfece6c1751`.
Build session 55090 and install session 28574 completed successfully. Independent
PATH/hash/signature/owner/mode/version/help, matching terminal receipt and prior
SHA-labelled backup verification: `scratch/recovery-branch/installed-readback.json`.
No running user session or runtime setting was changed. New processes use both
fixes. Plan: `docs/exec-plans/completed/BUG-261-recovery-branch-save-ownership.md`.
Review patches, before-images and all passing/failing logs are in
`scratch/recovery-branch/`. This delivery supersedes BUG-260's installed hash and
unresolved retention gate; its original failing evidence remains retained.

## BUG-260 leading-path Enter interruption (2026-10-06, installed)

The exact screenshot draft `/Volumes/projects/qui you can use this source`
failed against the previous installed release while `run_command` executed.
Leading slash text bypassed busy interruption even when no command matched;
an empty path-completion menu also returned before composer handling.

Input routing now distinguishes actual built-in/model/custom commands from
ordinary text, and unmatched completion Enter reaches existing interruption.
All 127 input tests, deterministic busy-control rendering, formatting, strict
workspace/all-target/all-feature Clippy and the all-feature release build pass.
Five signed release terminal scenarios verify exact submission, foreground
cancellation, queue preservation, empty-menu handling and terminal restoration.

The release is installed at `/Users/aji/.local/bin/ca`, SHA
`e8a58abcabed6cc9f5018031cfdafb6fcb9e634200e55a3c4167e9089ef75d65`.
Independent hash/signature/PATH/mode/owner/version/help and recovery readback is
in `scratch/enter-interrupt/installed-readback.json`; source/log/terminal
receipts and before-images are in that directory. New processes use the fix;
Ctrl+S was verified as the old-process workaround. Settings and user sessions
were untouched. Completed plan:
`docs/exec-plans/completed/BUG-260-leading-path-enter-interruption.md`.

Validation limit: the full default workspace run fails four unrelated main
fixtures (all other targets pass). All four pass focused reruns; an unfiltered
main rerun passes 2,944 with one recurring retention writer-lease failure,
42 ignored. Those modules match the prior AUT-259 validated manifest; the
recurring failure is unresolved and the full suite is not claimed green.

## AUT-259 automation absorption and continuation repairs (2026-10-06, complete)

ca now owns durable occurrence admission, run/action journals, resource claims,
worker cleanup, reconciliation and delivery. `ca-automation-protocol` and
`ca-automation` contain native contracts/services; legacy clients forward or
reexport compatible V1 behavior. Native typed targets work without external MCP.
Repository tasks require exact-tree host checks/review before guarded local
fast-forward; stale targets retain candidates. Harness evaluation ends in Needs
review, including resumed candidates; promotion requires exact authorization.
Both reported continuation problems are repaired with frozen bounded recovery
and parent lifecycle ownership across automatic/manual retry and new native turns.

Full unfiltered default/all-feature workspace tests pass: 3,783/3,932 cases,
zero failures, 43/44 existing ignored, zero filtered. Both strict lint profiles,
formatting and all-feature release build pass at compiled-input SHA-256
`130cc08ec3d36ff2c8dd4874a5368dc5f5d28298419267693dc87a7febc8fdfa`.
Actual signed terminal workflow/required-verification and continuation checks
pass with about 20 ms typing p95, zero typed provider requests and restored
terminal state. Real compatibility observer-crash reconciliation repeats no effect.

Signed ca and the matching compatibility CLI are installed at their existing
`/Users/aji/.local/bin/` paths. Candidate/installed hashes, PATH, signatures,
mode/owner, version/help and timestamped SHA-labelled rollback copies are verified.
Recoverable paired receipt:
`/Users/aji/.local/bin/.ca-automation-stage-7ck13j2t/paired-install.json`.
Private source/gate/terminal/install evidence is in
`scratch/automation-absorption/final-validation/`. Guide:
`docs/CA-automation-runtime.md`; completed plan:
`docs/exec-plans/completed/AUT-259-ca-automation-absorption.md`.

Production settings, paused automations, vault/session records and running
sessions remain unchanged. Config migration is explicit preview/copy/validate.
This non-Git checkout and the unavailable old delegate binding are surfaced by
doctor; repository tasks require a reviewed actual Git binding. The source
archive/manifest/patch preserve this delivery without fabricated Git history.

Sections below retain dated prior evidence. Their older workspace/install
statements are superseded by AUT-259; their separate real-model and eight-hour
validation remain unresolved and are not established by these bounded checks.

## ORC-258 oracle-informed improvements (2026-10-06 01:34 UTC, release gates open)

All ten authorized contracts have source implementations, grouped into seven
review milestones. Final applicable default tests passed 3,558 cases
(42 ignored); all-feature tests passed 3,707 (43 ignored).
Both runs exclude exactly nine individually reproduced pre-existing codex-team
failures. Exclusions are not passes, and the unfiltered workspace remains failing.
See `docs/exec-plans/active/ORC-258-baseline-test-exceptions.md`.
Both full matrices ran at source `8d452a762e0e95a793e436e0b641476372a70747dcce76b0cfb052afb7f645b1`.
The final candidate changes only `len % 2 == 0` to `len.is_multiple_of(2)`
in benchmark median reporting. Exact reverse substitution recovers the tested
file hash; all other 1,016 inputs match. All 28 affected memory/CLI tests,
formatting, strict Clippy and release run after that equivalent lint change.
Formatting, strict workspace/all-target/all-feature Clippy and the all-feature
workspace release build pass. Code/build/test inventory: 1,017 files,
SHA-256 `a47efd5519ff39d8f0b30a8045fb2d995d8c730170e75f0a0b9b444cbc94b18c`.

Fresh signed candidate: `/Volumes/projects/ca/ca/scratch/oracle-improvements/final-validation/release-20261006T013149Z/ca`,
SHA-256 `fcae06c3a84fe284a18ed7bcfec6316dc4d6ae121f630b349bc35a3ca525db58`. Signature/version/help, exact render-buffer parity, all nine
actual TUI paths and short 4,096-message responsiveness checks pass. Short
keyboard p95: 19.785–20.584 ms. Read-only installer readiness is
separately recorded; no installed executable has been replaced.

The candidate's actual 28,800-second soak is running, not passed. Its first
minute process probe succeeded; the durable receipt is
`/Volumes/projects/ca/ca/scratch/oracle-improvements/final-validation/release-20261006T013149Z/eight-hour-soak.json`. Harness PID 93807, child PID
93811; earliest possible completion after actual idle is about
2026-10-06T05:32:11.490286-04:00 (America/New_York). Inspect final
status, actual elapsed >=28,800, post-idle p95 <100 ms, input/theme/resize and
terminal restoration before claiming a pass. The older 60.459-second denied
process-probe failure is retained as superseded evidence.

The additional Enter repair retires only a cancelled turn's owned response hook
before invalidating its generation. Actual fixed-debug and fresh signed hook
walkthroughs verify one cancellation followed by exactly one next submission,
physical cleanup and preserved draft; owner isolation/denial regressions pass.
Other bounded synthetic active reply/tool/child, 32-cycle and 4,096-message stream
probes passed sampled installed/current artifacts. The user's broader
intermittent installed-session failure has not been reproduced or fully closed.

Real-model hybrid quality and warm <=200 ms latency remain unmeasured pending
the user's explicit existing runtime/model selection. Synthetic fixture quality
and ranking timings cannot establish that gate. No model download, real service
installation, production provider request or installed binary replacement was
performed. Signed recoverable installation remains authorized after all gates
pass; resolve destination/candidate anew and leave running sessions alone.

Recoverable source baseline/oracle manifests are in
`scratch/oracle-improvements/20261005T211220Z/`; usable Git metadata is absent.
The final build and release directories are resolved through `LATEST_BUILD`
and `LATEST_RELEASE`. Historical BUG-257 paths below are dated evidence.
Cold identity inventories warm on existing blocking workers. Short write proofs
fail closed when inventory/deadline work is incomplete; the cooperative bound
is not a whole-persistence wall-clock guarantee. Private failed/superseded and
focused positive receipts remain in `final-validation/audit-repairs/`.

## BUG-257 long-session responsiveness and failure recovery (2026-10-04)

Implementation complete; duration/network validation and installation remain
open. Plan: `docs/exec-plans/active/BUG-257-long-session-responsiveness.md`.
Keys no longer share a FIFO with ticks, deferred frames respect the 16 ms
limit, transcript layout is indexed/cached off-loop, theme sync no longer
reads `/dev/tty`, idle maintenance/admission probes are throttled workers, and
unchanged missing-price admissions emit one notice. Ordinary chat is not
blocked by unavailable prices.

Eligible transient main-turn failures get at most one frozen-request
replacement after a one-second cancellable delay, only before any tool/hook
side effect. Failed source sessions keep partial output and completion
annotations; replacement branches exclude it and never replay preparation or
worktree hooks. `/errors` and metadata-only diagnostics retain safe categories.

Validation: 566 focused tests pass; fmt, strict all-target Clippy and optimized
all-feature workspace build pass. Release PTY with 4,096 messages: keyboard
p95 19.5–19.7 ms, paste order/theme/resize/terminal restoration pass. Full
workspace run has 12 failed targets, including denied loopback binds/nested
sandbox execution, unavailable process probes, fixture/binary paths and
unrelated assertions; not claimed green. See TESTING.md for commands/logs.
Actual eight-hour isolated PTY soak is running (session 89271), not passed:
`/private/tmp/ca-bug257-pty-uds9g6kg/result.json`, earliest completion about
2026-10-05 01:23 America/New_York. It uses the debug input/render implementation;
the final signed release separately passed the short PTY run.

Signed candidate: `target/release/ca`, SHA-256
`6e5e8811c09b5033c33dc43bd16183ae7b9417e1f13620bba210a0d61a2d91ea`;
build digest `32d0b4cad67dfcd419594b06c29c11736f8c8903ec8e73a4fcd4b6ccb34b9591`.
Signature, mode/owner, version/help verified. Release job 98631 completed;
log `/private/tmp/ca-bug257-release.log`. Installation staging failed with
EPERM at `/Users/aji/.local/bin`; the installed TUI-256 hash is unchanged.
`bash tools/bug257_install.sh --check` is a read-only preflight; running it
without `--check` in a normal terminal installs this exact candidate with a
recoverable backup, atomic replacement and verified rollback. No running
session, real config or real session store was changed for validation.

## TUI-256 Grok-style usage-limit graph (2026-09-30)

The Usage limit tab now matches the Grok compact graph: title, filled/empty
bar, percent, and a caption. Live `/usage` fills `limit_percent` from CA
provider data (MiMo observed/allocation, else DeepSeek remaining as caption,
else OpenAI month-to-date). The verbose provider dump is no longer the
primary body. Unsupported adapters are omitted from compact details.

Validation: focused graph/dashboard/`/usage` tests pass;
`cargo test --locked --bin ca` **2,661 passed, 0 failed, 40 ignored**;
`cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` pass.
Snapshot `agent_dashboard_usage_limit` shows the graph. No oracle source
copied. Release log `/tmp/ca-tui256-release.log`. Installed
`/Users/aji/.local/bin/ca`. Installed SHA-256 (post codesign):
`46d60ee15c7a4893c357e75eccd19b8f05eaaeacc1fc52c25cf122e913c07b41`.
Candidate pre-sign `3e1a56aa2e07d3883dc5ae60dbbd21adbc713b8dc5fcb0ab58299a2ad3751b4b`.
Recovery: `/Users/aji/.local/bin/ca.recovery-TUI256-20260930T155545Z-864eae83`
(prior TUI-255 binary
`864eae83f2861daa425648405210102c003cade408e2a2bcb9379562e54ae5c2`).
`codesign -vvv` valid on disk; `--version`/`--help` show `--dashboard`.
Running sessions left alone.

## TUI-255 Grok-style agent dashboard (2026-09-30)

CA now has a full-screen agent dashboard matching the Grok Build roster the
user asked for. `/dashboard` (alias `/agents-dashboard`), `Ctrl+\`, and
`ca dashboard` / `--dashboard` open it. The view shows the git/cwd header,
`+ New Agent`, worktree toggle, Open Previous, Working/Idle/Inactive groups,
a dispatch prompt, and a tabbed usage modal.

`/sessions` is still the saved-session picker. `/usage` is the Context /
Usage limit / Session info overlay. One attached conversation at a time;
Inactive rows are other saved sessions.

Validation: focused dashboard/usage/CLI tests pass; `cargo test --locked --bin ca`
**2,653 passed, 0 failed, 40 ignored**; `cargo fmt --check` and
`cargo clippy --all-targets -- -D warnings` pass. Reviewed wide/narrow
snapshots against the attached Grok screenshot. No oracle source copied.
Release log `/tmp/ca-tui255-release.log`. Installed `/Users/aji/.local/bin/ca`.
Installed SHA-256 (post codesign):
`864eae83f2861daa425648405210102c003cade408e2a2bcb9379562e54ae5c2`.
Recovery: `/Users/aji/.local/bin/ca.recovery-TUI255-20260930T151317Z-7096ad60`
(prior BUG-254 GitHub-sandbox binary
`7096ad605c48c80cd1e4c01b3c0110b64f86063fcbc794ac70e9277eed39a977`).
`codesign -vvv` valid on disk; `--version`/`--help` show `--dashboard` and
`ca dashboard`. Running sessions left alone.

## GitHub/git through required sandbox (2026-09-26)

User: GitHub work was blocked; isolation was too aggressive. Root causes in the
network classifier and the sandbox credential profile:

1. `gh` (and curl/wget/rsync) were never treated as network clients, so they
   ran with `(deny network*)` even when `run_command` was allowed.
2. Compound stages (`cd repo && git push`, `git status && gh pr create`) only
   inspected the first word, so the real remote command got no network grant.
3. Homebrew/`/usr/local` program paths were not trusted spellings for network
   classification, so `/opt/homebrew/bin/gh` was ignored.
4. Even with network granted, `~/.ssh` (known_hosts), `~/.config/gh`, and git
   credential files were read-only, so SSH host keys and `gh` auth state could
   not be written.

Fixes in `src/workspace.rs` (stage-wise network detection + `gh`/HTTP clients)
and `src/sandbox.rs` (network-allowed profile grants VCS credential write).
`required` sandbox mode is unchanged; GitHub work now uses the normal
combined tool/network decision.

Validation: 15 workspace + 13 sandbox tests, full `cargo test --locked --bin ca`
**2,644 passed, 0 failed, 40 ignored**, `cargo fmt --check` and strict Clippy.
Release build installed. Installed SHA-256 (post codesign):
`7096ad605c48c80cd1e4c01b3c0110b64f86063fcbc794ac70e9277eed39a977`.
Recovery: `/Users/aji/.local/bin/ca.recovery-github-sandbox-20260927T001849Z`
(prior BUG-254 binary `cb26bc9528b2df55b87583b9c943e5a1078336f13e16f5ac29075efb423e2425`).
`codesign -vvv` valid on disk; `--version`/`--help` pass.

## BUG-254 unblock install (2026-09-25)

Second release after user reported `No reply after recovery. Resubmit for a clean branch.` and `await_job` freezing the turn on a multi-hour managed job.

1. `await_job` waits are bounded: omitted `timeout_secs` defaults to 30s, explicit
   timeouts clamp to 120s, and a timeout returns the running snapshot instead of
   hanging. Tool description documents this.
2. Failure footers stay short enough for the one-line status bar so the action
   (`Resubmit for a clean branch` / `/retry`) is never truncated. Snapshots updated.
3. Prior BUG-254 fixes (stale retry fallback, history-index shift, session-clear,
   `Waiting for managed job…`) remain.

Validation: 8 focused tests, full `cargo test --locked --bin ca` **2,643 passed,
0 failed, 40 ignored**, `cargo fmt --check` and strict Clippy pass.
Release log `/tmp/ca-bug254-unblock-build.log`.
Installed SHA-256 (post codesign): `cb26bc9528b2df55b87583b9c943e5a1078336f13e16f5ac29075efb423e2425`.
Recovery: `/Users/aji/.local/bin/ca.recovery-unblock-20260926T010850Z-309d92a9`.

## BUG-254 retry boundary and tool status (2026-09-25)

Two user-visible TUI failures are fixed and the release is installed.

1. `/retry` no longer fails with `retry boundary is not a user request` when the
   stored index is stale. `prepare_retry_branch` validates the boundary against
   the live transcript and falls back to the latest user message. History
   prepend now shifts `retry_boundary` / `active_turn_start_message_count`, and
   `apply_session_state` clears them when the transcript is replaced. A new user
   turn also clears a leftover retry boundary.
2. A tool batch whose only pending call is `await_job` now reports
   `Waiting for managed job…` (plural for multiple awaits) instead of a frozen
   `Executing tools…`. Single other tools report `Executing <name>…`.

Validation: 6 focused tests, full `cargo test --locked --bin ca` **2,641 passed,
0 failed, 40 ignored**, `cargo fmt --check` and strict all-target Clippy pass.
Release build log `/tmp/ca-bug254-release-build.log`.
Installed SHA-256 (post ad-hoc codesign): `309d92a9c3d97cddb81a9f683e8405f06fcc07d628aff9a6b112323e1b9cd177`.
Recovery copy: `/Users/aji/.local/bin/ca.recovery-BUG254-20260925T232448Z-d0a6b0ac`
(prior HRS-253 binary). `codesign -vvv` valid on disk; `--version`/`--help` pass.
Plan: `docs/exec-plans/active/BUG-254-stale-retry-boundary-and-tool-status.md`.

## HRS-253 complete and installed (2026-09-23)

The six-stage implementation and authorized dedicated-VM setup are complete.
The release is installed at `/Users/aji/.local/bin/ca` and automatic idle experiments
are enabled for `/Volumes/projects/ca/ca`, capped at $20 and 7,200 active seconds
per cycle. They require five quiet minutes and a 24-hour cooldown. Existing
running sessions were left alone; the new configuration applies on the next launch.

Final all-feature workspace tests: **3,461 passed, 9 failed, 42 ignored**. All 2,666 application
tests pass. Strict all-target/all-feature Clippy and formatting pass. The final
54-trial VM guidance lifecycle passes: 51 verified successes, three expected
baseline failures, no infrastructure failures or false completions, 106 local
provider requests, activation and rollback, and no outstanding reservations
(2,053.29 seconds). Final Tart inventory confirms both local templates stopped
and no disposable VM clones remain.
This used a deterministic local provider and real VM workers/verification; it
does not establish improvement on real-provider tasks.

The existing workspace integration failures remain in `mcp_sdk` (five),
`protected_baseline_cli` (three) and `remote_cli` (one). Engineering admission
continues to reject candidates that encounter them, including guidance candidates.
No gate was waived. The original checkout also contains pre-existing uncommitted
work, so source-changing experiments remain ineligible until it is clean.
Existing edits were preserved; no reset, stash or commit was performed.

The selected DeepSeek pricing refresh succeeded from its official page. New
cards explicitly account at the peak-rate upper bound; they are not invoice
amounts. Reservations use the verified provider context ceiling independently
of a smaller configured request-size cap. Historical pricing identities remain
unchanged. No paid-provider efficacy run is claimed.

The optimized workspace build passed. Candidate/staged/installed hashes,
ownership, executable mode, ad-hoc signature and version/help checks passed.
Installed SHA-256: `d0a6b0acf53bc33a7e30a8941bb54a6d8d558f100b6946b48e96a0b82349f4de`.
The previous executable and opaque configuration are recoverable in
`/Users/aji/.local/bin/.ca-recovery-HRS253-20260924T000247Z-27221`.
The staged configuration passed schema/settings validation in a private HOME;
saved credentials were resolved by the installed application at the original
config location, and its model-list check passed.

Final logs: `/tmp/ca-hrs253-final-workspace-tests.log`,
`/tmp/ca-hrs253-final-clippy.log`, `/tmp/ca-hrs253-final-format.log`,
`/tmp/ca-hrs253-vm-full-cycle.log` and `/tmp/ca-hrs253-final-install.log`.
Installation receipt: `/Users/aji/Documents/Codex/2026-09-22/https-github-com-china-qijizhifeng-agentic/work/hrs253-recovery/final-install-receipt.json`.

Completed plan: `docs/exec-plans/completed/HRS-253-evidence-driven-harness-improvement.md`.
Earlier HRS-253 progress sections below are historical. This receipt supersedes
their pending handles, installation state and proposed next steps.

## HRS-253 release and live-pricing checkpoint (2026-09-23)

Release build session 37868 is finished successfully (2m32s). The fresh
84,022,608-byte target/release/ca runs --version and has a valid code signature;
it is not installed. Full VM lifecycle session 86980 is LIVE: optimized test
compilation finished in 4m27s, and the one 54-trial test is now hashing the real
VM image (PID 83003 observed running at 98.7% CPU). Log:
/tmp/ca-hrs253-vm-full-cycle.log. Poll this exact handle; do not rebuild/replace
its baseline binary while it runs. Formatting follow-up 81969 is finished/pass.

Read-only --list-models identifies the configured default as deepseek-v4-pro.
Pricing status has zero fresh cards (all nine dated Aug 15). Official refresh
session 16304 finished with `official pricing source was incomplete or changed`;
no successful refresh or live inference occurred. Current official DeepSeek
pricing was downloaded to the task's work/hrs253-recovery/deepseek-pricing.html:
https://api-docs.deepseek.com/quick_start/pricing/. It lists a 1M context and Pro
peak cache-hit/input/output rates of $0.044/$1.32/$3.96 per million tokens;
off-peak rates are half. Flash peak rates are $0.006/$0.30/$1.20. Peak hours:
01:00–04:00 and 06:00–10:00 UTC on weekdays excluding Chinese public holidays.
Legacy v4-flash aliases now route to V4.1 Flash. Existing seeded prices and
validator strings in src/pricing.rs are outdated; do not bypass freshness or
call them current. Correct selected-provider refresh/accounting before enabling
automatic spending. The shared provider_contract model profile already declares
DeepSeek's 1,000,000-token context; connect that trusted route bound to harness
reservation validation instead of relying only on configurable context values.

## HRS-253 interrupted-cycle recovery connected (2026-09-23)

The full explicit/idle driver now holds a distinct controller lock across worker
boundaries. VM pre-decision abandonment requires that lock, artifact-worker
ownership, VM ownership, matching frozen inputs and no disposable clone or
activation journal. Closed cycles retain unknown reservations and settled usage;
the digest-bound closure is immutable. CLI `harness abandon` and owned idle
recovery are wired. User pauses, ordinary settled foreground resume, promotion
recovery and cooldown remain separate. Legacy host-only plans cannot use this
VM proof. Correction to the earlier checkpoint: stopped leftover clones now
also block admission, because an orphan Tart runner could still be starting one.

Four focused recovery checks, 2,660 all-feature ca tests (38 ignored), 14 shared
ledger tests, strict workspace Clippy, format and diff pass. Logs use
/tmp/ca-hrs253-recovery-*.log. A new opt-in full 54-trial VM guidance lifecycle
rehashes the real selected image and runs production freezing, gates, workers,
independent verification, CLI activation and rollback with a local test provider.
Its release build and run are in progress; do not call that test complete.
Current handles: release build 37868 (/tmp/ca-hrs253-final-release-build.log),
full-cycle command 86980 (/tmp/ca-hrs253-vm-full-cycle.log). Poll these exact
handles before restarting anything. No installed binary or runtime config changed.

## HRS-253 VM cleanup ownership (2026-09-23)

Each Tart home now has one private controller lock, inherited only by the VM
supervisor and retained through cleanup. Parent cancellation cannot admit a new
guest while its supervisor is still cleaning up. Startup also rejects running
or uncertain disposable leftovers. The parent retains its copy through final
verification; no explicit unlock or lock-file deletion breaks shared ownership.

Failure-first descriptor inheritance test reproduced missing ownership; all 18
focused VM checks now pass (11 opt-in ignored). Real startup/ownership/cleanup
passes in 16.67s. Strict workspace Clippy and format pass. Broader ca run: 2,655
pass, one unrelated external-runtime cancellation fixture failure, 38 ignored;
the failing test passes unchanged in isolation. See the active plan for logs.
Both templates are stopped and no disposable clone remains. Unknown-charge
abandonment/recovery is still pending; reservations have not been cleared or
relabeled as actual spending. Acquire both the artifact-worker lock and this
VM ownership lock, then inspect inventory, before recovering a dead cycle.
Full source/idle/live-provider validation and final configuration/install remain.

## HRS-253 VM activation and admission (2026-09-23)

VM execution now covers engineering, workers, result import, independent
verification, signed artifact identity/help/version, and installed activation/
rollback postchecks. The tiny-project build/adopt/activate/rollback probe passes
in 287.10s with 275 charged active seconds; no outstanding reservations remain.
The test supplies controlled trial receipts and does not establish a full
54-trial measured improvement. Main evaluation/trial guards are replaced with
frozen-backend validation; invalid VM bindings still cannot fall back to host.

Recovery no longer uses the entire application source hash as adapter identity.
A shared protected-input list drives the compiled authority/dependency digest;
ordinary application repairs preserve it, controller/dependency edits change it.
The failure-first test passes after this correction. Saved plans remain immutable.
Source/build/binary identities retain their existing independent bindings.
The new admission suite passes 15 checks (11 opt-in tests ignored). Final host
checks pass 2,653 units (38 ignored), strict all-feature workspace Clippy, format
and diff. All real VM probes are settled and both templates are stopped.
Next finish provider-specific context proof, unknown-charge/incomplete-cycle
recovery, full source/idle lifecycle and bounded live validation, then configure
and install as authorized. HRS-253 remains ACTIVE.

## HRS-253 VM worker/result/verifier integration (2026-09-23)

The real ca worker now passes guest editing, host-only provider framing and
protected-source denial (36.68s). Bounded result export/import passes a real VM
probe (13.15s) and rejects links, races, changed protected inputs and extra files.
The baseline verifier runs in a separate fresh VM. All six buggy inputs and
six repairs pass the expected checks (323.94s). A complete worker/edit/export/
cleanup/fresh-verifier trial passes (62.17s, two local provider requests). Trial runner
selection is wired, but both overall VM entry guards remain until executable
identity/activation checks and complete trial validation are migrated. Automatic
activation and installed-binary replacement remain pending.

The corrected current-ca VM full gate completed in 555.73s with 3,445 passes,
nine existing integration failures and 35 ignored tests. All 2,650 ca unit tests
passed; removing the inherited gate CARGO_TARGET_DIR fixed the nested Cargo
sandbox failure. Retain the previous failed run as evidence. Latest host checks
after trial selection: 2,652 passes/36 ignored, strict workspace Clippy and
format pass. All checkpoint probes are settled; see the active plan for logs.

## HRS-253 VM engineering checkpoint (2026-09-23)

Fixed engineering gates now select the frozen VM backend for guidance/source
builds. Release artifacts return as bounded opaque bytes, are published only
in the private checkout, and their hashes bind subsequent smoke receipts.
The real tiny-project 12-gate matrix passes (148.51s); current-ca format passes
(30.74s). Host ca checks pass 2,648 tests (31 ignored), strict all-feature
workspace Clippy and format pass. The real all-target/all-feature ca VM gate
finished in 523.39s: 3,442 passed, ten failed, 33 ignored. Nine failures are
previously recorded integrations; the other nested-sandbox Cargo fixture
inherited the gate's CARGO_TARGET_DIR outside its grant. The gate launcher now
omits that override; it still builds in the fixed source root's default target.
That correction awaits a real rerun. Log: /tmp/ca-hrs253-vm-engineering-ca-all.log.
Both templates are stopped and the owned clone was deleted. Do not treat the
failed gate as an admission receipt. Worker framing migration is in progress;
overall evaluation/trial VM guards remain closed. Runtime activation/install
and the full source/idle lifecycle remain unfinished.

## HRS-253 VM transfer checkpoint (2026-09-23)

All 2,646 current all-feature ca tests pass with four test threads (29 ignored).
Strict workspace Clippy, format and diff pass. No checkpoint process remains
live; both prepared VMs are stopped and the disposable transfer VM is deleted.


VM source/workspace/worker transfer and named artifact import are implemented.
Nine focused checks pass, including 16 hostile-transfer and ten supervisor
cases. Actual guest source/worker protection, writable task/build output, exact
artifact bytes, artifact-symlink rejection and cleanup pass in 11.90 seconds.
Strict all-feature workspace Clippy and formatting pass. A poll/kill race found
during these checks is fixed with a failure-first regression. See the active
plan's Bounded VM transfers entry for limits, exact logs and the initial failed
fixtures. Candidate execution still fails closed pending fixed-gate routing,
private artifact publication, worker/provider framing and verifier migration.

## HRS-253 owned VM controller checkpoint (2026-09-23)

The Rust lease controller and embedded supervisor are implemented and tested.
Actual clone/boot/guest-agent identity/stop/delete passes; the same supervisor
also passes owner-SIGKILL cleanup with a detached guest child. Seven focused
checks pass; strict workspace Clippy, format and diff pass. The all-feature ca
run has 2,643 passes and one privilege-fixture failure that passes unchanged in
isolation. See the active plan's Owned VM controller section for exact logs.
Both prepared VMs are stopped; no checkpoint process remains live. Next connect
bounded transfer/import and every candidate-execution boundary. VM plan entry
points still fail closed until this is done; the controller module's dead-code
allowance is transitional. No user config or installed binary has changed.



### Frozen VM environment binding — 2026-09-23

New optional `harness_improvement.vm` configuration freezes the selected Tart
binary, stopped template config/disk/NVRAM, guest-agent digest and host adapter
source digest into each new plan. Legacy plans omit the optional field and keep
their prior digest. Input reads reject links, unsafe ownership/permissions,
replaceable parent directories and concurrent changes. Subsequent checks detect
same-length edits even when mtime is restored. Both explicit and idle creation
charge preparation time, including image hashing.

VM-bound evaluation, engineering, source-build and trial entry points currently
fail explicitly before host execution. These are transitional guards, not a
finished VM backend; remove each only when its actual execution path is migrated.
User runtime configuration remains unchanged.

Validation: four focused VM tests and one shared plan-binding test pass. The
replaceable-parent regression failed before its fix. Strict workspace Clippy
(all targets/all features), full formatting and diff checks pass. Logs:
/tmp/ca-hrs253-vm-binding-tests4.log,
/tmp/ca-hrs253-vm-binding-core-tests3.log,
/tmp/ca-hrs253-vm-parent-before.log,
/tmp/ca-hrs253-vm-binding-{clippy,format,diff}.log.

Full all-feature workspace run: 3,420 passed, 25 failed, 29 ignored, including
all 2,641 ca unit tests passing. Nine failures match the previously recorded
integration failures. Sixteen shared-library commit tests fail because the host
fixture environment has no Git identity: one repeats in isolation and passes
with explicit fixture-only author/committer identity. The entire unchanged
shared-library binary then passes 576 tests (one ignored) with that identity.
No user Git config or production code was changed to obtain this result. Keep
the original failed run. Logs: /tmp/ca-hrs253-vm-binding-workspace.log,
/tmp/ca-hrs253-vm-binding-commit-{isolated,identity}.log and
/tmp/ca-hrs253-vm-binding-core-identity.log.

The unoptimized real-image freeze probe exceeded its explicit 600-second bound;
it failed, without booting or changing the VM. Optimized verification passed in 427.36 seconds under the same 600-second
bound. Logs: /tmp/ca-hrs253-vm-real-binding{,-release}.log. The frozen receipt is
retained at work/hrs253-vm-bootstrap/frozen-vm-verified.json in the Codex task;
its adapter source digest predates the controller added below. Disk SHA-256:
0050fcb56c7b9d0a12d02bd243ed25a01fd5d8a6de7acd68bb29fef70be920d4.

An independently sessioned host supervisor prototype lives in the Codex task's
work/hrs253-vm-lifecycle directory. Nine mock lifecycle tests pass: owner EOF,
abrupt owner exit, hard deadline, termination, template drift, cancellation
during clone, failed boot, failed cleanup and bounded inventory output. A real fresh
clone also passed: non-admin guest child PID/SID 370 escaped its parent's
session, then the owner was SIGKILLed. Pipe EOF caused the surviving supervisor
to stop the whole VM, confirm stopped state and remove only its disposable
clone. Independent inventory confirms the clone is gone and both prepared VMs
remain stopped. Receipt: work/hrs253-vm-lifecycle/
probe-22149b228c4448ab86e7e4519c1883e1/result.json. This is a controller prototype
probe, not production harness wiring or an engineering/promotion receipt.


## HRS-253 harness evidence implementation (2026-09-23; partial)

Active plan: `docs/exec-plans/completed/HRS-253-evidence-driven-harness-improvement.md`.
The user approved all six stages, automatic source/guidance candidates, $20/two
active hours per cycle, and clean-current-branch source adoption.

Implemented: task/event evidence, explicit feedback, delegated root attribution,
scoped verification, actual request fingerprints, sourced analysis, cohort trends,
frozen repeated coding evaluations, durable spend/time accounting, guidance/source
generation, private source builds, inspection and pause/resume. Prepared requests
now retain the resolved route so headless/service fingerprints include the actual
protocol/endpoint, matching explicit main/subagent route capture.

Explicit source runs connect verified commit import, signed staging, clean
fast-forward adoption and atomic executable replacement. Rollback restores the
baseline binary and adds a source revert only on the unchanged clean adopted
branch. Later user work is preserved and reported as deferred recovery.

Guidance runs now publish the evaluated repository-work paragraph in the built-in
system prefix. Build, provider route, model and options scope the active pointer.
Preparation applies it before telemetry and transport, preserves user text and
cache boundaries, and adjusts context estimates. Custom prompts without exactly
one matching slot retain their own instructions. Analysis and frozen experiment
requests do not load active guidance. Creation defaults to the active paragraph;
unrelated custom guidance baselines are rejected before generation.

Guidance rollback restores the previous promoted version, including after a
newer version replaced it. A refusal to roll back an older deployment preserves
its receipt so it remains a valid recovery target. Publication is not consumed
until time accounting settles. New-cycle admission also rejects unsettled work
in terminal-looking cycles. Recovery charges its own interrupted host reservation
at its full maximum and never clears unrelated charges.

Validation: combined suite passed 77 tests, including all 13 opt-in process tests
(`/tmp/ca-hrs253-guidance-combined-tests.log`). Final accounting/route refinements
then passed all nine targeted tests (`/tmp/ca-hrs253-guidance-final-accounting-tests.log`)
and all 166 client/request/headless/service/subagent tests
(`/tmp/ca-hrs253-guidance-final-runtime-tests.log`). Fresh debug build and all-feature
workspace check passed with existing warnings (`/tmp/ca-hrs253-guidance-final-debug.log`,
`/tmp/ca-hrs253-guidance-final-workspace-check.log`). Task-owned formatting and diff
checks pass. Source/guidance transaction fixtures use disposable repositories and
synthetic matched trial receipts; they do not establish a full automatic live cycle
or prove that the full ca engineering matrix passes its closed profile.

Independent text-input byte caps run before provider spend reservation and on
the final protocol body. Image inputs are rejected for these experiments.
Provider-specific token/context accounting still needs validation before live
automatic cycles. No paid provider request, user-vault activation, original-branch
adoption or installed-executable replacement occurred during implementation tests.

Guidance creation now freezes uncommitted build inputs matching the running
binary into a private Git repository. The bounded copier rejects links and
special files, excludes root runtime and oracle/reference trees, rechecks source
bytes, and leaves the original HEAD/index/working tree alone. Build and runtime
hashing share the same encoding; runtime reads are bounded. The sealed plan binds
the snapshot and charges freezing time. Engineering, trials and activation resolve
that private snapshot and recheck actual bytes, allowing later original edits.
Four snapshot/freezer tests, all 12 core lifecycle tests, and the extended real
private-project activation/recovery/rollback test pass. Logs:
`/tmp/ca-hrs253-dirty-guidance-{bounded-tests,core,activation}.log`.
The activation fixture uses synthetic matched trial receipts; full automatic
end-to-end coverage is still required. Fresh debug build and all-feature workspace
check pass with existing warnings; task-owned format and diff checks pass.
The final combined experiment suite passed all 72 tests, including opt-in process
checks (`/tmp/ca-hrs253-dirty-guidance-final-combined.log`). A pause regression
caught in the earlier run is fixed: stopped cycles do not require an available
repository. Final build/check logs are
`/tmp/ca-hrs253-dirty-guidance-final-{debug,workspace}.log`.
Do not rebuild the referenced worker executable during process tests: verifier
receipts bind the build identity; an earlier mixed-build run correctly failed.

`harness discover [--json]` groups unused current-build failures by exact runtime,
workspace and tool, excluding evidence used by any saved cycle. It shares freezer
attribution checks, rejects duplicate execution evidence, and preserves independent
roots when capping events. Outcome reads are bounded and reject links; unknown or
ambiguous diagnostic signatures remain unclassified.

`harness create --auto` now matches the configured provider/model/route and freezes
six regressions plus a target and two held-out variants. Eighteen embedded
host-owned implementations cover all six supported families. Automatic source
selection requires diagnostic paths from the same clean ca checkout; other
eligible groups select guidance. Discovery/materialization/freezing seconds are
sealed and charged for source and guidance cycles. Legacy accounting is preserved.
Git inspection disables optional index writes, fsmonitor and hooks.

The new catalog exposed a verifier false positive: inherited pipes stayed open
until a helper naturally exited, hiding missing timeout cleanup. Verifier pipe
capture now stops at the command deadline and marks timeout. Hidden probes also
cover UTF-8 prefix retention, relative scopes, duplicate call starts, partial
UTF-8 journal tails and unknown configuration keys.

Latest validation: 10 selection/freezer tests and 13 core lifecycle tests pass
(`/tmp/ca-hrs253-auto-selection-checked-tests.log`, `/tmp/ca-hrs253-catalog-core.log`).
All seven catalog/verifier tests pass, including actual verification of all 18
variants before/after repair and the original six baseline/repair pairs
(`/tmp/ca-hrs253-catalog-final-process.log`). The failed initial catalog run is
retained at `/tmp/ca-hrs253-catalog-process.log`. Fresh debug build and all-feature
workspace check pass with existing warnings (`/tmp/ca-hrs253-catalog-pipe-debug.log`,
`/tmp/ca-hrs253-catalog-final-workspace.log`). CLI help, scoped formatting and diff
checks pass. The earlier 72-test experiment checkpoint predates these changes;
full workspace and end-to-end automatic-cycle validation still remain.

Idle dispatch is wired into the TUI behind `harness_improvement.enabled` (default
false). Five uninterrupted quiet minutes, network capability and no learning job
are required. Admission rechecks the 24-hour cooldown under the freezer lock.
The shared runner yields at settled operation boundaries and resumes only its own
foreground-paused cycle with matching profile, repository and executable. Explicit
user pauses remain paused. The supervisor signals foreground activity on input
and shutdown. Blocking discovery/snapshot preparation now checks that signal
between bounded file operations and private Git polls; unpublished snapshots are
dropped on cancellation. An individual filesystem call can still delay shutdown.
This is current-TUI activity detection, not host-wide activity tracking. No runtime
configuration was changed. Latest focused suite: 116 passed
(`/tmp/ca-hrs253-idle-owned-tests.log`); 13 core lifecycle tests passed
(`/tmp/ca-hrs253-idle-core.log`). The combined harness suite, including opt-in
process tests, passed all 87 tests (`/tmp/ca-hrs253-idle-combined.log`). Fresh debug
build and all-feature workspace check pass with existing warnings
(`/tmp/ca-hrs253-idle-{debug,workspace}.log`); scoped formatting and diff checks pass.
Three new cancellation tests cover scoped worker cancellation/unwind, supervisor
shutdown joining the blocking worker, and cancellation without snapshot/cycle
publication. All 18 discovery/freezer/snapshot/idle tests pass
(`/tmp/ca-hrs253-idle-cancellation-final-tests.log`). The 87-test process checkpoint
predates this cancellation refinement. Full idle-cycle coverage remains open.

Full guidance-cycle integration now PASSES through the evaluated CLI: discovered
evidence, actual generation, foreground pause/resume without replay, all 54 real
worker trials, 106 accounted local fake-provider requests, tiny-project engineering
checks, guidance activation, consumed-evidence exclusion and rollback. Log:
`/tmp/ca-hrs253-integration-full-cycle5.log` (590.09 seconds). Earlier failed fixture
runs remain documented. This proves the complete guidance path on the build before
the new hint wiring; it does not prove real-ca engineering gates or measured hints.
Session 11583 is terminal. Its signed symbol-stripped worker remains in the task's
`work/hrs253-worker/ca`; no executable was installed.

Hint policies now travel through frozen generation inputs/proposals, trial hashes,
worker setup, runtime activation and rollback. A hint proposal adds one intervention
with unchanged instructions. Empty policies omit their fields to preserve legacy
hashes. Worker and runtime use the same bounded completed-call transformation;
runtime Config fingerprints include the active policy even before a hint fires.
Active pointers reconstruct the evaluated candidate hash, preventing an altered
pointer+journal from enabling an unevaluated policy. Twenty focused tests pass
(`/tmp/ca-hrs253-hints-wiring-final-tests.log`), one process test intentionally ignored.
Exposure checks are implemented: the host checks the exact worker system
message against the frozen policy, records delivered hints in the verification
artifact, and requires exposure in every candidate target repetition before
activation. Exposure suite: 19 passed, one process test ignored
(`/tmp/ca-hrs253-hints-exposure-tests.log`). The real worker process test passed
with policy disabled and enabled, checking exact trigger delivery and no reminder
on the next request (`/tmp/ca-hrs253-hints-worker-process2.log`, four seconds).
The first process-test attempt failed to compile a fixture JSON macro; it is fixed
and its failed log retained. Fresh worker build passed; trial artifacts now retain
bounded redacted worker failures and provider-request counts, removing the
previous unused WorkerResult-field warning. Final focused suite: 20 passed, one process test ignored; all-feature workspace
check passed with the two existing renderer warnings
(`/tmp/ca-hrs253-hints-final-{focused,workspace}.log`). Scoped format and diff
checks pass. All named sessions in this checkpoint are terminal. No live hint is enabled.

The full measured-hint lifecycle now PASSES: 54 real worker trials, 124 accounted
local fake-provider requests, exposure checks, engineering gates in the tiny
project, activation and rollback (`/tmp/ca-hrs253-hint-full-cycle.log`, 583.47 s).
Both target lanes receive identical probes; only the delivered reminder causes
the deterministic provider to repair the target. This validates the mechanism,
not real-world hint efficacy. Session 6914 is terminal.

Source policy carry-forward is implemented and validated at the binding and
disposable-project process boundaries. Signed-artifact
preparation reads the compiled build identity in its offline sandbox and binds
the evaluated text/hints and frozen provider route into the activation journal.
Publication is replayable; runtime consumption requires the matching source
transaction and core cycle to be settled and promoted. Rollback disables the new
build policy while preserving the old build pointer. Conflicting later guidance
is never overwritten. The synthetic binding test, all four source activation/
recovery process tests, and the real signed executable identity process test pass.
Final focused suite: 92 passed, 18 opt-in tests ignored. All-feature workspace
check and scoped format/diff checks pass. Logs are
`/tmp/ca-hrs253-source-guidance-{process,identity,focused,workspace,format,diff}.log`.
The source-process tests use synthetic coding receipts in a tiny engineering
project; they do not prove the full real-ca source-generation/evaluation path.
Sessions 24626 (initial check failed on return type, fixed), 97792, 18720, 47206,
41376 and 57898 are terminal. The private worker was rebuilt and signed after
the full hint-cycle run ended; it is not installed.

The first real-ca `test_default` engineering probe failed after 208.99 seconds
in a retained private snapshot under the production closed profile. Session
51642 is terminal. Snapshot:
`/private/var/folders/nx/1xjt7xc96zg_d9w7glkcyvhr0000gn/T/ca-hrs253-real-gate-wuOZ8U/vault/harness/inputs/guidance-source-l4EVQv/repository`.
Log: `/tmp/ca-hrs253-real-ca-engineering-test.log`. Its diagnostic was dominated
by repeated `/etc/gitattributes` permission warnings, obscuring stdout failures.
The clean environment now sets `GIT_ATTR_NOSYSTEM=1`. Diagnostics retain separate
redacted stdout/stderr tails within 4096 characters while hashing full output.
A regression exposed a second truncation in the learning sanitizer; the code now
uses its untruncated privacy-filter path before applying each tail bound. Session
28391 is terminal (failed regression before that fix). The corrected diagnostic
test passed (`/tmp/ca-hrs253-engineering-tail-test3.log`). Session 23145 is terminal: the second real gate failed in nine targets after
205.72 seconds (`/tmp/ca-hrs253-real-ca-engineering-test2.log`). It eliminated the
system-attributes warnings and retained the failure summary, but bounded excerpts
cannot show every earlier cause. An opt-in test-only `CA_HARNESS_TRACE_OUTPUT=1`
now tees bounded command streams to the diagnostic test log without changing
production receipts or passing the flag into tested subprocesses. Session 83451
finished the third real gate, failed after 212.95 seconds:
`/tmp/ca-hrs253-real-ca-engineering-test3.log`. It proves loopback bind and nested
sandbox installation failures (EPERM), plus restricted process observation;
ca bin reports 2383 passed, 224 failed, 24 ignored. An asynchronous user choice
is pending: dedicated macOS VM versus host candidate checks with isolated
HOME/workspace. No policy change made. Existing Lima VM was not used or changed.
Normal host development validation is independent of that choice. The stale
prepared-request count was updated for existing install_binary, with an explicit
assertion; session 57089 runs its regression. A current workspace default test
run with system/global Git config disabled is also running at
`/tmp/ca-hrs253-workspace-current-default.log`. The private snapshot already includes
integration tests, docs and support files beyond the compiled identity inputs.

Still required: idle lifecycle validation, escaped-descendant cleanup,
provider-specific context/token proof, real ca engineering-profile fixes, full
validation, a bounded live cycle and release/install.
The debug binary is not installed. The checkout began with 1,251 changed paths;
preserve them. The task baseline lives outside the repository in the Codex task's
`work/hrs253-baseline`. No oracle or reference files were changed.

## HRS-252 Apple Container execution-backend design note (2026-09-19)

Docs-only. User asked for a short design note relating Hermes #78035 Apple
Container work to `ca` (not code). Wrote `docs/HRS-252-apple-container-execution-backend.md`:
`SandboxMode` stays policy strength; future `ExecutionBackend::{HostSeatbelt,
AppleContainer}` would sit under jobs/`run_command` only; policy→mount/network
map; fail-closed + owner-bound lifetime; no oracle copy. Claimed `NEXT_STEPS`
HRS-252 (`Status: DRAFT`) and `PARITY` HRS-252 (CA extension, no Cursor parity,
`Unverified`). Not activated. `oracles/hermes-agent-2026.9.14` has no
`apple_container.py` (PR unmerged). Separate process track: user holds Hermes
#78035 open on its published ~2026-09-26 maintainer deadline.

## HRS-250 / HRS-251 Hermes context & cache (2026-09-17)

Implemented the Hermes 2026.9.14 Tier-1 transfer set after the Cursor-study
plans. `ContextCheckpoint` gained optional anchors, verbatim user quotes, and
recall pointer; checkpoint messages include those sections; the checkpoint
prompt forbids summarizing user instructions; generation fills the fields from
the covered prefix. In-place compaction remains prefix-on-same-session
(tested). SessionV4 gains `ContextUsageAnchor` (provider token counts +
content fingerprint + 60/300/900s checkpoint-failure cooldown). Skill
activate/deactivate status is cache-aware (`skill_mutation_cache_note`,
optional `--now`). Session recall FTS5 create failure degrades to a
LIKE-style fallback table. QUALITY GRD-009a + ARCHITECTURE session-scoped
capability note. Focused tests, fmt, clippy pass; no install. Plans:
`docs/exec-plans/completed/HRS-250-hermes-context-cache-compaction.md`,
`HRS-251-hermes-context-compaction-cache-resilience.md`. ESTOP sentinel
deferred.

## CUR-245 / TUI-247 / CMD-248 Cursor-informed execution (2026-09-17)

Completed the remaining Cursor-study DRAFT plans after SEC-246 and SES-249.
CUR-245: Ask tool set is empty via allowed_names; Plan profile is
proposal-only; launch `--mode plan|ask` now sets tool-policy status strings.
TUI-247: Headless already has ToolStart/ToolFinish; characterization locked;
card timing deferred (no host duration on ToolCall). CMD-248: model-id slash
alias subordinate to `/model`; visible-command descriptions tested; `/tasks`
usage added. Focused tests, fmt, clippy pass; no install this turn. Index:
`docs/CURSOR_INFORMED_PLAN_INDEX.md`.

## SES-249 headless session identity and resume (2026-09-17)

Executed after SEC-246. Characterization confirmed existing CLI conflicts for
`--no-session` vs resume paths and added `--new-session-id` exclusivity tests.
SessionStore fixtures prove resume-by-public-id retains conversational memory
and unsaved identities do not appear in the store. Result-envelope session id
echo deferred. Focused tests, fmt, clippy pass; no install. Plan:
`docs/exec-plans/completed/SES-249-headless-session-identity-and-resume.md`.

## SEC-246 trust/approval/sandbox prompt clarity (2026-09-17)

Activated from the Cursor-informed plan index. Characterization locked CLI
`--yolo`/`--auto-review` exclusivity and `--trust` + `--continue` rejection.
Interactive workspace-trust prompt names the path, states tools stay blocked
until trust, and lists y/n plus headless `--trust` process-only recovery.
`/auto-review` status and `--auto-review` help label CA-local safe-tool
evaluation (not a remote classifier). `ca doctor` reports approval mode with
that wording and clearer sandbox fail-closed copy. `ProviderManagementSnapshot`
carries `approval_mode`. Focused binary tests, fmt, and strict clippy pass; no
install this turn. Plan:
`docs/exec-plans/completed/SEC-246-trust-approval-sandbox-prompt-clarity.md`.

## PLN-244 plan card lifetime (2026-09-05)

The user's follow-up clarifies that expanded review should not leave a card
pinned throughout work. Rendering now follows the existing `plan_proposal`
binding and requires an idle foreground turn. Approval, dismissal, revision,
rejection, successful execution, and restored artifacts therefore leave no
inline card; `/plan show` still opens the saved plan. PLN-243's complete task
wrapping is retained during review. Reader Esc/q also dismisses the review.
Final focused tests (40), CA tests (2,534), format/lint, and debug/release PTY
checks at both sizes pass. The broad workspace run retained one unrelated
commit-fixture failure that passed unchanged in isolation. Job 32794 installed
the verified release with a recovery copy; `/tmp/ca-pln244-release.log`. Plan:
`docs/exec-plans/completed/PLN-244-plan-card-review-lifetime.md`.

## PLN-243 expanded plan tasks (2026-09-05)

The user's screenshot identifies a display issue only; its plan text is not a
request to execute that work. The inline card now renders every task and wraps
complete entries instead of showing three tasks capped at two rows. Large
cards use ordinary transcript scrolling. The full reader and approval/execution
actions retain their existing behavior. Focused tests pass (38), full tests pass (3,231; six ignored), and format/lint
checks pass. Release PTY checks pass at 100x30 and 60x24. Job 85952 installed
the verified release with a recovery copy; `/tmp/ca-pln243-release.log`. Plan:
`docs/exec-plans/completed/PLN-243-expanded-plan-default.md`.

## BUG-242 Astra reasoning (2026-09-05)

The screenshot's unavailable status traced to a missing exact Astra contract.
Official OpenAI Responses routes now expose low/medium/high/xhigh/max with
medium default. Codex context is 272,000 per local model metadata; direct API
context is 1,050,000 per official docs. Explicit profiles still take precedence.
XHigh now serializes as xhigh while legacy x_high settings remain readable.
Three focused tests pass, including status rendering at 100x30 and 60x24,
picker selection, endpoint boundaries, profile precedence, and request JSON.
Formatting and strict Clippy pass. Broad tests: 3,228 passed, two unrelated
process/privilege failures, six ignored; both failed tests passed unchanged
in isolation. The failed broad evidence is retained in TESTING.md. Background
release/install job 44274 completed and verified (`/tmp/ca-bug242-release.log`).
Plan: `docs/exec-plans/completed/BUG-242-astra-reasoning.md`.


## PLN-241 plan review and execution (2026-09-04, complete and installed)

The approved user repair separates pending plan review from an active provider
turn. `src/plan.rs` owns exact plan/revision actions and the save-before-start
transition; Execute starts the current Agent with freshly built instructions
and tools. `/plan approve` remains approval-only. Empty graph grants do not
constrain the current agent; running graph work still prevents overlap.

Plans now have optional persisted Markdown bodies, an inline preview card,
and a full scrollable/copyable reader. Enter executes from an empty composer,
Ctrl+O views, typed feedback revises, and Esc dismisses without rejection.
Material edits revoke approval; questions no longer synthesize plans. Restored
sessions do not start automatically, and explicit execution loads full history
before saving. Markdown list boundaries no longer concatenate following prose.

Plan: `docs/exec-plans/completed/PLN-241-plan-review-execution.md`.
Failure-first approval and Markdown regressions reproduced. The final
focused suite passes 37 tests, including a loopback provider response that
edits a temporary file with empty graph grants; Markdown tests pass 20/20.
Five paired plan snapshots and three intentional existing list-spacing updates
were reviewed; render tests pass 224 with one unrelated manual check ignored.
Debug PTY walkthroughs at 100×30/60×24 and optimized release at 60×24 pass,
including proposal, reading, typed revision, dismissal/reopening, and execution.

The full workspace run recorded 3,373 passes, one status-line sandbox timeout,
and seven ignored tests. That unchanged test passed immediately in isolation;
every other target passed. Formatting, strict Clippy, and release checks pass.
The known protected-tree committed-oracle-baseline discrepancy remains; no
oracle/reference content or receipt changed. See `TESTING.md` for exact logs.

Managed background job 42112 successfully installed `/Users/aji/.local/bin/ca`.
Candidate/staged/installed SHA-256: `d9cbe4aac3947fb79eba4510ecafba0f53513f526045fba5c91ce41c6efa88e6`.
Signature, mode/ownership, version/help, and atomic replacement checks passed.
Recovery: `/Users/aji/.local/bin/.ca-recovery-20260904T042329Z.jRN6nS/ca.previous-0336bd2050b525d71959f885af40ccc603873f534a0821b85e8bdd3c53fd0192`.
Log/script/release walkthrough: `/tmp/ca-pln241-release.12xj4sgt/`.
User config/sessions were untouched; existing running sessions were left alone.

## TUI-240 stop and interrupt (2026-09-03, complete; not installed)

User-reported queued `stop` messages traced to busy Enter queueing and hidden
Ctrl+S hints. Enter now interrupts/sends, Alt+Enter explicitly queues, Esc stops
from the composer, and Ctrl+C stops through popups. Held/restored queue copy is
`On hold`; keyboard hints remain visible alongside transient status messages.

The deeper cancellation defect was an independently managed foreground command
surviving its tool waiter's abort. A foreground wait guard now cancels the owned
process group unless explicitly promoted. Shared turn cleanup invalidates late
stream/tool events, clears approvals/interactions/continuations, preserves the
draft/images and holds old queues across replacement completion. Interrupt
submissions retain ordinary prompt hooks and validation. No daemon/background
job policy, model catalog, configuration schema, or oracle implementation changed.

Plan: `docs/exec-plans/completed/TUI-240-stop-and-interrupt.md`.
Initial regressions failed as expected; final interrupt tests (14), final CA
suite (2,486 passed/5 ignored), full all-feature workspace (3,357/7 ignored),
paired snapshots, formatting, strict Clippy and release checks pass. Both debug
100-column and optimized-release 60-column real PTY walkthroughs passed using
only a disposable home and local synthetic responses. No inference or actual
user runtime state was touched.

Pipeline 32927 completed successfully. Release SHA-256:
`925756efc7abeda78d6ede26000ade8cc788cfdf5e3215ac2e711eb9f97ee69d`.
The latest explicit no-install instruction was honored for this delivery;
installed `/Users/aji/.local/bin/ca` remains
`0336bd2050b525d71959f885af40ccc603873f534a0821b85e8bdd3c53fd0192`.
Logs and fixture: `/tmp/ca-tui240.oawiMS/`. The existing protected-tree
oracle-baseline discrepancy stays separate; no protected content/receipt changed.

## BUG-239 native Codex catalog (2026-09-03, complete; not installed)

The approved repair replaces generic OpenAI API discovery only on the exact
ChatGPT-backed `openai-codex` route. Native discovery uses compatibility version
`0.153.0`, CA client identity, and the existing resolved bearer/matching account
header. Only `models[].slug` with `visibility: list` is used; remote instructions
and `supported_in_api` do not affect the picker. No installed Codex is required.

The manual `native_codex_live_catalog_matches_picker` check SUCCEEDED through
CA's existing auth flow: seven visible IDs, exactly matching the live picker:
`gpt-5.3-codex-spark`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.5`,
`gpt-5.6-luna`, `gpt-5.6-sol`, `gpt-5.6-terra`. No inference prompt or
config/session write occurred and no credentials were printed. Safe model IDs
and real wide/narrow render output are in `/tmp/ca-bug239-live.log`.

Both refresh paths now retain last successful native IDs on failure, without
merging documented fallback into success. Configured/current entries absent
from that catalog remain selectable and are labeled configured only. The
existing fallback remains for failures before any success. Empty success is
distinct from failure. No model profile/pricing/settings migration was added.

Plan: `docs/exec-plans/completed/BUG-239-native-codex-catalog.md`. Full all-feature
workspace tests (CA 2,473, experiment 118, codex-team 652, all integrations),
focused catalog/render tests, formatting, strict Clippy and release checks pass.
The user then explicitly said **do not install it this time**;
no installer was launched and the installed executable stays unchanged. The
standing rule remains for future tasks. Release candidate SHA-256:
`1ab5d08e0b5075f353828c613e24deeee2cc4849627c7109657f9b1ca5f292a4`.
The unrelated protected-tree oracle baseline discrepancy remains separate.
Installed `/Users/aji/.local/bin/ca` was verified unchanged at SHA-256
`0336bd2050b525d71959f885af40ccc603873f534a0821b85e8bdd3c53fd0192`.

## REL-238 standing background release/install rule (2026-09-03)

The user explicitly requested background release build and replacement after
completed repository changes, then immediate execution. `AGENTS.md` now records
that standing authorization and updates both older installation clauses so a
fresh request is not required per task. Applicable validation, recoverable
atomic replacement, artifact verification, and truthful background status remain
part of the workflow. No periodic scheduler or runtime config was introduced.

Tool-managed background session 57224 (PID 45741) completed successfully. It
rebuilt the all-feature workspace, required the exact previously tested BUG-237
candidate, preserved the prior executable, and atomically installed it at
`/Users/aji/.local/bin/ca`. Installed/candidate SHA-256:
`0336bd2050b525d71959f885af40ccc603873f534a0821b85e8bdd3c53fd0192`.
The recovery copy is
`/Users/aji/.local/bin/.ca-recovery-20260904T004835Z.WDKErD/ca.previous-1e91f11413c0d48cdfe6bbc9af2e1f9bee7c98e4d739e87d4865ea2fb2418f72`.
Both are regular single-link owner-501 mode-0755 executables; installed strict
signature, PATH, version/help, candidate help comparison, and diff checks pass.
Job output remains at `/tmp/ca-rel238.kXCjlb/job.log`; the completed plan records
the pre-install filename-check failure and correction. The prior full Rust
validation was reused for this unchanged source/artifact. The existing oracle
baseline discrepancy remains recorded; no protected file, receipt, secret,
configuration, session, or running CA process was changed by this installation.

## BUG-237 OpenAI Codex picker fallback (2026-09-03)

The screenshot's single OpenAI model was the configured `gpt-5.6-sol` fallback
after optional catalog discovery failed. `documented_chat_models` previously
excluded the ChatGPT-backed Codex route even though the exact provider contract
already supports GPT-5.6 Sol, Terra, and Luna. Both exact first-party OpenAI
routes now supply those three fallback IDs. Custom endpoints receive no new
built-ins, and the picker still labels failed discovery as fallback.

The catalog regression failed first with an empty list and now passes; refresh
coverage proves all three rows survive failure, and reviewed 100x30/60x24
snapshots show the selected current Sol row alongside Luna and Terra. This
does not repair the account catalog request or prove account access to each
model. Official OpenAI model documentation was checked, but no live provider
request, credential access, runtime configuration/session mutation, or binary
installation was performed.

All-feature workspace tests pass (CA 2,462/4 ignored, experiment 118/1 ignored,
codex-team 652/1 ignored, and all integration/protocol/doc-test targets).
Formatting, strict all-target/all-feature Clippy, release build, CLI smokes,
snapshot hygiene, and diff checks pass. The protected-tree verifier still
reports the pre-existing `oracles/` committed-baseline mismatch; protected
trees and receipts were not changed. BUG-237 is complete in source, with the
installed executable unchanged.

## SES-236 fresh session command (2026-09-03)

`/new` is now a distinct idle-only built-in. It captures the outgoing session,
saves it on a blocking worker, and only then switches to a fresh root UUID.
The new conversation retains the current model, reasoning setting, workspace,
and worktree attachment, but resets transcript, retry/checkpoint/queue state,
goal/workflow/work records, session grants, task skill loads, undo history, and
sitting metrics. Queued drafts remain saved on the old session. Save failure
keeps the active session and draft; paged histories are reconstructed against
their storage generation. Empty starts do not accumulate saved blank sessions,
and bare/`--no-session` stay in memory. `/clear` remains unchanged.

The former missing-command test failed first; ten focused cases now pass. A
disposable-home terminal walkthrough saved `old session prompt`, started a new
UUID with only `new session prompt`, verified both roots have no parent, and
reopened the old conversation through `/sessions`. No real credential or user
state was used. All 218 rendering tests, the complete serial workspace suite
(CA 2,431 passed/4 ignored), formatting, strict all-target Clippy, release build,
and CLI smokes pass. The first two full runs stopped at the expected extra
command in the discovery/palette snapshots; both were reviewed and corrected.
The protected-tree verifier still reports the existing `oracles/` discrepancy;
no protected tree or receipt was changed. The completed plan records evidence,
and disposable logs/home remain recoverable in `/Users/aji/.Trash/ca-ses236.bwusk8`.
No installation was requested or performed; the installed binary is unchanged.

## BUG-235 bounded pre-output recovery (2026-09-03)

Implementation now shares a hard two-POST route budget across response headers
and body-stream transport failures before usable output. Metadata remains
attempt-local until commit; nonempty decoded text/reasoning and tool data
disable replay even when a text-format normalizer is still buffering. HTTP
400, malformed/protocol responses, and post-output failures are not retried.

Ordinary validated submission after a failed terminal user-only message now
saves the source and creates a clean child immediately before that message.
Hooks and attachments finish first, source queues remain on the source, and
branch failure restores the draft without starting a provider request. Bare
and `--no-session` remain in-memory. Existing explicit retry, partial-output,
resolved-tool continuation, wire schemas, and session format are unchanged.

The two failure-first tests reproduced the missing body replay and unchanged
source ID. All 58 shared client tests, ten BUG-235 state/session cases, the
focused recovery render, the complete workspace suite, formatting, strict
all-target Clippy, release build, and snapshot hygiene pass. A disposable-home
loopback TUI walkthrough proved identical bounded replays, single-request HTTP
400 handling, clean child histories, preserved failed source histories, and
the new footer copy.

The protected-tree verifier remains blocked by a stable ignored `oracles/`
digest mismatch (`bd0de373…189e` expected, `8dca8724…e258` observed). Counts
match the committed receipt, direct Git inspection is empty, and `reference/`
still matches. BUG-235 changed neither protected tree nor the receipt; separate
authority is required to reconcile that workspace state. The exact server-side
HTTP 400 cause remains unproven. No provider error body or credential was
accessed, no real runtime or protected-tree bytes were changed, and no
installation was requested or performed.

## Reasoning-effort picker (2026-09-03)

`/reasoning` with no argument now opens a centered "Reasoning effort" picker
listing the selected model profile's configured `reasoning_efforts`; the current
effort is annotated and pre-selected (falling back to the profile default, then
index 0). `↑`/`↓`/`j`/`k`/Home/End navigate, Enter dispatches `SelectReasoning`
into the existing `set_reasoning_command`, and Esc closes without change.
`/reasoning <effort>` still sets directly, `Ctrl+T` cycles the configured order
with wraparound, and a model switch applies the profile's
`default_reasoning_effort`. The chosen effort flows through the existing
provider wire translation (`chat_reasoning_wire` / `responses_reasoning_effort`),
so no provider contract, request schema, or approval surface changed.

Validation: focused `reasoning_picker` coverage passes 3/3; full render suite
216/216 and full input suite 195/195; the complete `ca` target passes 2,400/4
ignored with CLI 38/38 and role-run 17/17. `cargo fmt --all -- --check`, strict
workspace all-target/all-feature Clippy, and the all-feature release build pass;
the two new wide/narrow picker snapshots are accepted with zero `.snap.new`.
`git diff --check` is clean. The new candidate is a strict-signed arm64 Mach-O
with SHA-256 `682c45492e8e7060148f7d853cc783d394af1a2f8768195871042cd19aa1df12`;
it was recoverably installed to `~/.local/bin/ca` at the user's direction (the
prior `60727a1c…ad566` binary is preserved as a timestamped full-SHA recovery
copy, and candidate/installed hashes, mode `0755`, ownership, strict signature,
path lookup, version, and help smoke all match). No provider request,
runtime-state mutation, oracle/reference edit, or credential access ran.

## Render-snapshot consolidation (2026-09-03)

At the user's direction the TUI render snapshots were halved: each wide (100-col)
+ narrow (60-col) pair is now one snapshot via the `wide_narrow` helper, so
`src/tui/snapshots/` went from 719 to 370 files with both bounded layouts still
asserted per state. The pre-existing `delegation_model_picker` and
`mimocode_plan_toast` snapshot pairs (render tests no longer present) were
removed, and eight intentionally single-variant snapshots (e.g. split-pane and
provider-auth width-specific cases) are unchanged. This is a test-only change;
render tests pass 216/216, the full `ca` target passes 2,400/4 ignored, and
strict Clippy, formatting, and the release build pass with the release binary
unchanged.

## BUG-234 DeepSeek DSML and Dream queue recovery (2026-09-02)

DeepSeek V4's exact documented ASCII/full-width DSML wrapper is now normalized
before anomaly detection and turn assembly into ordinary structured CA tool
events. Automatic activation is exact-contract only; validated tool-capable
Chat Completions profiles may opt in with `tool_call_format =
"deepseek_dsml"`. Native tool events and fenced examples retain prior behavior,
while malformed, incomplete, oversized, duplicate, prose-mixed,
native/DSML-mixed, and non-opted-in markup fail closed without entering the
transcript. Existing advertised-tool, approval, sandbox, unavailable-tool, and
execution policy is unchanged.

Writable learning entry points now recover only authentic owner-only V1
supervised intents whose sole defect is the historical UTF-8 character/byte
bound mismatch. Recovery validates the original filename, enrollment, digest,
shape, and safety policy; archives exact original bytes privately by digest;
and atomically replaces the intent with a same-ID, re-sanitized record and new
immutable digest. Unsafe, tampered, malformed, symlinked, colliding, and
non-legacy records remain untouched and rejected. Normal startup reports only
the bounded recovered count; validation never opened the user's learning store.

At the user's follow-up direction, paused AUT-231's formatting drift and two
strict-lint blockers were also cleared: the existing due-claim path is reached
from schedule tick/daemon execution, and a redundant public successful-tool
count was removed. AUT-231 remains paused. The all-feature matrix then exposed
and corrected a test-only one-in-sixteen capability-forging no-op; no production
lifecycle behavior changed.

Focused DSML, client, stream, config, provider, learning, typed-plan,
automation, schedule, and lifecycle tests pass. Both serial workspace matrices
pass after the deterministic replacement run: default CA 2,395/4 ignored;
all-feature CA 2,424/4 ignored; experiment 118/1 ignored; codex-team 652/1
ignored; MCP stdio 8/8; every other target passes. Repository formatting,
strict all-target/all-feature Clippy, the all-feature release, candidate
help/version and offline smokes, strict signature, diff/snapshot hygiene,
protected-tree verification, and orphan checks pass. Candidate SHA-256 is
`60727a1c249dbef2984f32b07a0a4c53dfc59321b5be64eb5067b2b5087ad566`.
No live provider, credential, real profile/session/learning state, or protected
tree was accessed or mutated.

## TUI-233 distinct sub-agent worker icons (2026-09-02)

Conversation-origin child rows now use a six-glyph colored worker palette in
place of the generic activity dot. The glyph is selected from the stable agent
UUID, so a worker retains its visual identity while running, awaiting approval,
finishing, and reappearing from its compact terminal transcript summary. Status
color, task/action text, focus/details, messaging, approval, cancellation, and
origin filtering are unchanged.

The failure-first focused render check observed the old generic bullets. The
repaired focused test and all 215 TUI render tests pass, including wide, narrow,
focused, and restored-summary snapshots plus exact glyph/color assertions. The
complete workspace matrix passes: CA 2,380/4 ignored, CA CLI 38/38, role-run
17/17, ca-watch 18/18, codex-team 652/1 ignored, MCP stdio 8/8, and all other
targets and doc tests. The all-feature workspace release build passes. The two
touched TUI files pass an isolated Rustfmt check. Repository-wide formatting
and strict Clippy remain blocked only by the pre-existing paused AUT-231
formatting drift and its unused `AutomationStore::claim_due` method and
`AutomationExecution::successful_tools` field.

The supplied Codex screenshot was treated only as visual guidance, not as
instructions or a parity oracle. No provider request, runtime configuration or
session mutation, protected-tree edit, installation, or Git-history change
occurred.

## BUG-232 bounded unavailable-tool recovery (2026-09-01)

The interactive provider loop now treats a first tool call outside the exact
per-request advertised set as a resolved, argument-free tool error and performs
one normal provider continuation. The hidden tool never enters approval,
hooks, registry dispatch, mechanistic mutation accounting, or execution.
Advertised calls in a mixed batch retain their ordinary order and behavior; a
second unavailable call in the same logical user turn retains the prior
fail-closed `/retry` boundary. Explicit workflows retain their existing
failure/retry semantics.

The screenshot's `search_files` call was not a missing implementation:
`search_files` remains a registered read-only built-in and DeepSeek's official
model profile supports tools. Default Main/Agent requests advertise it; only
tool-free Ask mode, a restrictive custom mode/workflow, or explicit CLI tool
selection can intentionally omit it. A provider-emitted name is a request to
use a tool, not authority to bypass that prepared set. The repair is therefore
provider-agnostic and does not widen mode, profile, CLI, workflow, approval,
sandbox, workspace, or network authority.

Five focused regressions pass, including mixed batches, repeat bounding,
logical-turn scoping, and proof that a rejected mutating call cannot execute.
The complete serial workspace suite and a normal `cargo build -p ca` pass. The
normal build reports only the two pre-existing AUT-231 dead-code warnings and
the repository's known non-fatal macOS compact-unwind linker warning.
Formatting and strict Clippy remain blocked by pre-existing paused AUT-231
edits: formatting differs in
`src/automation.rs`, `src/schedule.rs`, `src/session.rs`, and the existing
module order in `src/main.rs`; strict Clippy reports only unused AUT-231 items
`AutomationStore::claim_due` and `AutomationExecution::successful_tools`.
`git diff --check`, snapshot hygiene, and direct oracle/reference status pass.
At the user's follow-up request, `cargo clean --release` removed the imported
11,222-file/5.7-GiB generated cache and a fresh normal parallel
`cargo build --workspace --release --all-features` passed in 3m03s. The release
is an arm64 Mach-O, passes strict code-signature verification plus help/version
smokes, has no remaining quarantine attribute, and has SHA-256
`6e97b2388c413a649bada43952ffe2791c545a8d0d8621553a698876f5bd1eb8`.
No provider call, runtime-state mutation, oracle/reference edit, installation,
or secret access occurred.

## FreeBSD native build and install restored (2026-08-31)

The `ca` binary now builds and installs natively on FreeBSD 15.1 amd64. The
long-standing `crates/codex-team` FreeBSD `setpriority` i32/u32 type error
(previously recorded as pre-existing in the SEC-154/SEC-155 records below) is
fixed: `libc::setpriority` takes `id_t` (u32) for `who` on macOS/Linux but
`c_int` (i32) on FreeBSD, so the `child.id()` argument is narrowed to `as i32`
under `#[cfg(target_os = "freebsd")]` only. No macOS/Linux behavior changed.

`cargo build --release --locked` succeeds on the FreeBSD 15.1-RELEASE-p2 host
(packaged cargo/rustc 1.96.1; the `rust-toolchain.toml` 1.97.1 pin is ignored
without rustup). The binary was installed to `/home/aji/bin/ca`, replacing the
2026-08-15 build (recoverable backup at `~/bin/ca.bak-20260831-120059`).
Installed SHA-256 matches the candidate
`103377fb89c0855a3575068b50dee79d82c87f2c18327a4f78fb3bddfed5c793`;
`--version`, `--help`, and `ldd` all resolve cleanly.

## PRT-230 protected-tree Finder metadata exclusion (2026-08-29)

At the user's direction, exact `.DS_Store` components are now excluded from
protected-tree traversal before entry counts, type checks, and hashes. The
semantic change is explicit as `sha256-merkle-v2-ds-store-excluded`; similarly
named paths such as `.DS_Store.keep` remain protected, and all no-follow,
hardlink, special-file, bounds, double-scan, revision, and digest checks remain.
No byte under `oracles/` or `reference/` changed.

The failure-first unit test observed four oracle entries where two were
expected. The repaired unit suite passes 6/6 and the real CLI suite passes 3/3,
including root/nested Finder changes and a protected similar-name control. The
full codex-team package passes 652/1 ignored. An intervening workspace run
reproduced REL-160's known one-versus-two clean-filter invocation assertion;
that test now accepts Git's valid one-or-two staging calls while still failing
on a third and verifying exact committed bytes, empty index, and HEAD CAS. It
passes ten repeated focused runs.

Both replacement workspace matrices are clean: default CA 2,371/4 ignored and
codex-team 652/1 ignored; all-feature CA 2,400/4 ignored, experiment 118/1
ignored, and codex-team 652/1 ignored; MCP stdio is 8/8 in both. Formatting,
strict all-feature Clippy, release, candidate signature, diff/snapshot, and
direct oracle/reference checks pass.

The v2 manifest is exact revision/digest-bound with digest
`eeb36e1d625884a375ffa48fe244491902d198b08a9054f7ce572a16ebceb4dc`.
After explicit user authority, manifest-only commit
`25c936f37e545cb0b4ccfdbbcb397d216a497e83` establishes HEAD trust. The
protected verifier passes, and the installed binary matches the validated
candidate SHA-256
`3ed7b2347f22671d2d45e25fd3d640309ecf6a734378e14fbdbff610409d0392`.

## REL-229 integrated validation and installation (2026-08-29)

Every stable ORC-218 child is implemented and its plan is archived. The two
required serial workspace matrices pass: the default matrix includes 2,371 CA
tests plus four documented ignores, and the all-target/all-feature matrix
includes 2,400 CA tests plus four ignores, the 118-test reasoning experiment
with one ignore, codex-team's 652 tests with one ignore, and the complete 8/8
MCP stdio suite. Formatting, strict workspace all-target/all-feature Clippy,
the all-feature release build, help/version and new-command/schema smokes,
strict code-signature verification, `git diff --check`, snapshot hygiene, and
orphan-process checks pass. The candidate is a regular mode-0755 arm64
executable with SHA-256
`3ed7b2347f22671d2d45e25fd3d640309ecf6a734378e14fbdbff610409d0392`.

The first default-matrix attempt exposed an existing same-process
`LearningLedger` race: independent handles used independent mutexes, allowing
a lock-file contender to observe removal between `AlreadyExists` and metadata
inspection. Handles for one canonical ledger path now share a weakly retained
process lane, and disappearance during collision inspection retries. The new
lock-sharing regression, the prior concurrent-writer regression, and the clean
complete rerun pass. Final strict Clippy also found two test-only construction
styles in pane regressions; their mechanical cleanup was followed by clean
format, Clippy, release, and both focused pane tests.

The initial protected stop was localized to ignored Finder metadata without
modifying or restoring the oracle. PRT-230 now excludes only exact `.DS_Store`
components under algorithm `sha256-merkle-v2-ds-store-excluded`; similarly
named files remain protected. The user separately authorized the exact
manifest-only commit, after which the HEAD-trusted verifier passed. A
timestamped full-SHA recovery copy was created and the release candidate was
atomically installed; candidate, staged, and installed hashes match, and the
strict signature plus representative local smokes pass.

## PRV-228 Responses WebSocket deferral (2026-08-29)

PRV-228 is complete as a documentation-only deferral. Official OpenAI
documentation fetched on 2026-08-29 documents ordinary stable Responses
creation as `POST /responses`, while WebSocket client/server events and input
injection remain under the explicitly Beta Responses resource with `beta_*`
schema identities. This supports the inference that no stable wire contract is
currently documented; it is not evidence that the Beta protocol is stable.

`docs/RESPONSES_WEBSOCKET_READINESS.md` records the exact future stable
endpoint/authentication, ordering, tool continuation, cancellation,
reconnect/replay, usage/billing, error/bounds, and no-replay evidence required
to reopen work under separate authorization. No provider source, request,
dependency, feature, CLI/config/session surface, runtime state, protected tree,
oracle/reference file, or installation changed.

## IMP-227 safe Claude setup import (2026-08-29)

IMP-227 is complete. `ca import setup claude` and `/import setup claude`
preview only bounded Claude instructions, rules, commands, and CA-valid skill
packages into native user/project roots. The versioned preview binds canonical
source/target roots, scope, every source and rendered target digest, skipped
categories, collision state, and byte counts. Sensitive settings, credentials,
hooks, MCP, permissions, shell, and account files are reported by category but
their contents are never retained or imported.

Apply reproduces the exact preview, refuses collisions/drift/bare profiles,
stages privately, publishes without overwrite, synchronizes parents, and rolls
back only owned files. The TUI worker is Cancel-first, scrollable, and refreshes
native registries only on success. Focused coverage passes 10/10 plus slash and
command checks; the complete CA target passes 2,370 tests with four documented
ignores, formatting, strict Clippy, release, help/schema, diff/snapshot hygiene,
and oracle/reference inspection. No real import, provider request, runtime
state, protected-tree edit, or installation occurred.

## EDT-226 stable editor context and hash-anchored editing (2026-08-29)

EDT-226 is complete. Stable ACP v1 advertises embedded context and accepts only
bounded text embedded resources or workspace-contained regular `file:` links,
with visible per-prompt provenance and strict count, item, aggregate, media,
range, traversal, binary, and symlink rejection before provider construction.
No editor state is independently discovered or persisted.

`read_file.include_line_hashes` adds optional eight-hex content anchors without
changing default output. `hashline_edit` rereads the exact file, requires one
unique ordered anchor range, and then uses the existing approval,
workspace/sandbox, undo, mutation-receipt, LSP, loop-guard, activity, and
artifact-ledger boundaries. Focused ACP/SDK and anchored-edit suites pass; the
complete serial CA target passes 2,359 tests with four documented ignores,
plus formatting, strict Clippy, and release. No provider request, runtime state,
protected subtree, oracle/reference file, or installation was touched.

## PAN-225 bounded split-pane sessions (2026-08-29)

PAN-225 is complete. The interactive TUI owns a process-only layout tree with
at most four horizontal/vertical leaves; each leaf materializes a complete
isolated `AppState` with its own session, conversation, model/mode/workspace,
composer, queue, approvals, work/goal/workflow state, job owner, stream
generation, agent manager, MCP manager, and LSP manager. Only declared
configuration/catalog/diagnostic/terminal services synchronize across pane
switches. Provider/tool/goal events are tagged with their originating pane
before the existing generation check, so background output cannot enter the
focused session. Existing-session split loads run off the terminal loop.

`/split horizontal|vertical [new|SESSION]`, `/pane next|prev|close|list`, mouse
focus, and configurable `Ctrl+X H/V/N/P/Q` controls are available. Only the
focused pane accepts composer input. Background mutating/network work becomes
an explicit pending approval; completion is shown by a pane badge and follows
the existing focus-aware notification policy. Closing active work is
Cancel-first, stops only the pane's turn/jobs/managers, and never deletes its
saved session. Process shutdown reaps every pane's owned work.

The 11 pane-focused tests, background-approval regression, and reviewed
vertical/horizontal/four-pane snapshots pass. The complete serial CA binary
suite passes 2,353 tests with four documented ignores, along with formatting,
strict CA all-feature Clippy, and the all-feature release build. The existing
macOS oversized `__eh_frame` warning remains non-fatal. No provider, runtime
profile/session state, protected subtree, oracle/reference file, or installed
binary was touched.

## ART-224 session artifact ledger (2026-08-29)

ART-224 is complete. Saved sessions have a defaulted, versioned, 512-entry
metadata-only artifact ledger. Successful tool/project-init/undo/redo
mutations are recorded by default; successful `read_file` calls are recorded
only with `sessions.track_reads = true`. Entries retain root UUID, relative
path, access/count, message boundary, timestamps, size, and content/receipt
digest, never file bytes or raw root paths. Full forks retain entries through
their boundary; historical branches/rewinds discard later entries, and
portable exports omit the ledger.

`/artifacts` lists and searches entries, inspects metadata, copies relative
paths, views only unchanged contained UTF-8 files, and untracks metadata
without touching files. Viewing rechecks the active root, every path
component, regular-file state, one-MiB bound, size, and digest. Focused artifact,
session, portability, configuration, mutation, command, and reviewed
wide/narrow snapshot tests pass. The corrected complete CA binary suite passes
2,341 tests with four documented ignores; formatting, strict workspace
all-target/all-feature Clippy, and the all-feature release build pass. The
first broader workspace run exposed only two deterministic command fixture
updates, which were corrected before the clean CA rerun. No provider,
profile/session runtime state, protected subtree, oracle/reference file, or
installed binary was touched.

## SYS-223 active-work lifecycle (2026-08-29)

SYS-223 is complete. Saved `agent.keep_awake` defaults to true, while
`--keep-awake` and `--no-keep-awake` are process-only mutually exclusive launch
overrides with provenance. A nonblocking actor reference-counts interactive
model turns and model-originated jobs, debounces idle release, and owns one
fixed no-shell idle-only helper: `/usr/bin/caffeinate -i -w <ca-pid>` on macOS,
or `/usr/bin/systemd-inhibit --what=idle ... /bin/sleep infinity` on Linux.
Unsupported/missing helpers degrade to a visible safe no-op. Helpers run in an
owned process group; normal shutdown terminates/reaps them, macOS binds helper
lifetime to CA's PID, and Linux adds a parent-death signal. Display sleep is
never inhibited and direct/internal jobs acquire no lease.

`await_job` now accepts `stop` and `grace_secs`. Stop conflicts with
`timeout_secs`; grace is 0–300 seconds and requires stop. CA checks the existing
job owner, reserves exactly-once completion, sends process-group SIGTERM,
preserves final bounded output, and escalates to SIGKILL after the exact grace.
`kill_job` remains the immediate cancellation API. Keep-awake 3/3, jobs 29/29,
job tools 30/30, configuration 101/101, CLI, and provenance tests pass, as do
formatting, strict Clippy, release, and hygiene gates. No provider, runtime
state, protected tree, or installation was touched.

## WFL-222 audited built-in workflows (2026-08-29)

WFL-222 is complete. CA now constructs immutable, canonically digested
`builtin:deep-research`, `builtin:fact-check`, and
`builtin:research-experiment` definitions inside `src/workflow.rs`, validates
them through the existing declarative engine, and retains their bounded
definition in saved run state for safe resume. User workflow files cannot
claim built-in identities. `/workflow templates`, `inspect builtin:<id>`, and
`tree [builtin:<id>]` expose routes, dependencies, attempts, evidence gates,
budgets, pause reasons, and terminal state without prompts or output bodies.

Deep research keeps network tools behind ordinary approval and optionally
writes only to one contained Markdown path. Fact check requires exactly three
pairwise-distinct configured routes; each juror receives a fresh request
projection with no canonical transcript, prior juror verdict, memory recall,
attachment, or pending diff context. The final report consumes all three
bounded outputs. Research experiments require a clean attached CA-owned
worktree, 1–10 iterations, bounded editable globs, and an exact command plus
network-flag digest. Host matching rejects a widened evaluator invocation
before hooks, approval, or execution; transfer/merge remains separate.

Forty workflow-path tests, 16 core workflow tests, focused isolation and exact
evaluation regressions, and reviewed wide/narrow definition/live-tree
snapshots pass. Formatting, strict all-target/all-feature Clippy, all-feature
release, diff/snapshot hygiene, and direct oracle/reference inspection pass.
No provider, profile, protected subtree, or installed binary was touched.

## REV-221 convergent review and acceptance packets (2026-08-29)

REV-221 is complete. Opt-in convergence resolves exactly two explicit,
distinct, tool-compatible provider/model routes before any request. Both
reviewers receive one byte-identical objective in isolated fresh read-only
contexts and start concurrently. One shared budget caps physical requests,
retained provider output, and exact known cost; missing usage on a priced route
fails closed, while unpriced routes require an explicit one-run
acknowledgment. Both reviewers must return an empty valid findings report.
Headless explicit repair reuses the ordinary authority-preserving agent loop
and runs fresh pairs for at most three rounds/six reviewer invocations. The TUI
runs one pair per explicit command and leaves repair to an ordinary user-
authorized agent turn before another fresh pair.

`ca.acceptance.v1` packets are private, immutable, content-addressed, bounded,
and revision-bound. They retain request and source/workspace/session digests,
relative changed paths, valid AGT-157 pass-receipt digests, hashed host work
events, reviewer route/verdict/usage metadata, unresolved-risk labels, and no
raw prompt, reviewer summary, hidden reasoning, tool output, credential,
provider payload, or absolute path. Only revalidated host evidence is labelled
`verified`; reviewer claims are inferred, contradicted, or unverified. `/acceptance
create|latest|list|export` reports stale revisions truthfully and never
overwrites an existing packet/export.

Focused convergence 3/3, acceptance 4/4, review 5/5, headless pass/repair 2/2,
configuration 2/2, and CLI parser coverage pass. Formatting, strict workspace
all-target/all-feature Clippy, the all-feature release build, diff/snapshot
hygiene, and direct oracle/reference inspection pass. No real reviewer route,
profile/session fixture outside disposable roots, protected subtree, or
installed binary was touched.

## RES-220 streaming anomaly protection (2026-08-29)

RES-220 is complete. Every concrete provider client now applies one bounded,
provider-neutral guard after wire decoding and before decoded events enter the
canonical turn assembler. Independent text/reasoning rolling state plus the
complete advertised-tool name set consumes at most 64 KiB; a route whose tool
names cannot fit fails before network I/O. The guard stops three repeated
bounded prose spans, four near-identical consecutive prose paragraphs, and a
primarily unfenced JSON/XML pseudo-tool envelope naming an advertised tool.
Real provider tool events disable pseudo-tool classification. Fences, ordinary
repeated code, short repetition, malformed envelopes, non-advertised names, and
ordinary prose pass unchanged.

On detection CA forwards no anomalous terminal event, drops the live response,
emits a static `StreamAnomalyKind`, and retains all prior decoded text/reasoning
through existing failure handling. Because usable output was already observed,
fallback and replay never start automatically; the existing branch-based
`/retry` remains explicit. Byte-at-a-time Unicode loopback coverage proves the
third span is stopped, two earlier spans survive, the connection closes, and a
two-attempt policy issues one request. Focused guard 10/10, stream 17/17,
provider 34/34, client 47/47, fallback 4/4, probe 8/8, service 25/25, and the
shared TUI partial-preservation regression pass. Formatting, strict all-feature
Clippy, all-feature release, diff/snapshot hygiene, and direct oracle/reference
inspection pass. No provider traffic, user runtime mutation, protected-tree
edit, public schema change, or installation occurred.

## DIA-219 support bundles and configuration provenance (2026-08-29)

DIA-219 is complete. `ca config explain` and `/config explain` expose only a
versioned selector, source class, and present/absent projection across built-in,
profile, process, environment-reference, Keychain-reference, and derived
sources. They never retain effective values or reference identifiers. `ca
support-bundle` and Cancel-first `/support-bundle` create a private, bounded,
hash-manifested `ca.support-bundle.v1` archive from safe typed doctor, audit,
runtime-health, metadata-log, and provenance records. Raw paths, prompts,
transcripts, system instructions, tool content, provider payloads, URLs,
environment values, and credentials are excluded.

Archive publication is mode-0600, regular-file, ownership, hardlink, symlink,
collision, drift, and 8 MiB-cap checked. A fully synchronized staging file is
atomically linked without overwrite, followed by parent synchronization;
failure removes only writer-owned staging. Focused provenance (3/3), bundle
(9/9), runtime-log, shell, slash, and reviewed wide/narrow render tests pass.
The default workspace matrix is clean after reviewing the expected one-line
command-count snapshot and rerunning the complete CA target (2,289 passed,
four ignored). Formatting, strict all-feature Clippy, all-feature release,
help/version/schema smokes, diff hygiene, zero `.snap.new`, and direct
oracle/reference inspection pass. No profile archive, provider request,
session/configuration mutation, protected-tree edit, or installation occurred.

## AGT-157 host-owned verification and budgets (2026-08-29)

AGT-157 is complete under its original ownership and unblocks the dependent
ORC-218 review/workflow work. Agent graph completion now requires a current
host-created pass receipt from deterministic contained artifact predicates,
exact closed verifier commands, or a user action against an explicitly
declared HumanReview gate. Assistant summaries, legacy missing contracts,
source/workspace drift, stale/tampered receipts, and non-pass dispositions
cannot release dependencies. Writable primary/worktree lanes serialize and
cross-lane dependencies require an explicit handoff.

One monotonic finite graph budget now covers active time, physical requests,
total/output tokens, exact priced cost or explicit capped unpriced authority,
tool calls/failures, nested children, retained output, retries, and restored
usage. Provider retries are disabled inside graph accounting. Focused suites,
both complete serial workspace matrices, formatting, strict all-feature
Clippy, all-feature release, diff/snapshot hygiene, and direct
oracle/reference inspection pass. No real provider, real worktree, runtime
profile/session, or installed-binary mutation occurred.

## TST-217 broker cache-publication repair (2026-08-29)

The authorized protected-subtree repair is complete. Broker atomic-publication
temporary names now combine process ID, a captured nanosecond timestamp, and a
dedicated monotonic sequence. Writers reserve private mode-0600 files with
`create_new`, retry only `AlreadyExists` up to 16 times, never remove a foreign
collision, and clean up only their own reserved path. Existing atomic rename,
file and parent synchronization, tamper enforcement, coalescing, receipts, and
last-writer-wins cache semantics are unchanged.

Fixed-clock concurrency, collision, exhaustion, complete-payload, and cleanup
tests pass with all 11 broker tests, three repeated shared-query regressions,
36 MCP unit tests, and the eight-test stdio suite. Both serial workspace
matrices, formatting, strict all-feature Clippy, and the all-feature release
build pass. The exact pre-edit dirty inventory is preserved after removing the
completed nested queue entry; no oracle/reference, runtime/profile, provider,
or installed-binary mutation occurred.

## TST-216 deterministic scheduler test repair (2026-08-29)

The post-CUR authorized repair is complete. The protected codex-team
five-client evidence fixture no longer assumes all clients attach within an
eight-second sleep. A disposable ignored marker barrier holds the build, the
test requires the active job to report exactly five consumers, and a second
phase keeps the child alive while four clients detach and the last one cancels.
Production broker behavior is byte-unchanged. The repaired case passed four
focused repetitions and every broad invocation. The eight-test SDK suite has a
clean pass; the package matrix separately reproduced the already documented
shared-query cache-publication `EEXIST` race, which passed in isolation and is
not treated as evidence against this repair. TST-217 subsequently repaired that
publication race and both complete workspace matrices now pass.

## CUR-204 complete missing local Cursor features (2026-08-29)

CUR-204 is complete. The sequential local-feature program covers named
multi-root workspaces; conditional rules and native command management;
background promotion and host-owned completion; verified plugin UX and
ephemeral plugin directories; approved direnv and shell integration; one-shot
mode/sandbox/trust/worktree controls; safe runtime logs and watchdogs; copy,
Mermaid, link, focus, theme, and title polish; stable ACP v1 stdio; bounded local
automation flags and repository schemas; and one final integrated release.
Hosted accounts, subscriptions, cloud agents, telemetry, billing, arbitrary
remote marketplaces, hosted PR discovery, and provider checks are excluded.

The exact pre-program working tree contains 556 tracked/indexed changes and 254
untracked paths. Canonical porcelain SHA-256 is
`02fd90e56932bc57a3de507368c86d060628a48eb8ad40a3d3a83fbb400a2d29`; a
lossless gzip/base64 copy is stored beside the umbrella plan for REL-215's final
path-attribution audit. Direct staged/unstaged status and diffs for `oracles/`
and `reference/` were empty. The existing active plans are AGT-157, INT-151,
LRN-150, LRN-166, LRN-170, REL-156, and SEC-158. Their primary overlap risks
are agent/job completion, central TUI and session code, release/Cargo plumbing,
and service/approval/sandbox boundaries. CUR-204 adds defaulted or process-only
behavior and must not rewrite those plans' owned semantics.

WSP-205 is complete. The profile-private `ca.workspace-catalog.v1` store is
atomic, bounded, exact-mode `0600`, rejects missing/non-directory/duplicate or
symlink-aliased roots, and preserves saved primary/additional order. Fresh
launches accept `--workspace` plus repeatable `--add-dir`; exact catalog names
precede ambiguous relative paths, while `./` and absolute selectors force path
resolution. Resume selectors reject both flags, and bare profiles expose only
process-local path roots. `/save-workspace`, `/load-workspace`, and the expanded
`/workspace` command use the same catalog. Commands, instructions/rules,
skills, MCP, LSP, attachments, sandboxing, provider construction, system
prompts, and restored sessions now share one `WorkspaceContext`.

KNL-206 is also complete. User rules under `~/.ca/rules` and project rules from
every workspace root now use bounded legacy/always/glob/manual semantics. Glob
rules activate only from explicit attachments, `@` mentions, or a host-supplied
path set; ordinary prompt prose and model requests have no activation path.
Manual rule IDs are saved through a defaulted, bounded session field. `/rule`
and `/command` provide list/filter/view/create/edit/delete/reload flows. Native
user/project writes require an exact digest-bound preview, reject source drift,
symlinks, hardlinks, escapes, and built-in collisions, and write atomically;
compatibility, generated, plugin, skill, and auto-distilled command sources are
visible but read-only.

JOB-207 is complete. Jobs distinguish model-tool, direct-shell, and internal
origins and track pending, explicit-await, and automatic delivery separately.
Owned foreground jobs promote without restart, output loss, retained timeout,
or changed cancellation authority. TUI Ctrl+B requires two presses for the
same job within 1.5 seconds. Idle model completions batch into one bounded
host-authored system message and one continuation; direct `!&` jobs never do.
Headless runs wait up to 1,800 seconds and spend normal `max_turns`, while
`--single-turn` cancels remaining model jobs with a structured outcome.

PLG-208 is complete. Repeatable `--plugin-dir` roots must be bounded canonical
directories containing exactly one `plugin.toml` or `plugin.json`; they reuse
the declarative manifest, digest, resource, and collision checks, are enabled
only for the current process, never touch the registry, and are unavailable in
bare profiles. `/plugin` and `/plugins` browse, search, inspect, add, enable,
disable, and remove local bundles and operate the existing signed local catalog
trust, sync, install, update, and rollback flow. Previews expose only safe
metadata and exact confirmations; no arbitrary URL or hosted discovery exists.

ENV-209 is complete. `RunEnvironment` is a process-local immutable snapshot
whose Debug form exposes counts only and whose values never enter persistence or
model context. Config/provider credentials and authorized child surfaces now use
that same overlay. `.envrc` is only detected until explicit `--direnv` or
digest-confirmed `/direnv` authorization in a trusted workspace; evaluation is
bounded, sandbox-required, network-denied, timeout-safe, and bound to the
canonical primary workspace. TUI re-evaluation rebuilds environment-sensitive
registries and rolls back on failure. CA-native zsh integration prints,
installs, and removes one exact managed block using symlink/regular-file checks,
atomic writes, and timestamped SHA-labeled backups.

HED-210 is complete. Fresh launches accept `--mode plan|ask`/`--plan`, a
process-effective `--sandbox`, and headless-only `--trust`; every fresh-run
authority flag conflicts with session resume, weaker sandbox modes require
explicit process trust, and nothing rewrites persistent trust or sandbox
configuration. Ask mode advertises no tools and carries the same explicit
guidance in preview and provider prompts. `--worktree[=<name>]` creates a
registered tree before workspace registries and provider construction, replaces
only the primary root, and preserves supplemental roots. Native typed
`.ca/worktrees.toml` setup wins over bounded `.cursor/worktrees.json`
compatibility; the preview digest binds source bytes, repository/base/worktree
identity, ordered steps, cwd, sandbox, and network. Headless setup runs only for
the exact digest, while the TUI uses an asynchronous Cancel-first preview.
Skipped or failed setup keeps the registered tree, and `ca repo worktree-setup`
is a local preview-only command.

WSP through ENV focused tests, deterministic managers and timing helpers, the
default serial workspace matrix, formatting, strict all-feature Clippy,
all-feature release builds, diff/snapshot hygiene, and protected-tree
verification pass. An intermediate KNL run found and corrected the legacy
`Rule: name` heading contract. Two JOB workspace attempts exposed different
unchanged `codex-team` timing failures; both passed immediately in isolation,
and the third full serial workspace run passed completely. PLG's first full
run hit one unchanged active agent-lifecycle capability-handle timing failure;
the exact test and next complete matrix passed. ENV's final clean run passed CA
2,198 tests plus every workspace/integration target, all 203 render snapshots,
strict lint, release, smoke, and hygiene checks. HED's completion run passed CA
2,216 tests plus every workspace/integration target, six reviewed setup
snapshots, strict lint, release, smokes, and protected-tree verification.
OBS-211 is complete. Ordinary agent launches now write only typed metadata to
five bounded private rotating files through a background writer; bare and
administrative no-state runs remain untouched. `/logs` exposes typed bounded
filters, while a central measured draw boundary and deterministic heartbeat,
render, flicker, restoration, and memory watchdogs feed safe operational
events. Unclean exits require persisted high-memory evidence before receiving
that classification. Hidden `/debug render` exposes redraw, dirty-region,
frame, failure, rate, and repaint-highlight diagnostics. Focused tests, four
reviewed snapshots, the complete serial workspace matrix, formatting, strict
all-feature Clippy, release build/smokes, diff/snapshot hygiene, exact baseline
comparison, and protected-tree verification pass. The first matrix's expected
command-count snapshot drift was reviewed; its unrelated five-client broker
race passed alone and on the complete rerun. TUI-212 is complete. CA now has a
searchable sanitized message/part copy picker, width-bound off-loop Mermaid
rendering with code fallback, destination-preserving Markdown and fail-closed
OSC 8 links, focus-gated notifications, bounded OSC 11 auto-theme detection,
and optional restoring terminal-title indicators. The corrected default and
all-feature workspace matrices, strict lint, release build, snapshot/diff
hygiene, exact protected-baseline comparison, and verifier pass. ACP-213 is
complete. `ca acp` is stable ACP v1 newline JSON-RPC stdio through the exact
official Rust SDK 2.0.0 dependency. It reuses CA's local provider, session,
workspace, tool, approval, and cleanup boundaries; advertises agent/plan/ask;
streams text, thought, tool, plan, command, usage, and mode updates; and fails
mutating/network work closed when the client cannot answer permissions. Client
MCP/filesystem/terminal, hosted auth/logout, WebSocket/TCP, privileged secrets,
and experimental v2 are absent. The SDK's transitive
`serde_json/preserve_order` feature exposed five deterministic byte
assumptions; explicit sorted compatibility serialization retains existing CA
outputs, and protected codex-team source remains untouched. Nine ACP tests, 24
service tests, both complete workspace matrices, strict lint, release build,
credentialless v1 stdio initialization, diff/snapshot hygiene, exact protected
comparison, and the verifier pass. CLI-214 implementation is complete. Strict
pre-initialization SemVer gating, bounded ephemeral session-export history,
collision-checked caller UUIDs, automatic workspace-context exclusion, bounded
host Git context, key-only redacted structured environment events, and local
versioned `ca repo info|status` surfaces are in place. Workspace skills remain
available to exact user activation while being absent from automatic catalogs,
suggestions, and generated commands; Git context cannot escape upward from the
selected primary workspace. Focused tests, the default workspace matrix,
formatting, strict all-feature Clippy, the all-feature release build, and local
smokes pass. Two complete all-feature matrix attempts reached different
pre-existing timing races in unchanged protected `codex-team` tests; every CA
and feature-gated experiment target passed, and isolated protected reruns passed.
The third exact complete all-feature matrix passed every target, including the
previously timing-sensitive protected cases. CLI-214 is complete. REL-215
accounted for every initial dirty-tree record plus exactly 58 CUR-owned
additions, reconciled the harness, and passed formatting, strict lint, release,
signature, diff/snapshot, schema-smoke, and protected-tree gates. A repeated
default matrix later hit the same protected cancellation timing race after all
CA/library tests passed; the exact case passed immediately alone. Two flawed
install-smoke transactions restored the old binary exactly before the corrected
transaction passed. Candidate, staged, and installed SHA-256 are
`30660c4e21598363feb3c99d11f271716f84e6f3e95907d07f633faa0af83111`.
The prior binary is preserved at the timestamped full-SHA recovery path recorded
by REL-215. No paid or hosted request ran, and no user configuration/session,
oracle, reference, or protected-tree source changed.

## HRS-203 Hermes delegation and learning completion (2026-08-28)

CA now closes the ten Hermes-informed gaps left after HRS-202. Child requests
can declare a bounded local JSON schema; the final value is validated and gets
exactly one authority-preserving repair turn before truthful failure. Atomic
batch admission validates every member before spawning, records one stable
batch identity, and returns terminal results in submission order. A global
pause is serialized with every individual, nested, batch, and lifecycle spawn
admission while leaving existing children controllable.

Detached terminal results re-enter only their originating conversation as
host-owned lifecycle data and can request one idle continuation without
impersonating user input. Optional child isolation uses the existing owned
worktree/branch store and reports its exact receipt. The versioned in-process
plugin lifecycle facade issues random origin-bound capability handles for
idempotent launch, status, wait, cancel, result, and reconnect with bounded
terminal retention. Oversized results spill through a private bounded store
with a SHA-256 identity; timeout receipts include current action, recent output,
events, and attributed background jobs. Admitted background jobs survive child
finalization under origin/turn attribution and remain awaitable or cancellable.

The new curator provides exact, journaled archive, restore, consolidate, pin,
sync, and adopt operations. Prepared transactions roll back and moved
transactions finish during recovery; foreign skill sources stay read-only and
automatic policy remains disabled unless explicitly enabled. `/learning graph`
now combines current skill metadata with bounded in-scope native-memory nodes
and evidence edges, displaying only scope, kind, term count, and short digest,
never full memory bodies. Compatibility defaults remain shared workspace, no
child output contract, spawning enabled, and curator automation disabled.

Focused agent-manager, lifecycle, registry, curator, learning-graph, and
detached-completion tests pass. The clean default workspace matrix passes. Two
all-target/all-feature runs covered every HRS-203 path: the first had one
unchanged voice descendant timing miss and the second passed that entire CA
target; the second had one unchanged five-client broker cache-pruning race,
while that complete integration target passed in the first run and the exact
case passed immediately alone. Formatting, strict all-feature Clippy,
all-feature release build, release help/version and strict signature, diff and
snapshot hygiene, and committed protected-tree verification pass. The release
candidate SHA-256 is
`7e041539d00f57504645e3b911daeeb0509286f31a167fd054c8fb2377a71a1d`.
No live profile, credentials, provider, session, installation, or oracle was
read or mutated.

## HRS-202 Hermes-informed learning and agent lifecycle (2026-08-28)

CA now exposes the pertinent Hermes behaviors that were absent without
duplicating its existing learning ledger or authority model. Portable skills
can declare bounded normalized categories and related-skill names at the top
level or under `metadata.hermes`; duplicate, self-referential, malformed, or
oversized declarations fail closed. `/learning graph` combines current-digest
usage and outcomes with declared and successful co-use relationships, reports
unresolved targets, ignores stale attribution, and remains metadata-only and
bounded.

Child steering is now lossless at CA's lifecycle boundary. Every accepted
message receives a process-local ID and stays pending until the provider runner
delivers it at a request boundary. Approval waiting retains messages, and
completion is serialized with acceptance so a final-boundary message appears
as bounded sanitized `missed_steer` rather than disappearing. Terminal children
reject new steering. `await_agent` supports an optional bounded timeout that
returns the latest live snapshot, while completed and failed summaries expose
a stable SHA-256 digest.

These changes add no provider or profile behavior, persistence migration,
automatic skill mutation, child authority, transcript retention, or oracle
modification. CA's existing exact grants, immutable workspace snapshots,
approval and cancellation ownership, usage ledger, and structured child
snapshots remain authoritative.

## PRV-201 DeepSeek cache-usage compatibility (2026-08-28)

DeepSeek Chat usage decoding now treats a complete, bounded, internally
consistent `prompt_cache_hit_tokens`/`prompt_cache_miss_tokens` pair as the
authoritative cache-billing partition. Supplemental `prompt_tokens_details`
metadata is ignored for an explicit `HitMiss` contract, so null, numeric,
malformed, or contradictory supplemental values cannot discard a valid
completion or change its accounting. Missing or malformed required counters,
incomplete pairs, oversized values, and totals that disagree with
`prompt_tokens` still fail closed. Canonical Chat usage records reported cache
reads and misses while leaving writes unknown.

The first-party `deepseek_v1` contract now selects cache usage by protocol:
Chat Completions uses `HitMiss`, while Responses uses `OpenAiDetails` and
decodes `input_tokens_details.cached_tokens`. Both remain provider-managed and
send no outbound cache controls. A byte-split SSE regression reproduces the
live failure after normal text with an empty-choice usage chunk and proves
`Usage` plus `Done`; Responses resolution and usage are covered independently.
Reasoning/tool continuation, provider verification, exact pricing, and generic
provider behavior remain unchanged.

Focused tests and both workspace matrices pass, along with formatting, strict
all-feature Clippy, optimized all-feature release, release help/version and
signature, diff/snapshot hygiene, and committed protected-tree verification.
The installed binary matches the validated candidate at SHA-256
`82c72a53a0092c1a274a92b550d5790d530fef39515247bd525144ec1fd603ea`;
the prior binary is recoverable at
`/Users/aji/.local/bin/ca.backup-prv201-20260828-005217-e68c2bfb`. The single
authorized default-profile live verifier passed
`deepseek/deepseek-v4-pro` with exactly two no-retry requests and reported
`ready`. No credential or provider body was inspected or printed, and no
configuration, session, trace, ledger, or protected tree was mutated.

## INT-194 dependency-aware verification and automatic diagnosis (2026-08-27)

Codex-team now plans focused evidence from a bounded package graph rather than
path affinity alone. Tracked changed paths map to owning packages, transitive
reverse dependencies, targets/tests, and typed no-shell recipes. Rust metadata
and Swift package descriptions run offline under the production isolation
boundary; tracked Node and Python manifests are parsed directly. Graph inputs,
packages, selected closure, commands, paths, and metadata are bounded. Graph
failure, excessive fan-out, cross-cutting inputs, and multiple ecosystems
require an explicitly policy-authorized full suite; otherwise the receipt is
unverified instead of falling back to guesses.

Authoritative failures are now diagnosed from the exact durable receipt before
CA spends a repair continuation. The deterministic diagnosis classifies
environmental, likely pre-existing, likely change-caused, or indeterminate
causes and reports bounded package/target/path/rerun evidence without rerunning
the recipe or calling a model. CA standardizes successful edits as sorted,
deduplicated 1–64-file mutation batches; undo/redo prevalidates the complete
batch and applies reverse/forward atomically. Newly created files and untyped
mutators remain unscoped and cannot gain query authority.

Interactive CA now exposes a process-local `/evidence [JOB_ID|metrics]` ledger
covering preflight, mutations, dependency selection, verification, diagnosis,
repair, feedback, drift, and terminal authorization. A profile-scoped mode-0600
rolling quality file stores only hashes, bounded categories/reason codes,
timing, cache/repair/fallback estimates, and feedback—never prompts, outputs,
contents, canonical paths, or secrets. Negative feedback clears CA's local
preflight cache and continues through codex-team's evidence-cache and learning
quarantine path without a provider call.

Saved sessions may carry an optional `MechanisticCheckpointV1` without changing
message bytes or the session version. Explicit session opening validates the
profile, delegate server, revision, job/request/receipt digests, bounded paths,
and non-increasing repair budget before model-free polling. Current terminal
work advances at most one provider continuation; stale, malformed, missing, or
mismatched work becomes visibly blocked and is never restarted. `--no-session`
remains process-only. Watcher and scheduled completions, refresh, polling,
cancellation, and feedback remain model-free, and the existing recursion
exclusions plus exact 11/15/28 zero-resource catalogs are unchanged.

Completion validation passed focused protocol, graph/diagnosis, MCP,
mutation/undo, session/event, evidence/history, schedule, retry, and exclusion
coverage. The single workspace run's only failure was the stale 90-command
palette snapshot introduced by `/evidence`; both affected snapshots now say
91 and the exact failed test passes. Formatting, strict all-feature Clippy,
optimized workspace build, release help/version, release 11/15/28
zero-resource compatibility, committed protected-tree/snapshot hygiene, and
production Seatbelt all pass. No runtime installation or profile mutation was
performed.

## INT-193 automatic mechanistic preflight and governed verification (2026-08-27)

CA now treats an enabled `[mcp.mechanistic]` delegate profile as an automatic
mechanical coprocessor. A deterministic local parser recognizes bounded
symbol, feature/edit, repository status/diff/history, approved recipe, and
fully specified local-observation intents. Before the first normal Main or
headless provider request, CA sends at most four extracted typed actions to the
repository-pinned codex-team server and injects a redacted 16 KiB receipt only
into that request. It does not persist or transmit the complete prompt as
mechanistic telemetry, and successful query evidence is cached only in process
memory by repository revision and action digest.

Codex-team expands five composite queries plus `verify_changed_files` through
tracked-file, repository-index, affected-work, recipe-registry, ca-watch, and
production-isolation boundaries. Receipt decisions now state confidence,
selected paths/recipes, reason codes, cache state, and isolation backend.
Successful isolated evidence can be reused only when repository revision,
typed action, affected-input bytes, working directory, toolchain/environment,
and isolation backend all match. Failures, timeouts, cancellation, stale or
unisolated work, and negative feedback are never reused. Host-unisolated
passes are explicitly advisory and cannot satisfy authoritative success.

Exact successful mutation receipts trigger focused post-edit verification.
Unknown-scope mutators compare the pinned revision and run a full detected
suite only when the repository actually changed and profile policy permits it.
Provider continuation waits for the durable receipt; a real failure allows at
most two current-turn repair continuations and cannot end as success. Explicit
user suppression records skipped/unverified evidence. A bounded process-local
coordinator deduplicates typed turn/edit/drift/delegation/watcher/schedule
events without initiating provider traffic. Schedule V2 supports one pure
mechanistic query or evidence target with zero provider requests and no
session. All excluded surfaces and the exact 11/15/28 zero-resource catalogs
remain unchanged. Completion validation and recoverable installation are
recorded in `TESTING.md`.

The completed release is installed at `/Users/aji/.local/bin/{ca,codex-team}`.
SHA-256 is
`e68c2bfb434c3c9ad936b12f3e523666cd1000e35a7bca9d2dabf418197df870`
for CA and
`841eff1f53dd98372d014e0f42416ec9f00503f30427d3bf856a2a5432cb881a`
for codex-team. Pre-INT-193 binaries are recoverable at
`/Users/aji/.local/bin/ca.backup-int193-20260827-153936` and
`/Users/aji/.local/bin/codex-team.backup-int193-20260827-153936`. The named
profile remains byte-identical at SHA-256
`2bc81df31bf284208e417fa34240016c6d0e240a5759e9b4f80f90cdb8b74a96`
and mode 0600. Installed `/delegate` reports the delegate catalog, 15 tools,
zero resources, and the pinned revision; `/jobs` reports no current activity.
Both controls were exercised without a provider request.

Release-readiness Git bookkeeping is no longer an activation veto. Dirty or
untracked state, untracked workspace members, and active-plan bookkeeping are
reported as informational facts. Actual input/workflow drift, protected-tree
or snapshot failures, failed gates, and any checkout change caused during the
gates still block. The exact 11/15/28 compatibility fixture also caught and
closed two unresolved task-import schema references introduced by embedding
the expanded mechanistic action schema.

## INT-192 live mechanistic delegation hardening (2026-08-27)

CA and codex-team now share an explicit literal/regex search contract. Literal
matching remains the wire-compatible default; explicit Rust regex matching is
available through `match_mode = "regex"`. Search scopes are repository-relative
exact tracked files or component-bounded directory prefixes expanded solely
from Git's tracked-file list. The Main-agent prompt and facade schema state that
codex-team already owns the pinned repository, so CA need not discover or send
an absolute root even when its launch workspace differs.

CA now distinguishes invalid tool arguments from missing workspace paths and
preserves bounded, redacted codex-team tool errors as typed server/code/message/
retryability values rather than collapsing them into protocol failures. Facade
results are truthful: completed work succeeds; invalid, stale, failed,
cancelled, and timed-out work fails; only unavailable or declined prefer-mode
work permits ordinary fallback.

`/retry` now records a process-local continuation boundary. If provider
transport fails after a fully resolved tool batch, the retry branch retains the
exact successful, failed, denied, and mutating calls plus outputs, discards only
the partial failed assistant continuation, and issues no duplicate host calls.
Failures before any resolved tool retain whole-turn replay. Persistent and
`--no-session` branches keep the selected provider/model, while the original
session remains immutable. All recursion exclusions and the 11/15/28,
zero-resource MCP catalogs are unchanged.

Focused protocol, query-safety, typed-error, facade, exclusion, retry, and real
stdio compatibility tests pass. The real fixture binds codex-team to one
temporary repository while CA is launched in another, completes relative
directory-prefix regex search, and returns an actionable `INVALID_ARGUMENT`
failure for a malformed regex. Completion gates and recoverable installation
are recorded in `TESTING.md` and the completed INT-192 ExecPlan.

The serial workspace evidence covers 2,080 passing CA tests, 38 CLI, 17
role-run, 15+3 ca-watch, 13 codex-team integration, and four protocol tests.
One stale codex-team assertion was corrected from generic query failure to the
new intentional stale-revision code; the full failed package target then passed
586 tests with one ignore. Formatting, strict all-feature Clippy, optimized
all-feature build, diff/snapshot/protected hygiene, production isolation, exact
release MCP compatibility, and reverse release-CA role-run compatibility pass.

Validated releases are installed with SHA-256
`c37240f55d320e02e1bf66d8968134ac2b61914d910f3ab0ce20d7f7bf588061`
for CA and
`aba99855744d39d838e77a897457df7e1560f69912c8c8b23d4035b129efc479`
for codex-team. Pre-INT-192 binaries are recoverable at
`/Users/aji/.local/bin/ca.backup-int192-20260827-001414-b966d7fd` and
`/Users/aji/.local/bin/codex-team.backup-int192-20260827-001414-770a000f`.
The named profile remains byte-identical/mode 0600. Installed `/delegate`
reports 15 tools and zero resources, `/jobs` is empty, and shutdown reaps the
MCP child.

Validation incident: a positional `doctor` smoke was mistakenly used instead
of the offline `--doctor` flag. It initiated one live provider turn and
completed two read-only mechanistic calls before interruption (exit 130). No
session, repository, profile, or configuration bytes changed; the corrected
offline doctor then passed and started no MCP child. Do not describe the
overall INT-192 validation session as provider-free.

## INT-191 protected-tree baseline and delegate profile active (2026-08-26)

Commit `1530d660916ecb8d6f5befa68d1a850e9912c674` contains only
`.codex-team-protected-trees.json` and establishes the approved day-zero
SHA-256 Merkle baseline for the ignored `oracles/` and `reference/` trees.
The verifier loads that manifest from `HEAD`, keeps the direct Git check, and
fails closed for missing, malformed, edited, stale, mismatched, hardlinked,
special, escaping, cross-device, oversized, or concurrently changing input.
The unchanged real trees now pass the built-in protected-tree receipt; direct
Git status/diff still names no protected path.

The protected-baseline preview/apply lifecycle is exact revision- and
digest-bound. Preview double-scans without writing; apply atomically writes
only the manifest after recomputation. The user-reported composer regression
was also fixed: ordinary and native horizontal cursor motion now reaches the
composer while slash-command and `@file` overlays are open. Three focused
overlay/path tests and the full 110-test input suite pass.

The complete workspace suite, formatting, strict all-feature Clippy, optimized
workspace build, help/version, diff/snapshot/protected hygiene, production
Seatbelt isolation, exact 11/15/28 real CA MCP compatibility, and reverse
release-CA role-run compatibility pass. Local activation no longer treats the
publication audit's clean/tracked Git findings as an installation veto; it is
bound to exact validated artifact hashes, the protected-tree receipt,
recoverable backups, and post-install compatibility instead.

Validated releases are installed at `/Users/aji/.local/bin/{ca,codex-team}`
with SHA-256 hashes `b966d7fd02a5f97a58f0a42ec99b08d98ec8dbd1a4d3054bdf7786810b1a98f9`
and `770a000fb296be68732278cc06fa1e3157491de7bf24e5396bd6bf07f6a502d9`.
The isolated mode-0600 profile
`/Users/aji/.config/ca/profiles/codex-team-delegate.toml` is active; the
default profile hash remains unchanged. Real TUI `/delegate` reports the exact
15-tool, zero-resource catalog, `/jobs` includes codex-team delegations, and
shutdown leaves no MCP child. The first-turn facade now explicitly tells the
model to prefer codex-team automatically for eligible mechanical work before
ordinary exploration or agent spawning; users do not need to name the tool,
and fallback requires an allowing receipt. No provider request, token accounting, service,
credential bytes, or protected content was used or changed. The smoke also
fixed literal sandbox access to an installed stdio executable and made
uninitialized codex-team `status` return a mutation-free repository receipt.
The first natural-language delegation then exposed a model-owned schema-version
bug; the facade now hides that discriminator, accepts action-only calls, and
constructs the downstream version itself while retaining exact legacy-call
compatibility.

## INT-190 CA ↔ codex-team mechanistic cooperation complete (2026-08-26)

The workspace now has one protocol-only `codex-team-protocol` crate and two
non-recursive execution directions. Codex-team remains the governor: its typed
pre-Worker router can supply compact repository context, complete admitted
Rust-only evidence without a Worker, and invoke MCP-free `ca role-run` exactly
once for edits. An opted-in ordinary CA TUI or headless profile exposes one
first-turn `delegate_mechanistic` facade; CA supplies the repository revision
and directly performs a bounded query or starts and polls one durable evidence
or local-observation job without additional provider turns. The facade is off
by default and absent from Plan, review, checkpoint, scheduled, service,
subagent, bare, and role-run surfaces.

The configured delegate server must be local stdio, network-disabled,
resource-free, and expose the exact 15-tool delegate catalog. Its raw tools are
hidden from model MCP search. `/delegate` and unified `/jobs` now provide
process-local summaries, exact durable recovery, refresh, cancellation, and
fixed categorical feedback. Cancellation stops only executor-owned work or
observation and never signals an observed PID; correction feedback needs a
separate explicit confirmation and fresh revision.

The final serial run passes 2,070 CA tests with two documented ignores, 38 CLI
tests, 17 role-run tests, 15 ca-watch tests plus three CLI tests, 580 codex-team
tests with one external-binary ignore, all eleven codex-team integration tests,
and all three protocol tests. Formatting, strict all-target/all-feature Clippy,
the all-feature release build, release help/version, and the explicitly invoked
real 11/15/28 zero-resource CA client fixture pass. Release doctor reports the
production isolation subcheck healthy as `macos-seatbelt-source-projection-v1`;
the overall doctor remains intentionally unhealthy only because this root is
not initialized as a codex-team target. No runtime profile/session, installed
binary, provider, service, oracle, or reference state changed, and no
implementation commit was created.
Direct Git status/diff confirms no protected-tree changes and built-in snapshot
hygiene passes. The built-in protected-tree receipt remains fail-closed because
the ignored `oracles/` and `reference/` roots have no trusted tracked-content
baseline; it does not report a concrete changed file.

## TUI-189 truthful model-catalog status complete (2026-08-26)

Full and provider-scoped model refreshes now distinguish optional live catalog
freshness from route availability. Catalog success is live; failed discovery
with configured or documented models is fallback; only a failed provider with
neither source is unavailable. Provider rows retain sanitized failure detail,
while footer categories are mutually exclusive. The user-reported shape is now
`13 models · 2 live · 2 fallback`: direct OpenAI remains a documented fallback
without an API key, and the working subscription-backed OpenAI Codex route is a
configured fallback when its catalog endpoint is unavailable.

The initial Codex, documented-fallback, provider-scoped, and model-less
characterizations failed as expected. The first serial run found one stale
assertion requiring a configured provider to be unavailable; after correcting
it, the complete run passed 2,061 CA tests, 38 CLI tests, 17 role-run tests, 15
watcher tests, three watcher CLI tests, 556 scheduler tests, and all
integration/doc targets, with three documented manual/external checks ignored.
Formatting, strict all-target/all-feature Clippy, the all-feature release build,
release smoke checks, diff/snapshot hygiene, and protected-tree checks pass.
No provider request, configuration, credential, session, model list, or
protocol changed during validation.

The verified release was installed at `/Users/aji/.local/bin/ca`; its SHA-256
matches `target/release/ca` at
`da46aaeb4715debabe3e271c361be3a9d12c81c1ad751098bfc632bab3a03c51`.
The previous TUI-188 binary is recoverable at
`/Users/aji/.local/bin/ca.backup-tui189-20260826` with SHA-256
`0f457421a0fd1ca1bf5a2d4bcc8bfb091f02f97fe606f00b9f3ec8c7bd886b22`;
hash equality, strict signature verification, command lookup, help, and version
checks pass.

## TUI-188 collapsible edit/command activity complete (2026-08-26)

The built-in presentation registry now classifies direct file mutations and
`run_command` as activity calls. Runs of at least two consecutive calls group
across tool-only assistant messages and stop at user, visible prose/reasoning,
exploration, unrelated-tool, or recorded turn boundaries. A process-only map
retains exact live mutation receipts for Created, Edited, and Deleted labels;
restored sessions reconstruct only what tool names, arguments, and canonical
markers prove, falling back to Changed. Standalone activity calls and the
existing `Explored` grouping remain unchanged.

Completed groups render as one bounded accent header with status, grammar, and
chevron. Ctrl+O expands the latest group into chronological underlined relative
file rows, complete sanitized command text, and the existing diff, failure,
semantic-command, and adaptive-output detail. Render-time document-coordinate
hit regions let a no-drag mouse release toggle any visible current, older, or
scrolled header. Drag selection wins, overlays and pagers clear or isolate the
targets, and a collapsed redraw removes all expanded cells. Canonical
`ToolCall.output`, command capture, model results, sessions, `/output full`,
generated files, and privacy-safe whole-transcript copy are unchanged.

The initially failing activity characterization now passes 19 focused tests;
all 197 render tests pass with nine reviewed 60-, 100-, and 184-column
TUI-188 snapshots, and all 107 input tests pass. The complete serial workspace
run passes 2,055 CA tests, 38 CLI tests, 17 role-run tests, 15 watcher tests,
three watcher CLI tests, 556 embedded scheduler tests, and all integration/doc
targets; three documented manual/external checks are ignored. Formatting,
strict workspace all-target/all-feature Clippy, the workspace all-feature
release build, help/version, diff/snapshot hygiene, and protected-tree checks
pass. Validation made no network/provider request and changed no runtime
profile/session, oracle, or reference state. After a separate explicit user
request, the validated release was installed at `/Users/aji/.local/bin/ca` with
SHA-256 `0f457421a0fd1ca1bf5a2d4bcc8bfb091f02f97fe606f00b9f3ec8c7bd886b22`.
The prior binary (SHA-256
`b1e029ff2a87d691f1eabd55a6a5960480ce0969096508d5571a58f0c7545591`)
is recoverable at
`/Users/aji/.local/bin/ca.backup-tui188-20260826`; installed hash equality,
strict macOS signature verification, command lookup, help, and version pass.

## ASY-187 dedicated process watcher complete (2026-08-26)

The workspace now includes `ca-watch`, a reusable Rust library and standalone
CLI for typed local or SSH-hosted process observation. Versioned requests and
events cover generic PIDs and copy jobs, enforce nonzero PIDs and bounded
labels/destinations/absolute paths/cadence, capture an opaque identity
fingerprint, check copy completion markers before PID state, bound log progress,
and distinguish target failure from recoverable transport degradation. Remote
inspection uses fixed `/usr/bin/ssh` batch/no-TTY/no-forwarding arguments and a
fixed quoted probe script; the crate accepts no recurring command text,
password, private-key content, or forwarding configuration. Stopping a watch
never signals its target.

Interactive CA sessions expose typed `watch_process` and `stop_watcher` tools
with stable originating-session ownership and exact remote network approval.
Plan, headless, service, scheduled, and subagent surfaces omit or reject them.
Watcher broadcasts enter the existing Tokio loop as redraw-only events: a
conditional activity strip, unified `/jobs` list/detail view, stop-watching
control, and completion/failure toasts update without provider polling,
synthetic messages, or token-accounting changes. Explicit background shell jobs
now remain alive until completion, cancellation, or shutdown while foreground
timeouts retain their configured behavior.

The clean serial workspace run passes 2,040 CA tests, 38 CA CLI tests, 17
role-run tests, 15 `ca-watch` library tests, three `ca-watch` CLI tests, 556
embedded scheduler tests, and all integration/doc targets; three documented
manual/external checks are ignored. All 185 render tests and 107 composer-input
tests pass, including wide/narrow activity, unified jobs, unreachable recovery,
terminal toast, watcher-control, and provider/token-isolation coverage.
Formatting, strict workspace all-target/all-feature Clippy, the workspace
all-feature release build, release help/version checks for both binaries, diff
and snapshot hygiene, and protected-tree checks pass. Validation made no live
SSH/BSD or provider request and changed no runtime profile/session, installed
binary, oracle, or reference state.

## TUI-186 adaptive tool-output layout complete (2026-08-26)

Ordinary in-TUI tool output now uses the available transcript width and closes
every visual row with a padded right border. Sanitized tab-separated runs of at
least two equal-arity rows render as deterministic tables: source order and
empty cells are preserved, numeric columns align right, identifier-like headers
are emphasized, and long Unicode cells wrap by display width under Ctrl+O.
Isolated or inconsistent tabs expand at conventional eight-cell stops.

Collapsed cards retain at most 20 complete visual rows and report omitted
source lines; expanded cards show the full bounded capture. The transformation
is process-only: canonical tool output, provider continuation, sessions,
exports, saved TSV files, and native exact-output reruns are unchanged.

The initially failing four-test characterization now passes as five pure layout
tests. All 182 TUI render tests pass with targeted 60-, 100-, and 184-column
coverage, including expanded/collapsed redraw and clean later Markdown. The
complete serial workspace run passes 2,030 CA tests, 38 CLI tests, 17 role-run
tests, 556 embedded scheduler tests, and all integration/doc targets, with
three documented manual/external checks ignored. Formatting, strict workspace
all-target/all-feature Clippy, and the workspace all-feature release build
pass. No network, runtime profile/session, installed binary, oracle, or
reference state was changed.

## REL-185 Hermes/MiMo-style interactive approval and command UX complete (2026-08-26)

Interactive built-in tools now receive one approval decision, matching the
observed Hermes/MiMo shell flow. A command's prepared network requirement is
part of that decision: RunEverything and AutoReview do not add a preliminary
network-only modal, an explicit Ask rule produces one command approval, and
explicit Deny still blocks. Headless, service, MCP, child, email, and privilege
authority boundaries remain unchanged. This supersedes REL-183's narrower
request-text SSH exception on the ordinary TUI surface without changing its
historical validation record.

The approval transcript renders the complete sanitized command and optional
working directory instead of clipping argument JSON at 80 characters. Slash
commands are recognized only from a leading composer token, so the exact
reported sentence ending in `/bigmac/sed-ro/seedbox/` submits as prose.
Pressing Enter with an empty path-completion result now dismisses completion
and submits normally. Nonterminal `await_job` observations (`Running` and
`Cancelling`) are successful status reads rather than failed command results.

The new characterization tests failed against the reported behavior and pass
after the repair. Approval-focused coverage passes 84 tests across the
workspace, the composer suite passes 106 tests, and the exact running-job and
read-only SSH/order regressions pass. The final serial workspace run passes
2,022 CA tests, 38 CLI tests, 17 role-run tests, 556 embedded scheduler tests,
and all integration/doc targets, with three documented manual/external checks
ignored. Formatting, strict workspace all-target/all-feature Clippy, and the
workspace all-feature release build pass. Validation used no live network
request and changed no runtime profile/session, oracle, or reference state.
After a separate explicit user request, the repository rule was clarified to
permit validated user-requested installation, and the exact release was
installed at `/Users/aji/.local/bin/ca` with SHA-256
`b1e029ff2a87d691f1eabd55a6a5960480ce0969096508d5571a58f0c7545591`.
The prior binary is recoverable at
`/Users/aji/.local/bin/ca.backup-rel185-20260826`; installed hash equality,
strict macOS signature verification, normal command lookup, `--help`, and
`--version` all pass.

## TUI-184 native macOS composer editing complete (2026-08-26)

The primary composer now follows the expected macOS editing model:
Option+Backspace deletes the prior Unicode word, Command+Backspace deletes to
the beginning of the current hard line, Option+Left/Right moves by word, and
Command+Left/Right moves to hard-line start/end. The same gestures work while
inline path completion is visible. Deletions expand across process-local
atomic paste/file/folder/reference ranges rather than corrupting a partial
attachment, and removed reference bookkeeping stays synchronized.

The terminal lifecycle now pushes disambiguated keyboard reporting on a
best-effort basis and pops it on suspension, setup failure, normal drop, and
panic cleanup. This lets compatible terminals expose Command as a distinct
modifier without making enhanced-keyboard support a startup requirement.
Option+Backspace is reserved for composer editing; Remove Last Attachment now
uses Ctrl+X A.

The new regressions failed before implementation. All 105 composer-input tests,
six keymap tests, and ten terminal lifecycle tests pass. The complete serial
workspace run passes 2,023 CA tests, 38 CLI tests, 17 role-run tests, 556
embedded scheduler tests, and all integration/doc targets, with three
documented manual/external checks ignored. Formatting, strict workspace
all-target/all-feature Clippy, and the workspace all-feature release build
pass. No runtime profile/session, installed binary, oracle, or reference state
was changed.

## REL-183 request-scoped read-only SSH authority complete (2026-08-26)

The generic network gate no longer interrupts an ordinary live AutoReview or
RunEverything turn for every SSH read the user already requested. CA binds the
one-call authority to the exact host/alias named in the initiating user message
and accepts only a closed non-interactive `ssh` invocation whose complete
remote pipeline is classified read-only. The reported pair of `root@bsd` file
inspections now bypasses the approval queue.

The authority is process-only and is carried through deferred ordered
execution. Appended attachment, directory, and image context is excluded from
intent matching. Allowlist, no-live-request and mismatched-host cases still
prompt; persistent Ask/Deny still win; transfers, forwarding, alternate SSH
config, redirects, substitution, sequencing, background execution,
mutation-capable `find`/`sort` modes, `tee`, unknown stages, browser/MCP
network, headless/service/role-run, goals, workflows, and restored turns are
unchanged.

Focused SSH, permission, approval, workspace-network, AutoReview, and ordered
execution suites pass. The complete serial workspace run passes 2,019 CA tests,
38 CLI tests, 17 role-run tests, 556 embedded scheduler tests, and all
integration/doc targets, with the three documented manual/external checks
ignored. Formatting, diff hygiene, strict workspace all-target/all-feature
Clippy, and the workspace all-feature release build pass. Tests blocked the
reported commands before process launch, so validation made no live SSH or
provider call and changed no runtime profile/session, installed binary,
oracle, or reference state.

## TUI-182 prose-safe path completion editing complete (2026-08-26)

Inline path completion no longer interprets every slash-separated composer
token as a filesystem path. Automatic discovery requires explicit path syntax
(`./`, `../`, `~/`, or absolute `/`), so ordinary text such as `red/ops` stays
in the composer without an error popup. Bare relative paths remain available
through explicit Tab completion.

The open path surface now leaves Left/Right available for grapheme-aware
composer cursor movement while Up/Down retain row selection. Moving within an
unchanged token does not restart its generation-tagged discovery; moving to a
different token refreshes or closes the surface normally. The three new
regressions failed against the reported behavior and pass after the correction.

All seven path-completion tests and all 101 input tests pass. The final serial
workspace run passed 2,631 tests with three documented manual/external ignores.
Formatting, strict workspace all-target/all-feature Clippy, and the workspace
all-feature release build pass. No runtime profile, provider, installed binary,
oracle, or reference state was changed.

## SKL-181 session-automatic skill activation complete (2026-08-25)

When a bounded metadata catalog contains a relevant skill, the primary model
can now call `activate_skill` with its exact qualified ID without a generic
approval interruption. The digest-pinned body becomes available on the next
provider turn and remains scoped to that session. Interactive, headless, and
service execution use the same contract.

This changes context loading only. Exact persistent Ask/Deny rules still
override the default, and unknown, ambiguous, unavailable, changed, or
over-budget skills still fail closed. Activation grants no filesystem,
process, network, MCP, email, privilege, dependency, or later-tool authority;
those actions retain their existing approval and sandbox boundaries. Composer
text performs no host-side relevance matching and stays quiet.

Focused automatic-activation, skill, headless, service, and session suites
pass, including next-turn-only prompt injection, exact Ask/Deny precedence,
later mutation approval, and session save/restore isolation. Formatting,
strict workspace all-target/all-feature Clippy, and the release build pass.
The first all-feature serial run found one stale AutoReview assertion; it was
updated to the new contract and its exact rerun passed. The final default
serial workspace suite then passed 2,006 CA tests, all 38 CLI and 17 role-run
tests, 556 embedded `codex-team` tests, and every integration/doc-test target.

## TUI-180 quiet skill discovery complete (2026-08-25)

The idle composer no longer performs or renders passive skill ranking, so
ordinary draft text cannot replace the stable status row with a changing
`Not activated` message. Skill discovery remains available on demand through
`/skills <query>`, while SKL-181 separately lets the model activate an exact
relevant skill for the session. TUI-180 itself changed no skill body, prompt,
session, provider, approval, or persistence boundary.

Focused skill coverage and the refreshed wide/narrow composer snapshots pass,
as do formatting, strict workspace all-target/all-feature Clippy, and the
workspace all-feature release build. The full serial workspace suite passed
all change-related coverage but reported one unrelated timing-sensitive voice
descendant-cleanup failure; its exact serial rerun passed immediately.

## LRN-179 cross-session recall and skill operations complete (2026-08-25)

Ordinary primary-agent tasks now have the read-only `session_search` tool, and
`/recall` exposes the same local search/browse/read/scroll surface without a
provider call. A lazy process-only SQLite FTS5 index covers all retained
completed CA, Codex, Hermes, and MiMo/Xiao history. Source discovery and index
construction run off the TUI event loop; exact-after-deterministic-redaction
text is cursor-paged under independent message/result/aggregate bounds. The
index is never persisted, duplicate sessions retain every provenance record,
and foreign databases remain immutable query-only inputs with no WAL/SHM.

Portable skills now enforce Hermes-compatible platform, tool, toolset,
fallback, required-environment, and typed non-secret configuration metadata.
Incompatible skills are absent from normal model/slash discovery and visible
under `/skills unavailable`; setup-required skills remain inspectable through
`/skills config`. Values resolve workspace override, active-profile value, then
declared default, and only the activated skill's quoted resolved data enters a
request. Credential-like declarations and values fail closed.

Profile and workspace YAML bundles group two to five exact digest-pinned skills
for one prompt. Discovery rejects ambiguity and collisions, activation is
atomic, combined bodies retain the existing 12,000-token ceiling, and explicit
create/delete uses recoverable journals. Foreign bundles are read-only.
`/bundles` owns list/show/create/delete/reload and bundle slash commands refresh
immediately.

The owner-only lifecycle ledger records metadata—not prompts, transcripts, tool
arguments/results, or secrets—for exact skill/bundle use, attributed outcomes,
mutations, learning/memory journals, undo, and recovery. `/learning` and its
`/journey` alias provide filters and exact drilldown while deterministic
suggestions cover one correction/two failures, ninety-day staleness, and three
successful repeated skill sets. Suggestions never patch, archive, delete,
install, or execute anything.

Focused suites and the complete workspace run pass: CA has 2,005 active unit
tests plus all CLI/role-run suites; embedded `codex-team` has 556 active tests.
Formatting, strict all-target/all-feature Clippy, the all-feature release build,
`git diff --check`, snapshot hygiene, and protected-tree checks pass. Validation
used temporary fixtures only. No live provider, real profile, `/learn`, binary
installation, credential read, or oracle/reference mutation occurred. The
completed plan is
`docs/exec-plans/completed/LRN-179-cross-session-recall-skill-operations.md`.

## LRN-178 efficient, auditable learning results complete (2026-08-25)

Future manual `/learn` and `/distill` runs now start from typed host-side asset
and memory indexes plus the complete workflow-cluster catalog whenever its
encoded representation fits the existing overview bound. The real-shaped
119-cluster catalog fits, so it needs no catalog-page provider round trip.
Signal is deterministic: three sessions or an explicit recurring instruction
is high, two sessions is medium, and an ordinary singleton is low. One bounded
cluster-inspection call returns two representative sanitized sessions for each
requested high/medium cluster together with relevant exact assets and memory.
Finalization reports the exact missing cluster IDs until every required
representative has been inspected.

Learning decisions now cite digest-bound typed asset IDs and typed memory
dispositions. Duplicate or extend-existing decisions cannot invent or drift an
asset reference, and `already_covered` memory decisions cite exact memory IDs.
Individual retrievals, per-turn aggregate retrievals, and retained provider
context are bounded independently; consumed tool and continuation payloads are
compacted rather than replayed through later turns. Privacy exclusions and the
eight-turn/cancellation/interruption/publication boundaries are unchanged.

Direct publication journals are version 5 and retain the complete decision,
resolved-asset, memory, model, turn, usage, and coverage audit. Versions 1–4
remain readable and their drilldown states that unavailable detail was never
recorded. `/learn status <run-id>` and `ca learn status <run-id>` expose the full
audit. Normal receipts distinguish existing assets from proposals, show no more
than twelve prioritized five-column rows, resolve duplicate asset paths,
summarize memory outcomes, collapse low-signal singleton noise, and end with the
exact status command.

The exact 190-session/1,596-message/119-cluster fixture with 132 existing assets
and all three MiMo BSD commands passes in two scripted provider turns. The late-
evidence 184-session fixture, focused adaptive/archive/privacy tests, compact
wide/narrow snapshots, and the complete workspace test suite pass. Formatting,
strict `cargo clippy --all-targets -- -D warnings`, and `cargo build --release`
also pass. No live `/learn`, evidence reopening, runtime-profile mutation,
installed-binary replacement, or oracle write occurred. The completed plan is
`docs/exec-plans/completed/LRN-178-efficient-auditable-learning-results.md`.

## LRN-177 resilient learning finalization complete (2026-08-25)

Adaptive correction now treats the host's current stable cluster catalog as the
identity authority. Unknown cluster references are discarded only from
non-authorizing `skipped` or `needs_more_evidence` rows; an unknown-only row is
dropped, while any unknown reference attached to `created` or `extended`
authority still fails closed. Known decisions remain additive across repair
turns. For catalogs of at most 512 clusters, the submission schema enumerates
the exact currently allowed IDs and correction turns enumerate only the missing
IDs; larger catalogs keep the compact schema and the same exact host validation.

The user-authorized repository-built bare `/learn` completed once, without
retry, as run `020c3875-ef34-4a0e-b034-51708fe22b0a` on
`openai-codex/gpt-5.6-sol`. It made 189 sessions / 1,591 messages searchable
(CA 115, Codex 872, Hermes 4, MiMo 600), retrieved 207 messages across 66
sessions, cited 26 support sessions, delivered both cluster-catalog pages,
explicitly decided all 118 clusters, and retired all 1,591 messages only after
the journal committed. It used four adaptive turns, 20 bounded retrieval steps,
120,418 input plus 5,503 output tokens. The evidence-backed zero-change result
was correct: the recurring BSD cluster is already represented by
`~/.mimocode/command/check-rsync.md`, `check-transmission.md`, and
`check-seedbox.md`, which LRN-176 makes directly discoverable to CA.

That run exposed one final receipt-only defect: direct Inventory output omitted
Hermes from a stale hard-coded source list even though status and the journal
counted it. Direct receipts now use the complete CA/Codex/Hermes/MiMo/Other
source renderer, with a focused regression. All focused suites and the final
2,599-test workspace run pass, as do formatting, strict all-target/all-feature
Clippy, the all-feature release build, release help/version/doctor, and
diff/snapshot hygiene. One unrelated `codex-team` clean-filter test transiently
failed on its first full rerun, passed in isolation, and passed in the subsequent
complete serial rerun. The optimized binary was still uninstalled when the plan
completed. The completed plan is
`docs/exec-plans/completed/LRN-177-resilient-learning-finalization.md`.

Post-completion, the user explicitly authorized installation. The exact
validated release was atomically installed at `/Users/aji/.local/bin/ca` with
SHA-256 `5474464afa6dadf5c2e6497481ff31c72a62c2895d334bc35ec3a5d841c9d415`.
Normal command lookup resolves to that path; version, help, offline doctor
(`Ready`), checksum, and strict macOS code-signature verification pass. The
previous SHA-256 `a74a8a007a2d7eec318a1127cae13868717365081bf8de6e62c93823cf766a93`
binary is preserved at
`/Users/aji/.local/bin/ca.backup-lrn177-20260825T135550Z`. No learning or
provider call was made during installation verification.

## LRN-176 foreign knowledge interoperability complete (2026-08-25)

CA now mounts compatible Codex, Hermes, and MiMo skill trees in place as
immutable `Managed` instances, and discovers MiMo/OpenCode/Codex-compatible
prompt commands as ordinary inert slash turns. The real local registry exposes
81 structurally compatible Hermes skills, 22 bundled MiMo skills, and one Codex
skill. An overlapping Codex system root was removed so that instance is no
longer ambiguous. The one incompatible Hermes tree contains binary PDF
templates and remains rejected under CA's text-only full-tree invariant; an
equivalent MiMo-bundled skill is available.

Bounded sanitized Codex/Hermes/MiMo memory notes now participate in normal
query recall and direct learning with explicit untrusted provenance. Hermes's
`state.db` is a standard read-only/query-only learning source: completed main
sessions contribute user requests, final conclusions, safe action shapes,
error classes, recovery, and stopping conditions, while reasoning columns are
never selected and raw tool output, email activity, secrets, incomplete turns,
and subagent sessions are excluded. Foreign assets are inventoried before
learning publication.

Focused foreign/command/memory/skill/archive-learning suites pass, as do all
2,593 workspace tests with three documented ignores, formatting, strict
all-target Clippy, and the release build. The real Hermes source preview was
local-only with zero provider calls and zero profile writes. No `/learn` run,
runtime publication, foreign write, or release installation occurred. The
completed plan is `docs/exec-plans/completed/LRN-176-foreign-knowledge-interoperability.md`.

## LRN-175 closed-loop MiMo/Hermes learning complete (2026-08-24)

Manual `/learn`, focused `/learn <procedure|path|URL>`, and `/distill` now share
one complete-archive adaptive investigation. Sanitized evidence retains
host-derived action shapes, success/failure classes, recoveries, user
corrections, outcomes, and stopping conditions while continuing to exclude raw
tool output, email, secrets, reasoning, incomplete turns, and subagent chatter.
Every stable workflow cluster must receive an explicit created, extended,
skipped, or needs-more-evidence decision before complete retirement. Retired
history remains searchable support and matching new failures or corrections
reactivate its cluster.

Generated skills may now contain complete bounded text trees (`SKILL.md`,
references, templates, scripts, assets, and agent metadata). The typed
`skill_manage` lifecycle supports create, patch, edit, delete, write-file, and
remove-file operations with an exact instance selector and full-tree digest.
It rejects traversal, links, binary/oversized payloads, drift, built-in
collisions, and read-only ownership. Changes stage through recoverable journals,
refresh slash discovery immediately, and never execute scripts or install
declared dependencies during publication. Actual use remains behind ordinary
CA tool, process, network, credential, and approval boundaries. A pinned skill
can be refined once in the same task after a concrete failure, recovery, or
user correction, with two bounded structured attempts and prior-tree
preservation on validation failure.

The named coverage-less run `9c24e870-3ae0-4036-b95d-80879b283659` was reopened
idempotently while preserving its memory and command. Its first real successor
exposed two protocol defects: otherwise-valid partial submissions were being
discarded, and host compaction rewrote visible function-call arguments while
retaining an inconsistent opaque OpenAI Responses item. Partial decisions and
artifacts are now accumulated, corrections receive only the missing cluster
IDs, and compacted submissions clear the superseded opaque provider item. A
116-cluster regression combines 72 initial and 44 supplemental decisions and
also exercises canonical Responses replay. Completed failed or exhausted runs
no longer auto-resume at startup; their evidence remains eligible, while true
process interruptions remain resumable.

The user-authorized repository-built bare `/learn` then completed as run
`58f65db2-272f-464c-bac3-b610be749ef3` using
`openai-codex/gpt-5.6-sol`. It made 186 sessions / 1,551 messages searchable,
retrieved 69 sessions / 236 messages, inspected 11 / 113, cited 66 / 329 as
support, delivered both catalog pages, explicitly decided all 116 clusters,
and retired exactly 1,551 messages only after the journal committed. The
truthful result created no new memory or skill package because the retained
trust-boundary agent already covered the supported reusable procedure. It used
eight provider turns and 148,767 input plus 8,611 output tokens; structural
validation passed and its undo command is
`/learn undo 58f65db2-272f-464c-bac3-b610be749ef3`.

All 94 focused CA learning tests plus 18 embedded learning tests pass, as do
the complete `cargo test` matrix, formatting, strict all-target Clippy, release
build, and diff hygiene. The optimized binary was not installed. The completed plan is
`docs/exec-plans/completed/LRN-175-closed-loop-mimo-hermes-learning.md`.

## LRN-171A full-archive truthful learning coverage complete (2026-08-24)

Manual learning now builds one ephemeral sanitized index over every eligible
30-day CA, Codex, MiMo/Xiao, and memory source record. The 32-session bound now
applies only to one search response; session inspection remains independently
bounded to eight. A deterministic complete workflow-cluster catalog, with
pagination when needed, keeps late and singleton evidence searchable without
inflating the first provider request.

Coverage is recorded as Indexed, Searchable, Retrieved, Inspected, Used as
support, and Retired. Exact message/session identifiers and catalog page
delivery drive complete versus partial retirement, and successful no-op runs
also receive recoverable journals. Failures, cancellation, exhausted repair,
or rollback retire nothing. Legacy `processed_message_ids` remains readable as
retired evidence.

The exact 184-session/1,515-message fixture puts rsync, Transmission, and seedbox
work beyond session 32 and proves one two-turn `/learn` can publish all three
parameterized commands while leaving variable dedup as a candidate and skipping
one-off FreeBSD investigations. Pagination, invalid repair, resume,
cancellation, rollback, zero-change completion, privacy exclusions, read-only
MiMo SQLite, wide/narrow receipts, and targeted migration are covered.

The named legacy run `567d44c0-fb60-466e-9cb5-d4f474ddf482` was reopened once;
its 1,515 retired IDs were cleared while its useful memory/assets were
preserved. A follow-up source-drift repair now binds reviews to one
archive-wide projection of the selected sanitized messages rather than the
files that happened to contain them. Appends and moves between registered
archive roots no longer invalidate a review, while selected-content changes
still do.

After the source-drift repair and all code gates, the repository-built CA ran
bare `/learn` once. Run `9c24e870-3ae0-4036-b95d-80879b283659` committed after
five adaptive turns, including one in-run structured-output correction. It
indexed, made searchable, and retired all 1,533 eligible messages across 186
sessions: 115 CA, 840 Codex, and 578 MiMo/Xiao. It delivered both pages of all
116 clusters, retrieved 144 messages from 30 sessions, inspected 43 messages
from eight sessions, and cited eight supporting sessions. It published
`.ca/commands/review-oracle-gaps.md`, refreshed slash-command discovery in the
same TUI, and recorded `/learn undo
9c24e870-3ae0-4036-b95d-80879b283659`.

The 61 focused archive-learning tests, formatting, strict Clippy, and release
build pass. The full suite passes serially. Parallel `cargo test` twice exposed
the embedded scheduler's pre-existing
`stateful_clean_filter_cannot_replace_prevalidated_commit_bytes` race; the
exact test and the complete serial suite pass, and no scheduler source was
changed for LRN-171A.

## LRN-174 MiMo end-to-end learning and Dream repair complete (2026-08-24)

The final audit re-read MiMo's complete Distill operator and prompt sources as
read-only evidence. CA's learning agent now receives bounded sanitized session
titles and ranked broad-term search across titles, user requests, assistant
conclusions, and safe tool-input summaries. This restores MiMo's useful
workflow discovery shape without persisting titles or raw transcript text and
without giving the model filesystem or SQL access.

A disposable first-run integration now proves the entire product promise in one
invocation: bare `/learn` automatically discovers two native histories, searches
them, completes in two provider turns, publishes one memory convention and one
native `.ca/commands/check-rsync.md`, renders the compact report, and undoes both
through the single run journal. It never uses `/learn sync`, review, approval,
or an Apply screen.

The reported Dream failure had a separate exact cause: evidence was truncated
by Unicode scalar count but validated by UTF-8 byte count, so a sanitized record
could remain unchanged at 8,234 bytes and fail every retry. Sanitized learning
text is now byte-bounded and idempotent, with a regression for multibyte input.
An additional product-path regression builds the multibyte corpus, queues it
through the enrolled background-maintenance bridge, reloads/validates it, and
exports it successfully. The supplied failure no longer blocks conservative
background maintenance.

All 79 focused learning tests pass. The final default workspace matrix passes
2,567 tests with three documented ignores; the all-target/all-feature matrix
passes 2,714 with four. Formatting, strict Clippy, the all-feature release build,
and release smokes pass. The release artifact is built but not installed, and no
live provider, real profile, oracle, or reference state was changed.

## LRN-173 first direct learning run repair complete (2026-08-24)

The first bare `/learn` or `/distill` run no longer discards the pending review
created by automatic standard-source registration. Direct preparation now keeps
that freshly synchronized evidence; when sources are already registered but no
review exists, it performs the required synchronization itself. The supplied
`no evidence is pending review` failure therefore recovers on the next direct
invocation without `/learn sync`, `/learn review`, or digest approval.

The new exact first-run regression failed before the repair and now passes. All
50 archive-learning tests and the 2,562-test workspace suite pass, as do
formatting, strict all-target/all-feature Clippy, the all-feature release build,
and release smokes. The release artifact is built but the installed user binary
remains unchanged, per repository policy.

## LRN-172 full command-operator examples complete (2026-08-24)

The slash command browser no longer gives its example/usage column a fixed
32-cell ceiling. Complete operator strings receive the space they need on wide
terminals; when the label plus description cannot fit, the row stacks and wraps
vertically instead of inserting an ellipsis. The supplied `/learn
[focus text|status|sync|undo <run-id>]` example and the longer `/distill` form
have an exact render regression. Wide and narrow snapshots also prove complete
review, workspace, goal, trace, and description rows remain readable.

The four command-browser tests, all 177 TUI render tests, and the 2,561-test
workspace suite pass. Formatting, strict all-target/all-feature Clippy, the
all-feature release build, release smokes, diff/snapshot hygiene, and protected
oracle/reference status pass. No installed binary or runtime state was changed.

## LRN-171 direct adaptive MiMo-style learning complete (2026-08-24)

Manual `/learn [focus]` and `/distill [focus]` now start one visible adaptive
run and publish validated output directly. The TUI path no longer uses evidence
or proposal pagers, digest approval, Apply dialogs, or retry-exact states;
legacy review/approve actions migrate into the same direct run. Progress and the
final Inventory/Shortlist/Created/Skipped/Needs-more-evidence/Receipt report stay
in the ordinary transcript, with explicit Escape cancellation.

The run starts from a bounded 30-day overview and can search sanitized evidence,
inspect selected sessions, refresh the asset inventory, and submit a strict
result. Clear evidence finishes in two provider turns; investigation is capped
at five turns followed by forced finalization and two in-run repair turns.
Native, Codex, MiMo SQLite, and memory sources are discovered automatically.

Reusable output now includes native prompt commands under `.ca/commands` or the
active profile, portable skills, and explicit-read-only custom subagents.
Commands expand `$ARGUMENTS` and `$1`–`$9` literally, cannot shadow built-ins,
refresh immediately, preserve human files, and use exact generated provenance
for replacement and undo. Publication has one aggregate direct-run journal.

The completion audit added compact visible investigation cards, identifier-only
logical-turn resume, active tool-capable model preference, active-workspace
publication authority, exact late-cancellation rollback, and prepared/applied/
committed grouped journals whose single undo command covers every workspace and
makes evidence eligible again.

A final real-source repair makes native CA bundles first-class instead of
feeding `*.turns.jsonl` into the Codex `session_meta` parser. Large valid Codex
rollouts now stream with independent record/message bounds, unsupported sibling
archives do not abort usable evidence, and first-time automatic registration
does not immediately repeat the full scan. The optimized uninstalled binary
previewed the actual 151-file, 1,063,142,977-byte source set in 3.91 seconds,
retaining 1,632 completed candidate messages with zero provider calls and zero
profile writes.

The final workspace matrix passed 2,560 tests; the all-target/all-feature
matrix passed 2,707. All 49 focused archive-learning tests, reviewed snapshots,
formatting, strict Clippy, the all-feature release build, release smokes, and
hygiene checks pass. No live provider, real profile publication, installed
binary, oracle, or reference state was touched. The completed plan is
`docs/exec-plans/completed/LRN-171-direct-adaptive-distillation.md`.

## LRN-170 MiMoCode-style learning and Distill complete (2026-08-24)

Manual reviewed learning now performs one coherent structured analysis instead
of a fixed Dream/Distill call sequence. The request inventories existing
portable skills and custom agents, sees bounded session boundaries, timestamps,
and safe tool-name/input summaries, and returns memory operations, inert skill
packages, and an evidence-backed decision shortlist together. Progress and
pricing are `0/1` through `1/1`; exact approval, checkpoint resume, drift
refusal, journaled publication, conflicts, and undo remain authoritative.

Native sessions retain useful command/path/query inputs but never tool outputs,
email activity, secret-shaped arguments, or content bodies. MiMoCode import now
recognizes both empty and `main` main-agent identifiers, reads intermediate tool
inputs in the completed user-to-final window, and keeps SQLite query-only and
the persisted index transcript-free. Fork deduplication compares visible
content and retains the richer workflow-bearing copy. Compaction prioritizes
newer sessions under the global bound, preserves representative middle action
turns, and reserves text space for workflow evidence; older corroboration is
still retained when capacity permits.

Successful publication renders Shortlist, Created or extended, Skipped, and
Needs more evidence sections. Empty creation is terminal success. Direct
`/distill run` now inventories supported assets and keeps its existing
Distill-only authority. Focused tests and both workspace matrices pass (2,536
default; 2,684 all-target/all-feature), as do formatting, strict Clippy, the
all-feature release build, and hygiene checks. No live provider, credential,
real profile/session/config state, service, installed binary, oracle, or
reference content was touched. The completed plan is
`docs/exec-plans/completed/LRN-170-mimocode-distillation-parity.md`.

## LRN-169 zero-jitter TUI, copyable sessions, and streamlined learning complete (2026-08-24)

Streaming responses now render one constant-width cursor at the final response
cell. Blink changes only that cell's style, so wrapped row count, transcript
maximum, composer boundary, visible top row, bottom following, and scrolled-up
anchoring stay stable. Wide/narrow, Markdown, resize, new-delta, and exact
screenshot-response regressions pass. A disposable 100x30 native PTY also kept
the four wrapped transcript rows untouched throughout a five-second Thinking
interval and updated only one cursor cell while streaming.

`/learn approve` now resolves the currently priced evidence revision inside the
TUI. Generation-tagged Dream/Distill progress advances from `0/N` through
`N/N`; closing and reopening the pager neither cancels nor duplicates work. A
separate strict owner-only `progress.json` atomically checkpoints validated
calls and binds them to evidence, route, model, price, and materialized plan.
Resume runs only missing calls, drift refuses reuse, success removes the
checkpoint, and known `evidence_roots` parse failures identify installed
component version skew without weakening the schema.

`/copy`, `/copy last`, `/copy all`, and default `Ctrl+X C` use the in-process
clipboard boundary. Copy-all hydrates saved history and projects only the
privacy-safe TUI-visible session, including active streaming and displayed
tool/diff/reasoning content while excluding system, hidden, provider-only, and
transient UI state. Transcript and pager selections retain forward/reverse
Unicode ranges across wheel and edge scrolling, enforce the 200,000-cell cap,
and cancel on incompatible geometry. Distinct successful byte-changing edits
now reset generic low-yield detection without weakening repeated-edit or
failure detection.

Focused suites, both workspace matrices (2,532 default and 2,679
all-target/all-feature tests), formatting, strict Clippy, all-feature release
build, snapshots, smokes, and diff/protected-tree hygiene pass. No live
provider, credentials, real learning/session/config state, installed binary,
oracle, or reference content was touched. The completed plan is
`docs/exec-plans/completed/LRN-169-in-tui-learning-approval-progress.md`.

## LRN-168 guided, copyable learning review complete (2026-08-23)

`/learn` now opens a responsive status page whose Enter action starts the
manual review, while `/learn review` opens it directly. Registered sources are
refreshed off the TUI event loop, and exact evidence/proposal digests remain in
typed process state. Evidence confirmation shows the message count, maximum
provider calls, route, and estimated cost; success advances to a separate
Later-first proposal decision with Apply all, Memory only, Skills only, and
Reject all. Partial application keeps the remainder inert and displays its new
exact digest.

Evidence or pricing drift never inherits approval. One automatic rescan and
re-price produces a fresh unapproved view at the top; repeated drift stops with
a visible refresh action. Pending proposals load before source sync, provider
failures retain the exact evidence for retry/reject/later, stale task results
are ignored, and completion after pager close becomes a notice without
reopening over unrelated work.

The shared pager now copies the rendered visible selection on mouse release and
copies its complete underlying content with C, including off-screen lines.
Borders, hints, and hidden conversation text are excluded; clipboard failure
retains selection and reports the failure. Focused learning, orchestration,
input, render, and reviewed wide/narrow snapshot suites pass. The final
workspace suite passes 2,517 tests with three documented ignores; formatting,
strict all-target/all-feature Clippy, the all-feature release build, and
diff/snapshot/protected-tree hygiene pass. Background learning remains disabled,
and no runtime profile, provider, service, installed binary, oracle, or
reference content was touched.

## SES-165 understandable session names complete (2026-08-23)

New unnamed persistent sessions keep the immediate bounded local first-request
fallback, then receive one concise 3–8 word model-generated title after the
first provider response. The title request is asynchronous, tool-free,
separately traced/accounted, and best-effort: it never delays or fails the main
turn, and a provider failure leaves the local fallback intact. Process-local
session ID and generation checks prevent late results from overwriting an
explicit `--name`, `/name`, resumed session, or different conversation.

Existing active and archived sessions remain unchanged until the user presses
Ctrl+G on a highlighted row. That action loads the exact full session off the
TUI thread, generates one title, persists it through the existing storage-key
plus UUID-checked atomic rename boundary, and refreshes both lists. Prompt-like,
path-like, truncated, and untitled rows display a repair hint. Merely opening,
searching, or navigating `/sessions` remains read-only and provider-free.

The browser now calls former active sessions `Recent`, calls the second tab
`Archived`, always explains search, and spells out open/rename/generate/archive/
delete/navigation controls in wide and narrow layouts. Focused fake-provider,
active/archive persistence, stale-result, input, and render tests pass. The
final workspace suite passed 2,492 tests with three documented ignores;
formatting, strict all-target Clippy, release build, snapshot/diff hygiene, and
help/version smokes pass. No live provider, credentials, real runtime session,
configuration, installed binary, oracle, or reference content was touched.

## SES-164 named session browser complete (2026-08-23)

`/sessions` now presents saved conversations as names instead of storage
records. The wide, borderless browser groups projected former sessions by the
user's local calendar date, labels the current day `Today`, keeps one title per
row, and right-aligns the updated time. Public IDs, storage keys, and workspace
paths remain searchable but no longer consume a second display row. Active and
Archive tabs, pin/archive/restore/delete behavior, former-only projection, and
all orchestration safety checks remain intact.

Ctrl+R opens a bounded inline rename editor for the highlighted active or
archived session. Submit runs an exact storage-key plus UUID-checked atomic
metadata update on the existing blocking session-management worker; empty,
overlong, and control-bearing names fail closed. The initial offline title
heuristic now prefers an explicit `My request:` envelope and removes control
characters, so attachment declarations and path boilerplate do not displace
the user's actual request. Explicit `--name` and `/name` remain authoritative.

The read-only MiMoCode oracle supplied behavioral evidence for title-first date
grouping, updated-time display, and rename; implementation remains native and
does not claim Cursor parity. Focused naming/store/input/projection/render tests,
reviewed wide/narrow snapshots, 2,484 workspace tests with three documented
ignores, formatting, strict all-target Clippy, release build, diff/snapshot
hygiene, and help/version smokes pass. Validation used temporary stores only;
no provider request, runtime config/session mutation, installed-binary change,
or oracle/reference write occurred. The completion record is
`docs/exec-plans/completed/SES-164-named-session-browser.md`.

## SES-163 primary sessions browser complete (2026-08-23)

`/sessions [session-id]` is now the canonical saved-conversation browser and
direct-load command. `/resume` remains compatible and `/continue` is an
additional alias. The singular `/session` current-conversation/process-sitting
report remains available while busy, but `/sessions` now occupies its former
common-command discovery position.

The Active picker projection excludes the current session UUID before counts,
query filtering, navigation, selection, and pin/archive/delete action mapping.
Archive rows remain available, so an archive-only history still opens with an
Active count of zero; only-current history reports `No previous sessions`.
Storage, profile isolation, lazy-history loading, startup flags, and the
orchestration-level current-session mutation guards are unchanged. MiMoCode's
read-only `/sessions` vocabulary and selection interaction informed this `ca`
extension; no oracle code was copied and no Cursor parity is claimed.

Focused command, orchestration, picker, session, and render suites pass, with
reviewed wide/narrow command and session snapshots. The final workspace suite
passed 2,478 tests with three documented ignores. Formatting, strict
workspace/all-target Clippy, release workspace build, diff/snapshot hygiene,
and release help/version smokes pass. No real session/config/credential file,
provider, installed binary, oracle, or reference content was touched. The
completed plan is
`docs/exec-plans/completed/SES-163-primary-sessions-browser.md`.

## REL-162 low-friction bounded autonomy complete (2026-08-23)

Default AutoReview now runs prepared local built-ins without repetitive
approval prompts, including workspace mutations, local shell commands,
asynchronous child launches, and owned job await/stdin/kill operations. Strict
Ask mode remains prompt-first for execution work, and persistent Ask/Deny,
network, outbound
communication, privilege, browser control, explicit skill Ask/Deny overrides,
and unknown or dynamic tools remain explicit boundaries.

Canonical typed reads and ordinary sandboxed shell reads may inspect requested
host paths outside the workspace while creates and mutations remain confined
to writable workspace roots. Each shell job advertises its private temporary
directory through `TMPDIR`, `TMP`, and `TEMP`; conventional `/tmp` writes also
work. Late sandbox denials keep `ca-privilege-*` handoff paths inaccessible. A
native macOS regression proves an ordinary command can copy an external file
into the workspace and use `/tmp`, but cannot create a sibling external file.
Supervised role-run remains narrower:
its exact workspace grants now reject a swapped final symlink during no-follow
preflight, and all hardlink/symlink security tests pass.

Named-agent calls now advertise `explore`, `plan`, and `general` and reject an
unknown name before approval. Spawned children default mutating path scopes to
the workspace roots when no narrower assignment is supplied, while read-only
typed tools retain external-read access. Provider headers/first-event windows
are 60 seconds and no-progress is 120 seconds; retry and cancellation limits
are unchanged.

The final default workspace suite passed 2,472 tests with three documented
ignores. Formatting, strict workspace/all-target Clippy, release workspace
build, and diff hygiene pass. The only recurring diagnostic is the existing
macOS debug-linker compact-unwind warning. No live provider, credentials,
runtime config/sessions, installed binary, oracle, or reference content was
touched. The completed plan is
`docs/exec-plans/completed/REL-162-low-friction-bounded-autonomy.md`.

## TUI-161 readable slash-command descriptions complete (2026-08-23)

The inline slash-command palette now expands to the terminal's available width
instead of stopping at 74 columns. On wide terminals, command syntax is capped
at a 32-cell scan column so descriptions receive the remaining space; narrow
terminals retain the existing two-line stacked presentation. At 100 columns,
the common session and process-usage descriptions render in full.

The focused readability regression, all 166 deterministic render tests, the
complete workspace test suite, formatting, strict workspace Clippy, release
workspace build, diff hygiene, and snapshot hygiene pass. The three affected
wide snapshots were reviewed against the user-provided clipped-description
screenshot. No oracle/reference files, provider traffic, credentials, runtime
configuration, saved sessions, or installed binary were touched.

## REL-156/AGT-157/SEC-158/TUI-159 integration review (2026-08-23)

The product checkout is locally functional but not release-ready. HEAD remains
`855ca635a4d8` on local `master` with no configured remote/upstream, 508 tracked
changes, and 355 exact untracked paths. The pinned Rust 1.97.1 root workflow
and `codex-team.release-readiness.v1` audit are implemented. Repeated unchanged
audits are deterministic and correctly refuse readiness for active-plan
tracking consistency, a dirty working tree, and untracked workspace/release
inputs. No cleanup, commit, remote workflow, tag, publication, or installed
binary change was authorized.

AGT-157 M1 now requires a valid host-created, revision-bound Pass receipt to
complete graph work or release dependents. Assistant-only output, legacy
completion, Fail/Inconclusive receipts, stale receipts, and tampered restored
receipts remain non-authoritative. Focused agent-graph, work, graph-tick, and
task-graph tests pass. Deterministic artifact/workspace-lane contracts and the
single monotonic physical-request/token/cost/tool/child/output budget remain
M2/M3.

SEC-158 M1 is fake-tested for the main interactive macOS Agent mode. It clears
the sudo environment; binds the exact one-use command, executable identities,
cwd inode, timeout, owner, nonce, and system-wide filesystem authority; accepts
one masked keyboard password; sends it through a mode-0600 FIFO inside a
mode-0700 directory; hides that directory and other processes from the
sandboxed target; closes target stdin; denies target network; and runs fixed
`sudo -k`, direct `sudo -A -p <fifo> -k -H -- ...`, then final `sudo -k`.
Targets are limited to a closed set of root-owned native foreground utilities.
Cancellation gives sudo a bounded catchable TERM relay window so a separate
PTY monitor/session can reap its command before forced cleanup; indeterminate
cleanup latches privilege unavailable. Ask, Debug, Plan, Custom, workflow,
headless, service, schedule, subagent, Linux, and FreeBSD surfaces fail closed.
No real sudo process or password was used, so production password acceptance
remains unverified. The public-tool rename, separate network approval, and
authenticated WebSocket service v2 remain open.

TUI-159 is complete. Markdown links now contribute only their label to layout,
linked inline code retains styling, and table rendering preserves arbitrary
columns and graphemes through bounded grid/stacked layouts. All 17 focused
Markdown tests pass. The broader TUI remains snapshot-rich, but native PTY
startup/paste/resize/suspend/cleanup and Cursor parity are still unverified.

Final local default tests passed 2,469 checks with three ignored; strict
all-feature Clippy, formatting, all-feature release build, and binary smokes
pass. The final all-target/all-feature matrix passed every `ca`, experiment,
CLI, integration, and privilege test, but one pre-existing `codex-team`
stateful-clean-filter assertion failed once under the parallel matrix and
passed immediately in isolation; REL-160 records the flaky exact-count
assumption. No provider traffic, credential access, runtime-state mutation, or
oracle/reference write occurred.

## SEC-155 Linux and FreeBSD privilege handoff complete (2026-08-23)

Historical milestone record: SEC-158 supersedes its runtime availability.
Current production privilege preparation requires macOS Seatbelt and therefore
fails closed on Linux and FreeBSD even if policy-only/off was previously
selected.

SEC-154's host-owned `run_privileged` protocol now supports the main
interactive Linux and FreeBSD TUI in addition to macOS. Helper selection is
fixed by host: `/usr/bin/sudo` on macOS/Linux and `/usr/local/bin/sudo` on
FreeBSD. The existing canonical root ownership, setuid, non-writable ancestry,
exact digest, one-use approval, masked authentication, private process-group,
closed-stdin, network-denial, and streaming byte-redaction invariants are
unchanged. Readable targets/helpers must be native Mach-O on macOS or ELF on
Linux/FreeBSD; interpreters, launchers, and nested elevation remain rejected.

The repository has no mandatory sandbox backend outside macOS. Linux and
FreeBSD therefore reject `required` during preparation, before approval,
hooks, cached-auth checks, or secret input. They run only when the user has
explicitly selected the existing `policy-only` or `off` mode; no automatic
sandbox downgrade was added. All unattended/delegated surfaces remain closed.

Local fake-sudo, platform-contract, registry, policy/order, and full workspace
tests pass without a credential. After explicit user authorization, a bounded
FreeBSD 15.1 amd64 probe verified root-owned mode-4755 ELF
`/usr/local/bin/sudo`, root-owned non-writable ancestry, and ELF `/usr/bin/id`.
The detached, stdin-closed `sudo -n -v` check was denied by host policy for user
`aji`, so no privileged target ran and no password was requested. A workspace
build also encountered a pre-existing `codex-team` FreeBSD `setpriority`
type error outside SEC-155. Per user direction, no wider FreeBSD portability
work was attempted; disposable source directories were removed.
The completed plan is
`docs/exec-plans/completed/SEC-155-linux-freebsd-privilege-handoff.md`.

## SEC-154 safe interactive macOS privilege handoff complete (2026-08-23)

Historical milestone record: SEC-158 supersedes the `sudo -n` execution and
cleanup details below with the direct askpass supervisor described above.

The main interactive macOS TUI now conditionally exposes a closed
`run_privileged { program, args, cwd? }` tool after validating the fixed
`/usr/bin/sudo` helper. Each request is bounded, rejects control characters and
dispatcher/interpreter/elevation programs, and requires a canonical native
executable with root-owned non-writable identity and ancestry. A versioned
digest binds the exact program identity, ordered argv, cwd, sandbox mode/roots,
helper identity, and process-only nonce; the private handle also rejects owner
mismatches. Explicit deny rules are ceilings;
allows, grants, AutoReview, Yolo, and `run_command` permissions cannot approve
elevation.

Approval, cached-auth checking, optional one-attempt masked authentication, and
target execution are separate host-owned stages. Masked input is keyboard-only,
bounded to 1 KiB, byte-backed, zeroizing, and absent from model, composer,
history, hook, session, trace, and learning inputs. Authentication uses a
detached private helper with streaming raw-byte redaction; the target uses the
existing direct-process sandbox preparation through `sudo -n`, with stdin
closed, network denied, the normal timeout, and detached process-group cleanup.
No privilege process is registered as a generic job. Unsupported or failed
capability surfaces omit or reject the tool, including headless, service,
scheduled, Plan-mode, subagent, and non-macOS execution.

All privilege tests use an injected fake-sudo fixture; no real sudo command or
credential was used. Focused privilege/policy/interaction/hook/surface/render
tests and reviewed wide/narrow snapshots pass. The complete default and
all-target/all-feature workspace suites, formatting, strict all-feature Clippy,
all-feature release build, `ca`/`codex-team` help/version smokes, diff and
snapshot hygiene, and protected oracle/reference checks pass. The completed
plan is
`docs/exec-plans/completed/SEC-154-safe-interactive-privilege-handoff.md`.

## CON-153 one-project monorepo consolidation complete (2026-08-22)

`/Users/aji/project/ca` is now the only active project directory for this
product family. The root Cargo workspace contains the `ca` package and the
embedded `crates/codex-team` package with one shared lockfile and build tree.
The August 20–22 role-run, TUI, delegation, recipe-cockpit, and supervised
learning work was reconciled onto the newer canonical `ca` tree without
discarding its later prompt-cache and quiet-TUI behavior.

All former tips are preserved as local archive refs:
`archive/ca-role-run-integration`, `archive/ca-tui-polish`,
`archive/codex-team-main`, and `archive/codex-team-ca-integration`. The exact
pre-merge canonical tree is `archive/consolidation/ca-working-source-20260822`.
Source-only recovery archives for the original canonical tree, the dirty
role-run checkout, and the untracked orchestrator plan live under
`.git/consolidation-backups/`. The two source checkouts containing recoverable
local state were moved to
`/Users/aji/.Trash/ca-consolidation-20260822-145511`; the two clean linked
worktrees were retired through Git.

Workspace formatting, strict all-target Clippy, the complete `ca` and
`codex-team` test suites, release builds, help/version smokes, diff hygiene,
and snapshot hygiene passed before generated output was cleaned. The combined
results were 1,797 passing `ca` unit tests, 55 `ca` integration tests, 540
passing `codex-team` library tests, and 10 `codex-team` integration tests;
three documented manual/pilot tests remain ignored. No provider traffic,
credential access, runtime profile/session mutation, installed-binary change,
or oracle/reference mutation occurred.

## OPT-152 hosted API context efficiency complete (2026-08-22)

The stable system prefix now tells repository agents to identify likely
symbols, prefer narrow targeted ranges, request condensed failures before raw
logs, and batch independent read-only searches/reads into one provider turn.
The text is deterministic and contains no turn/request value. Checkpoint
generation uses canonical Task, Constraints, Facts, Hypotheses, Files Touched,
Decisions, Test State, and Open Issues sections and explicitly preserves
uncertainty rather than promoting it to fact.

The existing opt-in `agent.command_output = "semantic"` policy now condenses
eligible proven Git/Cargo command results for model continuations in
interactive, headless, and service runs. Default bounded output and unsupported
commands remain byte-identical; human events stay truthful, and the same
process retains bounded raw capture for expansion. `/request` reports the
declared stable-prefix message/token boundary without its private identity or a
cache-hit claim. `/usage` and `/session` report exact-known new input/cache hit
ratio, tool-continuation provider round trips, and average total/first-observed-
output latency, falling back to unknown when provider detail is incomplete.

Focused suites and reviewed deterministic snapshots pass. Complete default
results are 1,731 main tests and 38 CLI tests with one ignored; complete
all-target/all-feature results are 1,760 main, 118 experiment, and 38 CLI tests
with two ignored. Formatting, strict Clippy, all-feature release, release
help/version, diff/snapshot hygiene, and oracle/reference worktree checks pass.
No credentials, live provider traffic, runtime config/session mutation, binary
installation, or oracle/reference content access occurred. Concurrent tool
execution remains deliberately deferred because approval and mutation ordering
need a separate safety contract. The completed plan is
`docs/exec-plans/completed/OPT-152-hosted-api-context-efficiency.md`.

## PRV-151 model-scoped prompt cache contracts complete (2026-08-22)

Prompt caching is now resolved per exact endpoint/protocol/model contract,
separate from model catalogs, Responses continuation state, and local GGUF KV
memory. Direct official OpenAI Responses routes support automatic,
stable-prefix, and off policies; Codex OAuth, MiMo, and DeepSeek remain
provider-managed and receive no undocumented cache fields. Exact custom model
declarations use closed request/usage/policy enums, validate before credentials,
and preserve unrelated TOML/comments through targeted rename/delete edits.

One pseudonymous conversation scope and frozen logical-turn envelope now span
the main call, transport retry, fallback route, and tool continuations. The
private key binds exact route/model, contract/layout versions, session epoch,
and canonical tools; resume reproduces it, while branch, subdomain, route, or
prompt/tool changes rotate it. Session-epoch, append-only, logical-turn, and
request-only segments keep memory, task skills, hooks, attachments, and controls
after the sole explicit breakpoint. Checkpoint summaries reach the wire once,
carry versioned source digests, and activate only between logical turns.

Cache usage is canonicalized as reported/derived/unknown read, write, and miss
counts with provider-dialect validation. Pricing distinguishes replacement,
surcharge, and included cache writes, with in-memory catalog migration and
unchanged external JSON. `/cache provider/model` and Shift+C expose a dedicated
wide/narrow-tested model cache screen; built-in formats are read-only and the
legacy provider row is retained only as a direct-OpenAI fallback.

Focused suites pass. Complete default results are 1,728 main tests and 38 CLI
tests with one ignored; all-target/all-feature results are 1,757 main, 118
experiment, and 38 CLI tests with two ignored. Formatting, strict Clippy,
all-feature release, release help/version, diff/snapshot hygiene, and
oracle/reference immutability pass. No credentials, live provider traffic,
runtime config/session mutation, binary installation, or oracle/reference
mutation occurred. The completed plan is
`docs/exec-plans/completed/PRV-151-model-scoped-prompt-cache-contracts.md`.

## LRN-167 manual MiMoCode session learning complete (2026-08-23)

Manual `ca learn run` now auto-discovers
`~/.local/share/mimocode/mimocode.db` beside the Codex active/archive roots.
The importer uses a read-only, query-only SQLite snapshot and accepts only
completed main-slice user/final-assistant text. Tool-call assistant steps,
reasoning, tools, files, synthetic/ignored text, incomplete and errored turns,
secret-shaped content, and sessions already marked as foreign imports are
excluded. Workspace directories and parent sessions feed the existing
partition/fork-dedup boundaries; content-bound IDs keep repeated runs stable.

Persistent archive-learning state contains no MiMo transcript bodies, and no
MiMo session is imported into `/sessions`. Adding the MiMo source now preserves
already indexed Codex evidence rather than replacing the current projection.
A read-only preview against the live database observed 114 session records,
567 completed candidate messages, and two secret-shaped skips without profile
writes or provider calls. The background LaunchAgent remains uninstalled and
disabled by explicit user choice. A future explicit service preview includes
the MiMo database as a fifth read-only evidence root when it exists, so manual
registration cannot create later sandbox/enrollment drift.

Focused scanner, registration, fork-dedup, combined native/Codex/MiMo harvest,
knowledge-enrollment, sandbox, and reviewed status-render tests pass. The full
workspace passed 2,510 tests with three manual-only ignores; format, strict
all-target/all-feature Clippy, all-feature release, help/version, diff and
snapshot hygiene pass. `launchctl` confirms the service remains unloaded.

## LRN-166 autonomous learning loop implemented; background activation deferred (2026-08-23)

`ca learn` is now the unified continuous-learning interface: `status`, `run`,
`pause`, `resume`, and `service preview|install|status|uninstall` sit beside the
existing manual source/review/proposal compatibility commands. `/learn` renders
the combined archive, Dream/Distill, and stable-supervisor status. The service
preview binds the default evidence profile, native active/archive sessions,
Codex active/archive JSONL, an isolated required-sandbox execution profile,
the three exact release binaries, a 15-minute harvest cadence, first-due
7/30-day Dream/Distill work, unbounded metered provider use, and
memory/inert-skill-only publication.

The stable `com.codex-team.self-improvement` supervisor is the sole recurring
service. It harvests locally, defers provider work behind foreground leases,
health-checks before each stage, and safety-pauses on corrupt/drifted state,
failed health, unavailable roots/routes, or low disk. Knowledge-only enrollment
does not require a clean checkout and grants no source, binary, routing,
permission, provider-setting, role-prompt, executable, plugin, dependency, or
ordinary-session mutation authority. Literal credentials, MCP commands, hooks,
and approval rules are excluded from the isolated profile; credential
references remain external.

Bridge completion is digest-bound and idempotent. Imported evidence stays
queued until a terminal replay rejection or successful publication through the
existing Dream/Distill journals, conflict limits, generations, and undo path.
Native and Codex histories reuse completed-turn, secret-removal, bounds,
workspace partitioning, and stable deduplication rules, and foreign transcripts
never enter `/sessions`. The full workspace suite passed 2,500 tests with three
manual-only ignores; formatting, strict all-target/all-feature Clippy, release,
help/version, and diff hygiene pass. The release activation digest is
`1aae1e531b984a353c8221a94bd14b8d8f7b87a6316dbd83e4fc35adde2e1bdd`.
No service, execution profile, provider call, memory, or skill was activated by
this validation. The first confirmation failed closed before mutation when it
exposed nondeterministic provider-map serialization; ordered serialization is
now regression-tested and two release previews agree. The user subsequently
chose manual learning, so the service remains unloaded and the old four-root
activation digest is historical. If background learning is requested later, a
fresh preview must bind the optional MiMoCode database as a fifth evidence root
before activation or a live workflow smoke.

## LRN-150 reviewed Codex archive learning validated; activation approvals pending (2026-08-22)

`ca learn` now owns explicit source preview/add/list/remove, local incremental
sync/status, evidence review, inert generated proposal review/application, and
macOS service lifecycle. The streaming adapter enforces the 10,000-file/1-GiB,
64-MiB-file, 1-MiB-line, depth-64, and 8-KiB-message bounds. It uses first
metadata identity, completed user/final-answer turns, secret/control filtering,
workspace/root-thread relationships, and response/content deduplication. Its
owner-only state contains no transcript bodies and never populates ordinary
sessions.

Evidence approval binds exact source revisions, selected IDs, workspace
partitions, bounded calls, route, and aggregate forecast. The provider sees
workspace digests rather than paths; generated Dream/Distill output remains
inert for a separate exact proposal review. Exact per-memory/per-skill item
digests support selective apply/reject through existing journals/conflict/undo.
`/learn`, `/learn sync`, and `/learn review` expose the same local manager in
the TUI pager.

The direct no-shell LaunchAgent is exact-confirmed and background work re-execs
under required macOS sandboxing with a clean environment, no network, one
writable index root, and read-only registered archives. Focused tests currently
pass, including fake-provider prompt privacy and fake-`launchctl` lifecycle.
All focused/default/all-feature, format, strict-Clippy, release, smoke,
wide/narrow snapshot, diff, and protected-tree gates pass. The final real
preview observed 93 files / 504,570,420 bytes / 1,145 completed candidates
after four live session files appeared since the initial 89-file inventory;
profile state remained absent. Binary/source-service/evidence/proposal
activation approvals remain pending in the active LRN-150 plan.

## REL-149 reliable tool execution and scoped consent complete (2026-08-22)

Unified patching now enforces the existing 1 MiB mutation safety bound,
validates complete contextual/multi-hunk diffs, uniquely relocates only exact
old images, preserves LF/CRLF and EOF newline state, and
leaves files/undo history unchanged on bounded failures. Repeated identical
mutation failures participate in the existing loop guard. Skill activation and
resource-view schemas come from the effective registry, invalid selectors fail
before approval, and LSP schemas require a real manager. No-server or
extension-mismatch post-edit diagnostics are silent while configured reports,
timeouts, stale results, and failure status remain visible.

Prepared approval calls remain ordered and are reclassified against live rules,
exact current-run grants, and approval mode before display. Same-file grants
cover later queued edits; other files still prompt. Command grants include the
exact command, canonical cwd, background, normalized arguments, and network
request, including an explicitly approved compound shell. Ask, persistent deny,
network, outbound communication, and one-shot allow/deny retain precedence and
isolation. The misleading `d` choice is removed and approval cards state reason
and scope.

Ordinary macOS jobs can traverse `/etc` metadata to `/private/etc`, resolve
validated system/Homebrew/Cargo/Rustup/Xcode executables, and run literal Cargo
stages against read-only registry cache data in offline mode. Cargo credentials,
config, Git caches, unrelated home paths, arbitrary PATH roots, and writable
caches are not granted. Effective prepared network intent reaches execution;
`read_lints` is managed, approval-requiring, and absent from automatic safe
children. Required sandboxing and FreeBSD behavior remain fail closed.

Focused suites and all 74 approval tests pass. Complete default results are
1,671 main tests and 37 CLI tests with one documented ignore. Complete
all-target/all-feature results are 1,700 main tests, 118 experiment tests, and
37 CLI tests with two documented ignores. Formatting, strict Clippy,
all-feature release, release help/version, diff/snapshot hygiene, and
oracle/reference immutability pass. The completed plan is
`docs/exec-plans/completed/REL-149-reliable-tools-scoped-consent.md`.

## TUI-148 fx-inspired quiet default complete (2026-08-22)

Idle startup now renders only a compact context title, a borderless composer,
and `/ commands`. The duplicate welcome/workspace text, shortcut legend, and
zero-message label are gone. Configured model, mode, and profile fields appear
in either the title or status line, not both. Active queue, interrupt, scroll,
approval, failure, overlay, and transcript-detail states retain their existing
contextual presentation.

Empty slash discovery now leads with Model, Session, Permissions, Diff, Review,
Context, Usage, Undo, Clear, and Help, followed by the complete remaining
visible registry. Filtered search and command identities are unchanged. All 391
focused TUI tests pass. Complete default results are 1,652 passed main tests and
37 CLI tests with one documented ignore; all-target/all-feature results are
1,681 passed main tests, 118 passed experiment tests, and 37 CLI tests with two
documented ignores. Formatting, strict Clippy, all-feature release, help/version,
diff/snapshot hygiene, and oracle/reference immutability pass. No runtime state,
credentials, installed binary, provider traffic, or oracle/reference content was
accessed or changed.

## REL-147 token and memory retrieval repair complete (2026-08-19)

Automatic checkpoint control no longer relies on raw UTF-8 bytes divided by
four. Exact official OpenAI/Codex, MiMo, and DeepSeek endpoint/protocol/model
contracts select distinct estimator identities; custom compatible endpoints
remain generic. The current implementations are explicitly conservative and
network-free rather than falsely claiming exact local tokenization. They never
return less than the old heuristic and weight punctuation, long identifiers,
Unicode, framing, and attachments more safely. Primary and fallback prepared
attempts recompute the complete category breakdown with their own route.

Prompt-facing memory recall is now selected locally for each current request,
without changing stored order or schema. User and workspace scopes retain
independent 25 KiB/200-line limits. A small deterministic reserve retains broad
preferences/conventions, while positive BM25 matches fill the remaining budget
whole-record-at-a-time. Query-blind external facts are excluded. Interactive,
headless, side-question, and service paths use the query-aware projection;
approved in-process mutation refresh remains intact, while external file edits
are still not watched mid-session.

Focused context (24), memory (13), and service (22) suites pass, along with
headless-adjacent prompt, mutation, side-question, fallback, and reviewed wide/
narrow context-dashboard coverage. Complete default results are 1,651 main
tests and 37 CLI tests with one documented ignore. Complete all-target/
all-feature results are 1,680 main tests, 118 experiment tests, and 37 CLI
tests with two documented ignores. Formatting, strict all-feature Clippy,
all-feature release, help/version, diff/snapshot hygiene, and oracle/reference
immutability pass. No provider/tokenizer traffic, credential inspection,
runtime-state mutation, installation, native TUI session, or oracle/reference
access ran.

## PRV-146 first-party provider support complete (2026-08-19)

Official OpenAI API/Codex OAuth, MiMo PAYG/Token Plan, and DeepSeek behavior is
now selected by exact endpoint, protocol, and model. Built-in current-model
profiles cover context, tools, image support, and truthful reasoning choices;
user profiles and legacy context overrides retain precedence, while named
custom proxies remain generic. New MiMo definitions prefer Responses,
DeepSeek remains Chat-first with a certified Responses alternative, and saved
configurations are never rewritten.

Request generation, decoding, continuation replay, and usage validation now
follow the resolved dialect. Direct OpenAI Responses sends `store: false`,
replays complete output/encrypted-reasoning state, and supports implicit,
stable-prefix, or off prompt-cache policy with a private hashed session key.
MiMo and DeepSeek serialize their own thinking/effort contracts and exclude
foreign OpenAI fields. The provider editor exposes cache policy only for exact
direct OpenAI Responses routes.

Pricing catalog v2 remains backward-compatible while scoping rates by endpoint
and billing mode. It adds OpenAI Terra and MiMo PAYG, preserves MiMo Token Plan
credits and DeepSeek hit/miss accounting, and never assigns direct-API pricing
to Codex subscription routes or unofficial endpoints. The explicit repeatable
`ca --doctor --connect --provider-contract openai|mimo|deepseek` verifier uses
at most two no-retry requests per route, a synthetic local tool only, transient
runtime authentication, sanitized versioned output, and no session/ledger
writes.

Focused suites pass. Complete default results are 1,645 main tests and 37 CLI
tests with one documented ignore. Complete all-target/all-feature results are
1,674 main tests, 118 experiment tests, and 37 CLI tests with two documented
ignores. Formatting, strict Clippy, all-feature release, help/version,
diff/snapshot hygiene, and oracle/reference immutability pass. No live provider
verifier, credential inspection, runtime-state mutation, installation, native
TUI session, or oracle/reference access ran; fake credentials and loopback
transport are the acceptance evidence.

## TUI-145 simultaneous trackpad scroll and text selection complete (2026-08-19)

Default Scroll mode now supports both behaviors that previously conflicted:
wheel/trackpad events retain focused navigation, while ordinary left-button
down/drag/up creates a visible application-owned selection over the current
conversation cells. Release copies the exact bounded visible text through the
existing in-process clipboard dependency. A failed clipboard write is visible
and leaves the highlight intact; no shell clipboard command runs.

The selection snapshot is process-only, capped at 200,000 cells, and excludes
hidden transcript history and canonical session messages. Extraction supports
forward/reverse multiline drags and Unicode wide-cell continuations, trims
unselected row padding, clamps a drag that leaves the conversation, and
ignores starts outside it. Wheel or keyboard activity clears stale selection.
`mouse = "off"` remains the native terminal-owned alternative; no persisted
configuration shape or default changed.

All 9 lifecycle, 5 transcript, 163 input, and 158 render tests pass. Complete
default results are 1,624 passed main tests and 37 CLI tests with one documented
ignore. Complete all-target/all-feature results are 1,653 passed main tests,
118 experiment tests, and 37 CLI tests with two documented ignores. Formatting,
strict Clippy, all-feature release, help/version, diff/snapshot hygiene, and
oracle/reference immutability pass. No native TUI session, clipboard read,
runtime-state mutation, installation, provider traffic, or oracle/reference
access ran.

## AGT-144 automatic safe agent launches complete (2026-08-19)

AutoReview now admits only asynchronous `spawn_agent` calls whose nonempty
tool grants are registered read-only subagent tools without network,
external-communication, or nested-agent capability and whose command grant is
absent or empty. Persistent Deny/Ask rules still win, strict Allowlist and
synchronous `task` still prompt, and unknown, empty, write, shell, web,
communication, nested-agent, and command grants remain gated.

The model-facing `task` and `spawn_agent` contracts now state that omitted
complexity inherits the active provider/model and that explicit complexity is
reserved for user-requested inter-provider cost routing. Explicit selection
still fails closed without parent fallback. Route tables now store provider
specifications rather than clients. At child start, Codex routes resolve the
canonical runtime provider under a serialized gate before a transient client
is constructed; non-Codex routes bypass runtime OAuth. Fake/loopback coverage
proves resolution precedes provider traffic, auth failure records no request or
usage, and runtime credentials stay out of diagnostic surfaces.

The transcript projection groups only contiguous completed `spawn_agent`
calls with byte-identical errors. The first call ID owns one expandable card,
whose collapsed form shows the count and one body and whose expanded form lists
every task in order. Canonical calls, IDs, outputs, statuses, sessions, hooks,
and provider continuation messages remain individual.

Focused permission, registry, 16-test subagent, fake OAuth, routing, and all
157 render tests pass. Complete default results are 1,619 passed main tests and
37 CLI tests with one documented ignore. Complete all-target/all-feature
results are 1,648 passed main tests, 118 experiment tests, and 37 CLI tests with
two documented ignores. Formatting, strict Clippy, all-feature release,
help/version, diff/snapshot hygiene, and oracle/reference immutability pass.
No live OAuth, paid provider request, runtime-state mutation, installation, or
native TUI session ran; deterministic loopback and wide/narrow snapshots are
the acceptance evidence.

## REL-143 low-friction reads and credential migration complete (2026-08-18)

The exact legacy saved approval value `allowlist` now migrates, after complete
configuration validation, to a comment-preserving atomic `auto-review` patch.
Canonical `ask` remains the explicit strict mode, and `/auto-review` now saves
the selected `auto-review`/`ask` behavior outside bare profiles. Registry-
declared file/list/search reads and closed read-only shell pipelines within
declared workspace roots therefore skip approval in AutoReview. Explicit
Ask/Deny rules, writes, deletion, network access, unsafe shell composition,
unknown programs, and out-of-root paths retain their existing boundaries.

macOS direct-provider secrets now use profile-scoped private files as primary
storage. A missing private secret performs one legacy `ca.provider-api-key`
Keychain read and requires a safe atomic import before returning it; subsequent
loads do not consult Keychain. Direct saves/deletes affect only the primary,
leaving legacy items intact for rollback. Directory/file modes remain 0700/0600
with regular-file, ownership, no-follow, bounded-read, and redaction checks.
Codex OAuth and the email-gateway Keychain are unchanged.

The focused approval/configuration/credential/session-handoff regressions pass.
Complete default results are 1,610 passed main tests plus 37 CLI tests with one
documented ignore. Complete all-feature results are 1,639 passed main tests,
118 experiment tests, and 37 CLI tests with two documented ignores. Formatting,
strict all-target/all-feature Clippy, all-feature release, released help/version,
diff/snapshot hygiene, and oracle/reference immutability pass.

The validated release was atomically installed at `/Users/aji/.local/bin/ca`
with SHA-256 `a6be8911f5ef4317bc21257708886d482d5849da95d69946bd99bc5fc7a9036f`.
The live profile migrated to `auto-review`; both configured legacy credentials
were imported with verified 0600 metadata, and a second secret-resolving load
completed without another Keychain prompt. Offline doctor reports Ready and no
paid provider request ran. Recoverable pre-task backups are
`/Users/aji/.local/bin/ca.backup-rel143-20260818-211356` and
`/Users/aji/.config/ca/config.toml.backup-rel143-20260818-211356`; the
pre-hardening installed artifact is also retained at
`/Users/aji/.local/bin/ca.backup-rel143-prehardening-20260818-212102`.

## SES-142 exit session handoff complete (2026-08-18)

Normal interactive exits now print a compact post-terminal handoff for every
resumable, non-empty session: `Session` carries the sanitized bounded title and
`Continue` carries the exact copyable `ca -s ses_…` command. Empty sessions,
bare runs, and persistence-disabled profiles do not advertise continuation.
Headless/service output, session identity and storage, exit codes, and the
existing `/session` report are unchanged.

The handoff is returned from `run_tui` and printed by `run_standard`, after the
TUI owner has dropped and restored raw mode and the alternate screen. The
focused characterization failed before implementation and now passes, as do
session-report, short-resume, and public-ID regressions. Complete default
results are 1,602 passed main tests plus 37 CLI tests with one documented
ignore. Complete all-feature results are 1,631 passed main tests, 118
experiment tests, and 37 CLI tests with two documented ignores. Formatting,
strict all-target/all-feature Clippy, all-feature release, help/version,
diff/snapshot hygiene, and oracle/reference immutability pass. A native TUI
walkthrough was not run because safely isolating it would require runtime-state
authority outside this task; deterministic formatter coverage and the return
boundary provide the recorded evidence. No runtime state, installed binary,
oracle/reference file, or Git history changed.

## REL-141 practical read-only AutoReview complete (2026-08-18)

AutoReview now recognizes ordinary `find` discovery and stdout-only `sort`,
including the exact reported `find src -maxdepth 3 -type f | sort` pipeline.
Classification remains argument-sensitive: `find` execution, interactive
execution, deletion, and file-output actions are gated, as are `sort` output
files and compression helpers. The existing closed shell parser, background
refusal, unknown-program refusal, explicit Ask/Deny precedence, network policy,
and static mutating classification of general `run_command` remain unchanged.

The exact pipeline first failed its focused characterization, then passed after
the stage-specific repair. Ten permission tests, five focused AutoReview main
tests, one CLI policy test, and a real headless execution fixture pass. Complete
default results are 1,601 passed main tests plus 37 CLI tests with one documented
ignore. Complete all-feature results are 1,630 passed main tests, 118 experiment
tests, and 37 CLI tests with two documented ignores. Formatting, strict
all-target/all-feature Clippy, all-feature release, help/version, diff/snapshot
hygiene, and oracle/reference immutability pass. No runtime state, installed
binary, oracle/reference file, or Git history changed.

## SES-140 automatic session names and public identifiers complete (2026-08-18)

Interactive sessions now allocate their existing UUID-backed `SessionV4`
identity before the first provider turn. If no explicit `--name` or `/name`
exists, the first user request supplies an immediate local title: whitespace
and leading Markdown markers are normalized, appended attachment bodies are
excluded, Unicode graphemes remain intact, and the result is capped at 64
display cells. This adds no provider request, cost, or naming failure mode.

`session::public_session_id` reversibly encodes the same 128-bit UUID as a
fixed-width `ses_…` base62 identifier. The V4 schema, storage key, internal
UUID-based hooks/jobs/approvals, and saved-session compatibility are unchanged.
`ca -s <ses-id>` resolves through the existing UUID-checked store; UUID and
legacy-key resume remain accepted. Session filtering and responsive picker rows
show the public identifier, while `/session` prints the title, identifier,
internal UUID, and copyable `ca -s <ses-id>` command. Persistence-disabled runs
do not advertise a resumable command.

Focused round-trip/malformed/overflow, automatic-title, store-resolution,
short-CLI, early-identity, report, and no-persistence tests pass. All 155 render
tests, 1,600 default main tests, 37 default CLI tests, 1,629 all-feature main
tests, 118 experiment tests, and 37 all-feature CLI tests pass with the two
documented ignores. Formatting, strict Clippy, all-feature release,
help/version, diff/snapshot hygiene, and oracle/reference immutability pass. No
runtime session/config state, installed binary, oracle/reference file, or Git
history changed.

## TUI-139 white composer cursor blink complete (2026-08-18)

The focused composer now hides the terminal-owned cursor and applies a white
block style to the exact insertion cell during the visible half of a stable
500 ms application-owned blink. The alternate half restores the underlying
rendered cell without inserting marker text. Keyboard input and paste reset
the cycle to white; modal focus still suppresses the composer cursor. The same
Unicode/display-cell layout continues to own hard-line, soft-wrap, exact-width,
combining, and wide-glyph placement. The streaming marker shares the phase but
retains its existing glyph presentation.

Focused white/normal-phase and cadence tests pass, including empty input,
ASCII end-of-input, Unicode middle, wide glyphs, modal suppression, hard lines,
soft wraps, and exact-width wraps. All 155 render tests and the complete default
suite pass, as do 1,624 all-feature main tests, 118 experiment tests, and 37
CLI tests with two documented ignores. Formatting, strict all-target/all-feature
Clippy, the all-feature release build, released help/version, diff/snapshot
hygiene, and oracle/reference immutability pass. The prior TUI-137 native-cursor
record remains historical evidence; its presentation choice is superseded by
TUI-139. No session/config format, runtime state, installed binary,
oracle/reference file, or Git history changed.

## TUI-138 composer-first usability overhaul complete (2026-08-17)

The composer is now a seven-visual-row Unicode layout driven by grapheme and
display-cell boundaries rather than newline counts. Rendering, cursor
placement, Up/Down and preferred-column motion, hard-line Home/End, whole-draft
Ctrl+A/Ctrl+E, history boundary entry/restoration, and Vim `h/l/j/k/0/$` use
the same model. File/folder/MCP references and large pastes are registered
atomic ranges; images have a compact process-only row. Large-paste payloads
expand by range before send, steer, queue, history, stash, or editor handoff,
and image-bearing queue/stash requests fail without discarding the draft.

Slash, attachment, and one-level no-follow path results are now anchored above
the composer, retain the active token, reject stale generations, and use
explicit command Enter behavior. A shared Unicode-safe viewport/row layout
also supplies wraparound/page/jump navigation and narrow stacked descriptions
to model, session, provider-list, and permission pickers. Startup and title
chrome are compact; the footer adapts to typing, active work, completion, and
scrolled transcript states; empty-composer `?` opens the dynamic shortcut
browser. Queue previews say `After this turn`, show three plus overflow, mark
restored review-required entries, and support safe last-item editing.

Transcript scrolling is process-only and bottom-relative, measured from the
same wrapped Ratatui paragraph used for rendering. Page and top/bottom motion,
unseen counts, frozen scrolled output, follow restoration, and lazy-history
anchoring have deterministic coverage. `[ui] mouse = "scroll" | "off"`
defaults to best-effort vertical trackpad capture, routes the focused surface,
ignores clicks/horizontal gestures, and is owned through setup, runtime toggle,
suspend/resume, partial failure, panic, and reverse-order Drop cleanup. The
At TUI-138 completion, the shortcut browser documented the terminal selection
modifier and the `off` escape hatch. TUI-145 supersedes that presentation: the
default Scroll mode now owns visible left-drag selection and copy on release.

Completion passes 1,594 default main tests and 37 CLI tests with one documented
ignore, plus 1,623 all-feature main tests, 118 experiment tests, and 37 CLI
tests with two documented ignores. The first concurrent all-feature pass had
one unrelated timing-sensitive voice descendant-cleanup failure; its isolated
rerun and the complete all-feature rerun passed. All 154 render tests and 380
TUI tests pass; reviewed 100×30/60×24 snapshots have no pending `.snap.new`
files. Formatting, strict all-target/all-feature Clippy, all-feature release,
released help/version, diff hygiene, and oracle/reference immutability pass.

A disposable native PTY walkthrough could not be isolated without overriding
`HOME`, which the repository safety rules explicitly forbid. A bare attempt
stopped during startup on the existing credential check and changed no runtime
state; the interactive Finder/trackpad walkthrough is therefore recorded as
blocked rather than inferred from automated results. No installed binary, Git
history, oracle/reference file, session, provider, or runtime configuration was
changed. Codex material remained read-only engineering inspiration only.

## TUI-137 native composer cursor complete (2026-08-17)

Superseded presentation note (2026-08-18): TUI-139 retains this task's exact
insertion-cell calculation and modal suppression, but replaces terminal-owned
shape/blinking with a deterministic white software block blink.

The composer no longer draws a permanent narrow `▌` into the ratatui
buffer or reverse-styles the character at the insertion point. While the
composer has focus, rendering now computes the terminal display column from
the clamped UTF-8 byte cursor and exposes that exact cell as ratatui's native
cursor position. The user's terminal therefore owns the cursor's full shape
and visible/hidden blink frames. Modal focus suppresses the composer cursor so
it cannot float above an overlay.

Focused TestBackend coverage proves exact empty, ASCII end-of-input (`over`
with no trailing space), Unicode-middle, and modal-hidden positions, and proves
the real render buffer contains no composer `▌` glyph. Text snapshot support
retains deterministic cursor notation without changing runtime rendering; all
150 render tests pass with zero pending snapshots.

Completion passed 1,561 default main tests and 37 CLI tests with one existing
manual-browser ignore, plus 1,590 all-feature main tests, 118 experiment tests,
and 37 CLI tests with two existing ignores. Formatting, strict all-target/
all-feature Clippy, all-feature release build, release help/version, diff
hygiene, snapshot hygiene, and oracle/reference immutability checks pass. A
manual blink capture was not run because cursor blinking is terminal-owned and
the deterministic backend proves the requested cell/visibility contract. No
runtime state or installed binary changed. Codex material was read-only
secondary engineering evidence only; `PARITY.md` remains unchanged.

## TUI-136 bracketed paste for folder drops complete (2026-08-17)

The native TUI now enables Crossterm bracketed-paste mode while it owns raw
alternate-screen input. Finder-dropped paths therefore arrive as one paste and
enter the composer literally instead of sending a leading `/` key that opens
the slash-command palette. Ordinary typed `/` behavior remains unchanged.

`TerminalLifecycle` owns bracketed paste as a distinct acquired mode and
disables it before leaving the alternate screen or raw mode. Setup failure,
suspend/resume, resume failure, panic unwinding, and normal Drop paths have
deterministic reverse-order coverage. A focused composer regression proves an
absolute folder path remains literal input with Commands closed.

Focused tests, 1,560 default main tests plus 37 CLI tests, 1,589 all-feature
main tests plus 118 experiment and 37 CLI tests, format, strict Clippy, release,
help/version, diff, snapshot, and oracle/reference hygiene all pass. The two
documented all-feature ignores remain. A native Finder drag was not manually
run; no user runtime state, installed binary, oracle/reference file, or Git
history changed.

## REL-135 sandboxed SSH and read-only approvals complete (2026-08-17)

Ordinary required macOS command sandboxes now grant only the OpenDirectory
`libinfo` Mach lookup needed for libc uid/gid resolution. This repairs the
observed `ssh` failure `No user exists for uid 501` before connection without
granting the broader OpenDirectory API/membership surfaces; the closed
planner/executor experiment profile deliberately remains unchanged. A native
generated-policy regression proves `id -un` resolves a nonnumeric account, and
a strict BatchMode `ssh bsd true` smoke check completed with exit 0 and no
output.

Prepared `run_command` calls infer network authority for exact plain or system-
path `ssh`, `scp`, and `sftp` executables, so a model omission can no longer
turn an obvious SSH call into a sandbox-network failure. Inference only adds
the existing one-use network prompt; persistent grants, AutoReview, and Yolo
still cannot silently authorize network access.

New and omitted agent configuration now defaults to `auto-review`. AutoReview
uses a separate closed parser for foreground local read pipelines rather than
relabeling arbitrary `run_command` calls: exact non-writing utilities such as
`rg`, `ls`, `ps`, and `grep` can run without approval, while redirects,
substitutions, sequencing, background jobs, SSH, Git, Cargo, `tee`, `rg --pre`,
wrappers, unknown executables, and malformed shell remain gated. Explicit
Ask/Deny and network decisions retain precedence, and explicit saved
`allowlist` remains supported without automatic config rewrite.

Focused tests pass across permission, sandbox, workspace, jobs, config,
interactive orchestration, and headless execution. Default completion passes
1,557 main tests plus 37 CLI tests; one documented browser test is ignored.
All-feature completion passes 1,586 main tests, 118 experiment tests, and 37
CLI tests; two documented cases are ignored. One unrelated voice process-
cleanup assertion failed on the first concurrent all-feature run, passed
immediately in isolation, and the complete all-feature rerun passed. Format,
strict all-target/all-feature Clippy, and the all-feature release build pass.
No runtime config/session, SSH material, remote file, installed binary,
oracle/reference file, or Git history was changed.

## LRN-134 autonomous Dream and Distill complete (2026-08-17)

Interactive persistent profiles now schedule bounded Dream and Distill
maintenance after seven and thirty days by default. Missing learning and
corresponding memory switches adopt the enabled defaults without rewriting
configuration; explicit `false` remains authoritative. Bare, no-session,
headless, service, scheduled, gateway, review, and checkpoint runners have no
maintenance capability. The idle TUI serializes jobs, routes only through the
cheapest eligible priced `standard` route, issues at most one provider request
per run, and cancels maintenance before handling new user input.

The sanitized evidence corpus is limited to 32 eligible sessions, 64 recent
messages per session, and 256 KiB. It excludes hidden/provider/tool/media,
private-email, credential, and learning-ineligible import material. Dream's
strict maximum-32 batch carries source IDs and expected digests; automatic
changes can touch only Dream-owned memory and use a crash-recoverable CAS
journal. Distill accepts at most two one-file `SKILL.md` packages, publishes
only profile-owned `auto_distill` generations, retains the prior generation
disabled, and rejects scripts, assets, dependencies, tools, commands, plugins,
subagents, and executable metadata. `/dream` and `/distill` expose bounded
status, manual run, on/off, and exact-digest undo controls.

Focused learning/request/config/command/render coverage and native disposable
PTY status/default persistence checks pass. The complete validation matrix is
recorded in `TESTING.md`; no real provider, oracle/reference file, installed
binary, or user runtime state was changed. All seven MiMo-informed roadmap
tasks are complete, and no task in that roadmap remains active.

The exact combined roadmap tree also passes native FreeBSD 15.1-RELEASE-p2
amd64 validation on the `bsd` host as unprivileged `aji`: 1,535 default main
tests plus 37 CLI tests, then 1,564 all-feature main tests plus 116 experiment
tests and 37 CLI tests, with only the documented browser and external-fixture
ignores. The packaged native formatter, strict all-target/all-feature Clippy,
all-feature release build, help/version, snapshot hygiene, ELF identity, and
linkage checks pass. The disposable release binary SHA-256 is
`e5ed2c281efac7391d5dc4db793576871978996b3f517b22a307abe541c34c8c`.

## SES-133 foreign transcript import complete (2026-08-16)

`/import codex <path>`, `/import claude <path>`, and `/import opencode <path>`
now inspect only an explicit regular file or directory, parse within
1,000-file/64-MiB aggregate limits, and present a Cancel-first preview before
publication. Codex/Claude JSONL accepts only visible user/assistant text;
OpenCode SQLite is opened immutable/read-only and accepts only exact session,
message, and text-part rows. Symlinks, hidden reasoning, system prompts, tools,
raw output, media, credentials, provider state, approvals, grants, and foreign
routes never enter the canonical session.

Confirmation creates fresh inert session UUIDs without replacing the active
session. Safe source timestamps survive, while bounded provenance retains only
the source kind, base filename, source-session digest, counts, import time, and
an explicit learning-eligibility bit that defaults off. Resuming an import uses
the current configured `ca` route. Existing `/import <path>` portable JSON and
legacy sessions remain compatible.

Focused adapter, compatibility, preview, input, persistence, and resume tests
pass. Local completion passes 1,532 default main plus 37 CLI tests and 1,561
all-feature main plus 118 experiment plus 37 CLI tests; documented ignores
remain. Format, strict all-feature Clippy, all-feature release, help/version,
diff, and snapshot hygiene pass. A released disposable PTY proved asynchronous
inspection, Cancel-first behavior, explicit confirmation, default learning
exclusion, inert persisted content, and bounded path-free provenance. The
fixture moved recoverably to
`/Users/aji/.Trash/ca-ses133-pty-20260816`.

No oracle/reference file, installed binary, real user runtime state, foreign
source, external provider, or Git history changed. At that completion boundary,
LRN-134 was the sole remaining roadmap task; it is now complete as recorded
above.

## CFG-132 recent and favorite models complete (2026-08-16)

The model picker now projects each available provider/model identity exactly
once: Favorites first, Recent second, then provider groups. Explicit `Shift+F`
toggles the selected favorite, configurable `Ctrl+X R` and `Ctrl+X F` cycle
their available scoped lists, and the existing `Ctrl+P` cycle-all behavior is
unchanged. Removed models remain dormant in the bounded store and reappear in
their preference section if configured again.

Profile preferences retain at most eight recent models and 128 favorites in an
owner-only, symlink-safe, atomic file containing no credentials or route
authority. Successful model-selection entry points update recency without
changing passive resume semantics. Bare mode never loads or writes the store,
keeps recency only in memory, and refuses favorite mutation.

Focused storage, projection, input, keymap, persistence, unavailable-model, and
snapshot tests pass. Local completion passes 1,520 default main plus 37 CLI
tests and 1,549 all-feature main plus 118 experiment plus 37 CLI tests; the
documented ignores remain. Format, strict all-feature Clippy, all-feature
release, help/version, diff, and snapshot hygiene pass. A released disposable
PTY proved favorite/recent presentation, scoped cycling, restart persistence,
and bare isolation; the preference digest remained unchanged by the bare run.
The fixture moved recoverably to
`/Users/aji/.Trash/ca-cfg132-pty-20260816`.

No oracle/reference file, installed binary, real user runtime state, external
provider, or Git history changed. SES-133 is the sole active roadmap task.

## INP-131 explicit prompt stash complete (2026-08-16)

`/stash`, `/stash save`, and configurable `Ctrl+X S` now share one explicit
profile-scoped draft store. It retains at most 50 UTF-8 drafts, 64 KiB per
entry and 1 MiB aggregate, under owner-only atomic storage. Normal composer
history is never persisted. Enter removes and restores the selected draft; a
non-empty composer is atomically swapped into the store first. Deletion
requires the same selected entry to receive `d` twice, and bare mode refuses
every persistent mutation.

Stored attachments are inert text. Typed attachment state is cleared on save
and restore, and the ordinary submit path revalidates each restored `@` file
reference against current containment, file type, size, contents, and line
range. Pasted process-only images are refused rather than silently discarded.
Corrupt stores remain read-only until the next explicit save, which
quarantines the owner-controlled regular file before publishing a fresh store.

Focused storage, command, input, swap, corruption, bounds, bare, symlink,
permission, and attachment-drift tests pass. Local completion passes 1,507
default main plus 37 CLI tests and 1,536 all-feature main plus 118 experiment
plus 37 CLI tests; the documented ignores remain. Format, strict all-feature
Clippy, all-feature release, help/version, diff, and snapshot hygiene pass.
A released disposable PTY proved chord save, picker preview, restore, and
double-confirm deletion. The first walkthrough exposed macOS `/tmp`'s system
symlink; profile safety was narrowed to the canonical home and profile-owned
components, then the successful rerun completed. The fixture moved
recoverably to `/Users/aji/.Trash/ca-inp131-pty-20260816`.

No oracle/reference file, installed binary, real user runtime state, external
provider, or Git history changed. CFG-132 is the sole active roadmap task.

## TUI-130 optional turn metadata complete (2026-08-16)

Transcript annotations now optionally retain the actual provider/model, total
turn duration, and typed terminal outcome. The single event-loop completion
boundary updates only the final annotation for a turn and saves it through the
existing session path. Legacy records deserialize with every new value absent;
no missing value is inferred. `[ui.transcript]` timestamps and assistant
footers both default off. When enabled, the final assistant message renders one
compact `provider/model · duration` footer and labels cancellation as
`interrupted`.

The statusline has opt-in generation-speed and current-sitting-cost fields.
Both reuse `/context`'s shared usage summary: speed is the latest completed
foreground output tokens divided by first-provider-output-to-finish time, and
cost remains distinctly incremental, subscription allocation, mixed, or
unknown. Resume starts these process-only sitting values unknown until a new
request completes.

Focused lifecycle, compatibility, duration, cost/speed, configuration, input,
and render tests pass. Local completion passes 1,497 default main plus 37 CLI
tests and 1,526 all-feature main plus 118 experiment plus 37 CLI tests; the
documented ignores remain. Format, strict all-feature Clippy, all-feature
release, help/version, diff, and snapshot hygiene pass. A released disposable
80x24 no-session loopback PTY showed timestamps, one `fake/fake-model · 705ms`
footer, `/context` at 18.3 tokens/s, and honest unknown cost. The fixture held
only its injected/trust config and moved recoverably to
`/Users/aji/.Trash/ca-tui130-pty.Tv20E5`.

No oracle/reference file, installed binary, real user runtime state, external
provider, or Git history changed. INP-131 is the sole active roadmap task.

## TUI-129 asynchronous attachments complete (2026-08-16)

The composer `@` picker now starts generation-tagged blocking workers rather
than walking the workspace on the TUI thread. Discovery covers every declared
root, honors ignore files even outside Git repositories, follows no symlinks,
stops at 50,000 safe paths, and presents at most 250 fuzzy-ranked results.
Profile-relative frecency is capped at 1,000 records in owner-only atomic
storage; bare runs retain it only in memory.

Selected files retain typed root-relative identity and support `#N` and
`#N-M`. Submission revalidates current containment, lexical symlink
components, regular-file type, size, UTF-8 content, and line bounds. Already-
discovered MCP metadata appears in a distinct section without starting or
refreshing a server. Explicit selection resolves at send time through the
existing MCP origin/network boundary; bodies live only in a bounded transient
request message, while canonical history keeps the textual digest reference or
an unavailable marker.

Focused attachment, stale-generation, TUI input/render, and fake-MCP tests
pass. Local completion passes 1,488 default main plus 37 CLI tests and 1,517
all-feature main plus 118 experiment plus 37 CLI tests; documented ignores
remain. Format, strict all-feature Clippy, all-feature release, help/version,
diff, and snapshot hygiene pass. A released bare/no-session 80x24 PTY showed
the indexing phase, ignore-filtered results, fuzzy narrowing, and exact
`@src/main.rs#2-3` insertion; its disposable home remained empty and the
fixture moved recoverably to `/Users/aji/.Trash/ca-tui129-pty.hqK18T`.

No oracle/reference file, installed binary, real user runtime state, external
provider, or Git history changed. TUI-130 is the sole active roadmap task.

## REL-128 low-yield loop guard complete (2026-08-16)

Completed tool batches now feed bounded structured outcomes into a process-only
guard. It pauses before provider continuation after a third approximately
identical same-file edit, a third normalized identical failed command/output,
or a fourth edit/verification action without observable progress. Temporary
paths, timestamps, large numbers, seeds, durations, and long output are
normalized before comparison; transcript text is never parsed.

The TUI presents bounded evidence with Replan, Continue, and Cancel. Replan
resumes exactly once with a transient materially-different-approach instruction;
Continue suppresses the guard for the rest of that turn; Cancel opens with Keep
paused selected and preserves completed tool effects. Service mode emits an
explicit structured choice. Headless/check/scheduled execution terminates with
typed `low_yield_loop` failure instead of choosing or hanging. Session/route/
turn terminal boundaries clear all detector state, and nothing is persisted.

Focused detector, lifecycle, input, render, service, and headless tests pass.
Combined local completion passes 1,479 default main plus 37 CLI tests and 1,508
all-feature main plus 118 experiment plus 37 CLI tests; documented ignores
remain. Format, strict all-feature Clippy, all-feature release, help/version,
diff, and snapshot hygiene pass. A released disposable 80x24 loopback PTY
proved the third `false` failure pauses, Cancel defaults to Keep paused, and
Replan wakes the coordinator and completes with a different response. That
walkthrough exposed and drove a regression fix for the wake-up action. Its
stopped fixture was moved recoverably to
`/Users/aji/.Trash/ca-rel128-pty.M7cKLW`.

No oracle/reference file, user runtime state, installed binary, external
provider, or Git history changed. TUI-129 is the sole active roadmap task.

## TUI-127 timed thought and inline agent activity complete (2026-08-16)

Provider reasoning now has a measured process-local phase boundary: the first
reasoning fragment starts timing and the first text, tool call, completion,
cancellation, or failure freezes it. The transcript renders collapsed
`+ Thought: <N>ms` or expanded `− Thought: <N>ms`; old records never invent a
duration. The latest completed foreground request also records
first-provider-output-to-finish duration for honest `/context` output speed.

Conversation-origin child agents publish bounded sanitized watch snapshots
without making the TUI await manager locks. Their task, status, current action,
events, and recent output render beneath the originating turn and support
`Ctrl+X Down` focus, selection, detail scrolling, messaging, approval, and
Cancel-first cancellation. Task-graph and multi-run origins remain on their
existing surfaces. Sessions persist only typed terminal identity/task/status/
summary/timestamp records; raw events, output, approvals, messages, and grants
remain process-only.

`/context` remains read-only and now combines context occupancy/map/checkpoint
with distinct current-sitting cost states, latest generation speed, working
directory and roots, synchronous non-activating LSP classification, and bounded
task counts/titles. Local completion passes 1,462 default main plus 37 CLI
tests, and 1,491 all-feature main plus 118 experiment plus 37 CLI tests; the
documented ignores remain. Formatting, strict all-feature Clippy, all-feature
release, help/version, diff, and snapshot hygiene pass. Reviewed wide/narrow
snapshots cover the complete feature. A released disposable 80x24 no-session
PTY showed live `+ Thought`, expanded `− Thought: 652ms`, and `/context` at
18.3 tokens/s with honest unknown cost, workspace, inactive LSP, and tasks.
Only its credentialless fixture config was created, and the stopped fixture
moved recoverably to `/Users/aji/.Trash/ca-tui127-walk.s3mT9Z`.

This is a `ca` extension informed only by the supplied read-only Mimocode
screenshots, not a Cursor parity claim. No oracle/reference file, user runtime
state, installed binary, provider wire contract, commit, or physical FreeBSD
system changed. No roadmap task is active.

## Reliability-first roadmap complete locally (2026-08-16)

PRJ-126 completes OBS-117 through PRJ-126 in the current delivery tree. Scratch
data is profile-isolated and canonical-workspace-keyed under profile data,
owner-only, atomically written, bounded to 256 entries/256 KiB, human-mutated
through `/scratch`, and model-readable only through a filtered 16-KiB read-only
tool. It is never injected automatically or placed in sessions/exports/Git.
All combined local gates passed. The user explicitly paused BSD work, so the
PRJ-126/final exact-source FreeBSD gate is recorded as not run rather than
claimed. No roadmap task remains active.

## OUT-125 complete; PRJ-126 active (2026-08-16)

OUT-125 adds an opt-in conservative Git/Cargo semantic presentation without
changing canonical bounded tool results, sessions, exports, or provider
continuations. Ctrl+O retains the bounded raw result and `/output full` is an
exact Cancel-first native rerun. Local gates passed; exact-source FreeBSD test
suites passed, while the later native Clippy/release continuation was stopped
at the user's request and is not claimed. PRJ-126 is now the sole active task.

## CFG-124 One-Turn Models and Route Presets

Completed on 2026-08-16 while retaining the uncommitted OBS-117 through
CTX-123 tree. `/model <id> --once` now captures one original
provider/model/reasoning baseline, may replace an unused next-turn choice
without changing that baseline, and is consumed only when a new top-level main
turn starts. Completed, Failed, and Cancelled outcomes restore the baseline at
the existing lifecycle boundary before queued input. `/clear`, successful
resume, ordinary model selection, and `/preset` cancel unused overrides.

Backward-compatible route presets contain only provider, model, and reasoning
effort. `/preset` opens a deterministic picker or applies one named route
process-only. `/preset-save` validates 1-64 lowercase safe names, uses the
comment-preserving atomic config store, refuses bare persistence, and defaults
existing-name confirmation to Cancel. No endpoint, credential, prompt, full
profile inheritance, session shape, or provider wire format changed.

Focused lifecycle, config, command, input, orchestration, and eight reviewed
wide/narrow modal snapshots pass. Local completion passes 1,437 default main
plus 37 CLI tests and 1,466 all-feature main plus 118 experiment plus 37 CLI
tests. Formatting, strict Clippy, release, help/version, diff, and snapshot
hygiene pass. A disposable loopback TUI proved `once` then restored `base`,
preset application cancellation, comment-preserving save, default-Cancel
overwrite, and no session creation; the fixture moved to Trash.

The exact source also passes physical FreeBSD 15.1-RELEASE-p2 amd64 as
unprivileged `aji`: focused coverage; 1,424 default main plus 37 CLI tests;
1,453 all-feature main, 116 experiment, and 37 CLI tests; native format,
strict Clippy, release, help/version, snapshot hygiene, ELF identity, linkage,
and key-file source hashes. The disposable source was removed. `PARITY.md`
remains unchanged. OUT-125 is the only active next task.

## CTX-123 Retained-Boundary Context Controls

Completed on 2026-08-16 while retaining the uncommitted OBS-117 through
USG-122 tree. `/compact here [N]` now retains the latest two completed
canonical exchanges by default, accepts 1 through 32, and summarizes only the
complete prefix before that suffix. Exchange derivation follows unresolved
tool-call identity, so no tool interaction can be split. Invalid, incomplete,
or all-suffix boundaries refuse before provider I/O. Existing bare, focused,
and automatic compaction behavior and the checkpoint persistence schema are
unchanged.

`/context` now retains all exact textual estimates, occupancy, limits, and
checkpoint coverage while adding a deterministic 5x20 map. Its 100 cells use
the existing seven accounting categories and stable largest-remainder
allocation; category glyphs and colors are display-only.

Focused boundary, preservation, orchestration, allocation, command, and render
coverage passes with reviewed wide/narrow snapshots. Local completion passes
1,427 default main plus 37 CLI tests and 1,456 all-feature main plus 118
experiment plus 37 CLI tests. Formatting, strict Clippy, release,
help/version, diff, and snapshot hygiene pass. A disposable no-session
loopback walkthrough proved the fourth request summarized exactly the first of
three exchanges and `/context` reported 2 of 6 retained messages; the fixture
was moved to Trash.

The exact source also passes on physical FreeBSD 15.1-RELEASE-p2 amd64 as
unprivileged `aji`: focused coverage; 1,414 default main plus 37 CLI tests;
1,443 all-feature main, 116 experiment, and 37 CLI tests; native format,
strict Clippy, release, help/version, snapshot hygiene, ELF identity, and
linkage. The disposable source was removed. `PARITY.md` remains unchanged.
CFG-124 is the only active next task.

## USG-122 Provider-Account Usage

Completed on 2026-08-16 while retaining the uncommitted OBS-117 through
TUI-121 tree. Explicit `/usage` now separates provider-account scopes from the
unchanged process-local `ca` ledger. `src/provider_usage.rs` owns typed,
concurrent adapters, ten-second one-attempt bounds, a 256-KiB response cap,
sanitized fixed diagnostics, deterministic reports, and a 60-second
process-only cache.

OpenAI queries the official organization completions Usage and Costs APIs for
the current UTC month with the already configured credential. A 401/403 is
reported as unavailable organization-admin access. DeepSeek queries only its
official live balance endpoint. MiMo performs no account request and clearly
labels configured Token Plan allocation plus locally observed consumption as
an estimate, with live quota unavailable. Arbitrary configured provider names
are supported only when their routes are the exact recognized official hosts;
canonical IDs with overridden hosts are refused before network I/O.

Provider-account calls occur only when the user opens `/usage`, never retry,
and do not enter OBS traces, provider history, sessions, exports, canonical
messages, or the local usage ledger. Focused coverage includes ten adapter
tests, exact command/cache behavior, and eight reviewed loading, empty,
unavailable, and populated wide/narrow snapshots.

Local completion passes 1,420 default main plus 37 CLI tests and 1,449
all-feature main plus 118 experiment plus 37 CLI tests; the documented ignores
remain. Formatting, strict Clippy, release, help/version, diff, and snapshot
hygiene pass. A disposable no-session TUI showed loading, official-host
refusal without network, cached refresh, and clean exit, then was removed.

The exact source also passes on physical FreeBSD 15.1-RELEASE-p2 amd64 as
unprivileged `aji`: focused coverage; 1,407 default main plus 37 CLI tests;
1,436 all-feature main, 116 experiment, and 37 CLI tests; native format,
strict Clippy, release, help/version, snapshot hygiene, ELF identity, and
linkage. The disposable source was removed. `PARITY.md` remains unchanged.
CTX-123 is the only active next task.

## SEC-120 Agent-Harness Audit

Completed on 2026-08-16 while retaining the uncommitted OBS-117 through
LSP-119 tree. The visible idle-only `/audit` and offline `ca audit` now share
one deterministic scanner in `src/security_audit.rs`. It inspects only declared
primary-workspace and active-profile instruction, skill, hook, MCP, permission,
credential-reference, and executable surfaces. It performs no model request,
network access, subprocess execution, repair, or mutation.

The scanner uses no-follow reads and bounded deterministic traversal: 512
files, 2,048 directory entries, 64 KiB per file, 2 MiB total evidence, depth
eight, 128 findings, and a 64-KiB report. Typed findings carry severity, stable
rule ID, sanitized authority-relative path/evidence, and remediation. It flags
unsafe ownership/modes, symlinks, secret-shaped literals without their values,
suspicious executable resolution, remote MCP trust, broad permissions, invalid
definitions, and incomplete scans. The CLI returns 0 with no high/critical
finding, 1 with one, and 2 only when the audit itself cannot run.

Focused scanner, command, parser, CLI, and render coverage passes, including
eight reviewed wide/narrow loading/report/empty/error snapshots. Local
completion passes 1,402 default main plus 37 CLI tests and 1,431 all-feature
main plus 118 experiment plus 37 CLI tests; the documented ignores remain.
Formatting, strict Clippy, release, help/version/audit-help, diff, and snapshot
hygiene pass. A disposable offline TUI walkthrough showed the complete
scan/pager/restore lifecycle without user runtime or installed-binary changes.

The exact source also passes on physical FreeBSD 15.1-RELEASE-p2 amd64 as
unprivileged `aji`: 1,389 default main plus 37 CLI tests; 1,418 all-feature
main, 116 experiment, and 37 CLI tests; focused audit coverage; native format,
strict Clippy, release, help/version/audit-help, snapshot hygiene, ELF identity,
and linkage. An initial root-run permission result was discarded because root
invalidates that fixture; the corrected unprivileged run passed. The disposable
source tree was removed. `PARITY.md` remains unchanged. TUI-121 is the only
active next task.

## LSP-119 Post-Edit Diagnostics

Completed on 2026-08-16 while retaining the uncommitted OBS-117 and INI-118
tree. Configured language servers now retain sanitized `publishDiagnostics`
notifications by server and canonical document path. After successful
`write_file`, `edit_file`, `apply_patch`, and deletion operations, the LSP
manager synchronizes the affected document and waits only within the server's
existing timeout for diagnostics matching the new document version. `/init`,
`/undo`, and `/redo` use the same best-effort receipt boundary. Deletion sends
`didClose` and clears retained diagnostics.

Reports include bounded severity, source/code, one-based ranges, and sanitized
messages. They distinguish unavailable, stale, unversioned, timed-out, failed,
closed, and truncated outcomes, and are capped at 50 diagnostics and exactly
32 KiB. LSP absence or failure never changes a successful mutation or its undo
receipt. No server installation, automatic configuration, provider request,
session data, or background workspace scan was added.

Focused local coverage passes 5 `post_edit` tests, 11 LSP-focused tests, the
undo integration test, and reviewed wide/narrow pager snapshots. Local
completion passes 1,393 default main tests plus 36 CLI tests and 1,422
all-feature main tests plus 118 experiment tests plus 36 CLI tests; the two
documented ignored cases remain. Formatting, strict all-feature Clippy,
all-feature release, help/version, diff, and snapshot hygiene pass. A
disposable loopback-only TUI walkthrough used a fake configured LSP: a model
write succeeded, the exact version-1 warning was displayed, and the provider
continuation received the diagnostic. No user runtime state or installed
binary changed.

The corrected metadata-free exact source also passes on physical FreeBSD
15.1-RELEASE-p2 amd64 as unprivileged `aji`: 5 `post_edit` and 13 broad
LSP-matching focused tests; 1,380 default main tests plus 36 CLI tests; 1,409
all-feature main tests, 116 experiment tests, and 36 CLI tests; native
formatting, strict Clippy, release, help/version, ELF identity, and linkage.
An initial disposable re-transfer admitted macOS AppleDouble companion files,
which correctly caused fixture-manifest failures; that tree was removed and
the metadata-free archive passed. All disposable LSP-119 FreeBSD trees were
removed. `PARITY.md` remains unchanged. SEC-120 is the only active next task.

## INI-118 Safe `/init`

Completed on 2026-08-16 while retaining the uncommitted OBS-117 tree from
baseline `ae107301999744d490f1c134e9925f4d4c125011`. The visible idle-only
`/init` command accepts no arguments and targets only root `AGENTS.md` in the
canonical writable primary workspace. It refuses every existing target type,
including symlinks.

`src/project_init.rs` owns bounded ignore-aware evidence collection, excluding
build, dependency, oracle, and reference trees; bounded manifest/README and
applicable inherited-instruction excerpts; an evidence SHA-256; untrusted
prompt delimiting; 32-KiB/control-safe draft validation; and final fingerprint
revalidation. Confirmation defaults to Cancel. Unix publication opens the
canonical root and creates `AGENTS.md` with create-new and no-follow semantics,
then records an ordinary Missing-to-UTF-8 undo receipt. The provider request is
exactly one typed `ProjectInit` request with no tools and is observable only
when explicit OBS-117 tracing is active.

Focused local suites pass 7 `project_init` tests and 8 `init_` tests. Local
completion passes 1,387 default main tests plus 36 CLI tests and 1,416
all-feature main tests plus 118 experiment tests plus 36 CLI tests; the two
documented ignored cases remain. Formatting, strict all-feature Clippy,
all-feature release build, help/version, diff check, and snapshot hygiene pass.
Six reviewed wide/narrow snapshots cover preparing, preview, and error states;
the usage and command-palette snapshots contain the intentional request kind
and command additions.

A disposable bare-profile, loopback-only TUI walkthrough made one tool-free
request per invocation. Default Enter canceled without creating a path; a
second preview was confirmed, created exactly the displayed draft, and `/undo`
removed it. No user state, credentials, session, installed binary, oracle, or
reference file was touched, and the disposable fixture was removed.

The exact-source tree also passes on physical FreeBSD 15.1-RELEASE-p2 amd64 as
unprivileged user `aji`: 7 `project_init` and 8 `init_` focused tests; 1,374
default main tests plus 36 CLI tests; 1,403 all-feature main tests, 116
experiment tests, and 36 CLI tests; native formatting, strict Clippy, release,
help/version, ELF identity, and linkage checks. The disposable remote tree was
removed. `PARITY.md` remains unchanged because this is a deliberate `ca`
workflow rather than a Cursor-parity claim. LSP-119 is the only active next
task.

## OBS-117 Persistent Provider Exchange Tracing

Implemented on 2026-08-16 against clean `master` baseline
`ae107301999744d490f1c134e9925f4d4c125011`. Provider tracing is explicit and
off by default. `--trace` enables it for TUI, headless, and service runs, while
the visible always-available `/trace` surface reports status and supports
`on`, `off`, `list`, bounded pager inspection, and exact Cancel-first deletion.
Bare profiles refuse persistent activation, and no config field or implicit
saved setting was added.

`src/provider_trace.rs` owns one process hub and one unique versioned JSONL
file per activation below the active profile data directory. A bounded
asynchronous writer preserves a monotonic accepted-record sequence across
concurrent requests, applies owner-only Unix permissions, rejects symlink
storage/entries, tolerates a crash-truncated final line in the viewer, retains
files until explicit deletion, and stops only tracing at 16 MiB or on writer,
serialization, or redaction uncertainty. Request objects are bound to the
activation that created them, so late events can neither enter nor disable a
later trace.

The common `AgentClient` transport records every typed production
`RequestKind` across Chat Completions, OpenAI Responses, and Anthropic
Messages: request/attempt identity, elapsed time, final method/URL/headers/body,
status and retry decision, bounded raw stream frames, decoded events, and
terminal outcome. URL userinfo and query values, authorization/cookies,
account identifiers, all configured/runtime credentials, secret-shaped JSON,
encrypted reasoning, binary/media content, and terminal controls are redacted.
Trace failures never change provider behavior. Traces do not enter sessions,
exports, canonical history, `/request`, usage records, continuation state, or
non-model network traffic.

Focused local suites pass 16 provider-trace and 14 trace command/integration
tests. Local completion passes 1,379 default main tests plus 36 CLI tests and
1,408 all-feature main tests plus 118 experiment tests plus 36 CLI tests; the
documented manual-Chromium and external-fixture cases remain ignored.
Formatting, strict all-feature Clippy, the all-feature release build,
help/version and service-help smokes, diff checks, and snapshot hygiene pass.
The local arm64 release SHA-256 is
`bc363d34e5355e52c8dcc3534b848b1c7b0e0f641b79073a381010139f85929b`.

The disposable loopback-only walkthrough used synthetic credentials and no
external network. Headless and TUI provider turns created redacted owner-only
traces; `/trace` status/on/off/list/open worked, Cancel preserved the selected
file, and the synthetic credential and raw provider request ID were absent on
disk. It did not modify user config, sessions, credentials, installed binaries,
or repository content. The exact 863-file implementation-source archive (SHA-256
`124e80c6a6a1865025daadfec787017cc8c2276155c46119c375036a774dfccb`)
passes on physical FreeBSD 15.1-RELEASE-p2 amd64 with Rust/Cargo 1.96.1 as
unprivileged user `aji`: 16 provider-trace and 14 trace-focused tests; 1,366
default main tests plus 36 CLI tests; 1,395 all-feature main tests, 116
experiment tests, and 36 CLI tests; native formatting; strict all-feature
Clippy; release build; help/version/service-help; snapshot hygiene; and
ELF/linkage inspection. The FreeBSD release SHA-256 is
`794bc65e7958e88a8f95a3cf8a8d1fd9356a9ec7bcc176abd5c0b18a9dfe012a`.
The stale user-local Linux `rustup` shim was bypassed with packaged native
format/Clippy binaries; no host state was changed. `PARITY.md` remains unchanged
because this is an intentional `ca` observability extension, not a Cursor
parity claim. No later roadmap task is selected.

## CTX-112–MD-116 Selected Oracle-Informed Enhancements

Completed on 2026-08-16 against baseline
`dd1dc647e3c52d073099c0c3cf51c3ca73203dfa`. Manual `/compact` now accepts an
optional validated focus while automatic compaction remains unfocused and the
six checkpoint sections remain mandatory. UI configuration has an opt-in
`off`/BEL/OSC 9 completion notification with comment-preserving persistence,
one top-level turn-completion seam, a 30-second inclusive threshold, and
post-idle-redraw TTY-only emission. The new read-only `/session` pager keeps
conversation-history facts separate from process-only sitting time, model
requests, terminal tools, token subsets, cost/allocation, and context coverage.

Terminal Markdown now renders Pulldown-Cmark math plus unescaped `\\(...\\)`
and `\\[...\\]` through a deliberately bounded Unicode converter. Currency,
code, URLs, unsupported TeX, malformed input, canonical text, exports, and
sessions remain literal and unchanged. The tool registry now explicitly marks
the selected 18 built-ins as exploration; adjacent calls collapse across empty
assistant messages under stable first-call keys while ordinary/dynamic tools,
execution order, provider history, and persistence remain unchanged.

Focused suites pass for compaction (4), notifications (5), session/reporting
(88), Markdown math (5), and exploration grouping (6). Local completion passes
1,359 main tests plus 36 CLI tests and 1,388 all-feature main tests plus 118
experiment tests plus 36 CLI tests; the documented manual-Chromium and
external-fixture cases remain ignored. Formatting, strict all-feature Clippy,
the all-feature release build, release help/version, diff checks, and snapshot
hygiene pass. The local arm64 release SHA-256 is
`47a997081ae94906a2fff82420a5fe378cb526fdf3695bf96c87d4168724b86e`.

The exact implementation-source archive, created before final documentation
reconciliation (SHA-256
`5e640069a28e45e0df7eaeec8eb75a07fe3b4ba1fb47e0bc953d7fdd515fd96e`)
also passes on physical FreeBSD 15.1-RELEASE-p2 amd64 with Rust/Cargo 1.96.1
as unprivileged user `aji`: 1,346 default main tests plus 36 CLI tests; 1,375
all-feature main tests, 116 experiment tests, and 36 CLI tests; all focused
suites; native formatting; strict all-feature Clippy; release build;
help/version; snapshot hygiene; and ELF/linkage inspection. A platform-specific
system-prompt token estimate initially made the `/session` render fixture show
3403 on macOS and 3404 on FreeBSD; the fixture now normalizes only that display
number while production retains the native estimate. The linked FreeBSD 15.1
x86-64 ELF SHA-256 is
`8ff0208e7a7f2a9c9ecab60ff5792d6e41d98685e4c0bd99fda7f411f69ad1b0`.

The disposable loopback-only TUI walkthrough rendered the collapsed two-tool
group and inline/display math, showed `/session` totals, refused notification
persistence under the bare profile, created a focused checkpoint, and then
reported 3/3 checkpoint coverage. It made exactly three local requests, kept
`notification = "off"`, created no session file, and left the sample bytes
unchanged. No user configuration, credential, installed binary, oracle,
reference, or provider service was touched. All disposable local and FreeBSD
trees were removed. `PARITY.md` remains unchanged because no Cursor-parity
conflict was discovered.

## GIT-111 Approval-Safe `/commit` Workflow

Completed on 2026-08-15 against baseline
`b426f5ca7a951f61ba1d78c20049fd0f80719e8f`. `/commit` is now a visible,
idle-only, argument-free workflow over the existing working-tree review
surface. It stages and unstages selected literal whole-file paths immediately,
keeps exact Git path bytes separate from sanitized labels, accepts one manual
Unicode-safe subject, and presents a scrollable final confirmation that
defaults to Cancel. Ordinary `/diff` behavior is unchanged.

The Git service now resolves typed repository state, refuses detached HEAD,
conflicts, and active special operations, and permits named unborn branches.
Before committing it binds the named branch, optional parent, exact index tree,
and complete staged manifest; it revalidates branch/parent/tree immediately
before invoking ordinary `git commit -m` and verifies the resulting parent and
tree afterwards. Git is launched directly with literal pathspecs, bounded
output and time, owned process-group cancellation, normal hooks/signing/
filters/configuration, and uncertain-outcome reporting if HEAD moves after a
failed or canceled command. No provider, push, PR, branch change, config,
session, file-undo, or automatic recovery path was added.

Focused service, input, orchestration, and render coverage passes, including
exact-path edge cases, initial commits, drift, hook failure, timeout cleanup,
uncertain outcomes, Unicode editing, default cancellation, stale generations,
and 14 reviewed wide/narrow snapshots. Local completion passes 1,332 default
main tests plus 36 CLI tests and 1,361 all-feature main tests plus 118
experiment tests plus 36 CLI tests; the two documented manual/external cases
remain ignored. Formatting, strict all-feature Clippy, the all-feature release
build, release smokes, diff checks, and snapshot hygiene pass. The local release
SHA-256 is
`c704629724be722749945968808248cb746324f1887d1e72e3c16560fd5060a2`.

The exact source archive (SHA-256
`8642f812a140ee0476c3e51e08a6f3a18c8e6c8fab3976d133aaa4f1fcf8a03d`)
also passes on physical FreeBSD 15.1-RELEASE-p2 amd64 with Rust/Cargo 1.96.1
as unprivileged user `aji`: 1,319 default main tests plus 36 CLI tests; 1,348
all-feature main tests, 116 experiment tests, and 36 CLI tests; formatting;
strict all-feature Clippy; release build; help/version; ELF/linkage; and
snapshot hygiene. macOS archive metadata initially materialized 772 untracked
AppleDouble files in the disposable extraction; removing only those generated
files restored the exact source set and the experiment target passed 116/116.
The FreeBSD binary SHA-256 was
`d1ff2b32e57515efb1aaf3dede9cbdc1cae3adcbecc3991eb48bcead05b3cdfa`.

A disposable 80x24 no-network TUI walkthrough staged two files, retained the
subject after default cancellation, then committed exactly the confirmed tree
with the expected parent while leaving no runtime-home state. No binary was
installed. The implementation was committed on `master` as
`dd1dc647e3c52d073099c0c3cf51c3ca73203dfa`; all disposable local and FreeBSD
validation trees were removed. `PARITY.md` remains unchanged because no real
Cursor parity conflict was discovered.

## BSD-110 FreeBSD Recovery Regression Repair

Completed on 2026-08-15 against the exact repair tree derived from baseline
`06015abc90e567de4a440f03516ab5de3487767d`. Recovery artifacts now write a
V2 format containing regular files plus explicit directories, empty
directories, and their modes. Reconstruction creates safe temporary
directories, writes files, then restores final directory modes deepest-first,
so the WSP final-manifest proof is exact even when the source archive uses
`0775` directories. V1 regular-file-only artifacts remain readable under
their historical semantics.

FreeBSD `openat(..., O_NOFOLLOW)` symlink refusal via `EMLINK` is normalized
to the existing explicit symlink diagnostic. The required sandbox remains a
macOS-only fail-closed boundary: unsupported-platform tests now assert that
boundary rather than expecting successful sandbox execution. The logically
equivalent credential-readiness expression is accepted by FreeBSD Rust 1.96.1
strict Clippy.

Local completion passed 1,308 default main tests plus 36 CLI tests and 1,337
all-feature main tests plus 118 experiment tests plus 36 CLI tests; the
documented manual-browser/external-fixture cases remain ignored. Formatting,
strict all-feature Clippy, the all-feature release build, release smokes, diff
checks, and snapshot hygiene passed.

The exact tree archive (SHA-256
`06716749a4cebabeee10dbae66f9c83627eb91f468c3813e283f6efa00fc2b7b`)
passed on physical FreeBSD 15.1-RELEASE-p2 amd64 with Rust/Cargo 1.96.1 as an
unprivileged user: 1,295 default main tests plus 36 CLI tests; 1,324
all-feature main tests, 116 experiment tests, and 36 CLI tests; focused 7/7
recovery and 4/4 checkpoint tests; formatting; strict all-feature Clippy; and
the release build. The FreeBSD binary was a linked x86-64 ELF for FreeBSD 15.1
with SHA-256
`da11a35941fc6924e0aa922b55602bf726b5698f8102213dbaa7fea6e7cd0fec`;
help/version and linked-library smokes passed. No binary was installed and all
disposable validation trees were removed.

## CFG-109 Partial-Config Agent Defaults

Completed on 2026-08-15. `AgentConfig::default()` now uses the same semantic
values as its field-level serde defaults, and `CaConfig::default()` delegates
to that single implementation. A raw TOML document that omits `[agent]` now
got `max_turns = 100`, `approval_mode = "allowlist"`, a 30-second command
timeout, `sandbox_mode = "required"`, and the existing finite subagent limits.
Partial tables still preserve explicit values and default only omitted fields;
loading an existing partial file does not rewrite it. The provider-management
snapshot now reports the truthful default sandbox mode instead of the prior
accidental empty string. Provider defaults and runtime paths are unchanged.
REL-135 subsequently changed only the current omitted/new-profile approval
default to `auto-review`; explicit saved `allowlist` values remain compatible.

The characterization first failed with `max_turns` 0 versus 100. After the
repair, 67 config-store tests, 1,308 default main tests, 1,337 all-feature main
tests, 115 experiment tests, and 36 CLI tests pass, along with formatting,
strict all-feature Clippy, the all-feature release build, diff checks, and
snapshot hygiene. No network, runtime state, credentials, installation,
manual TUI, clean-Cursor, or physical-FreeBSD validation was used.

## Working-Tree Preservation and Stabilization Review

Reviewed on 2026-08-15 against baseline commit
`ce787efc4dce99d9ddb48a831bae5e45629657bc`. Before any repository edit, the
complete Git-relevant working tree was preserved outside the checkout at
`/Users/aji/project/ca-working-tree-preservation-20260815T225026Z/` as a
full-index binary patch for the 78 changed tracked files and a compressed
archive of all 44 non-ignored untracked files. The tracked patch SHA-256 is
`7a69d88efcdbbdb399a26d1e659c08dc128e3bf00eb2ed74183da15ce5634384`; the
untracked archive SHA-256 is
`7bfd0fb4522bef7aa1c1979699e6d50168ea98f8f4976b676f27e528fe179dcc`.

A reconstruction from `git archive` plus those two artifacts matched every
changed and untracked file byte-for-byte. Preservation did not stage, commit,
stash, or remove anything and did not access or mutate runtime configuration.
The review found no oracle/reference changes, no non-ignored untracked
symlinks, and no checkout-specific `/Users/aji` or `/home/aji` path in changed
Rust source. The pre-review delta was 11,364 tracked insertions, 463 tracked
deletions, and 15,449 lines in untracked files. At preservation time this was
a recovery checkpoint, not a Git integration action; the later owner decision
created the baseline commit recorded below.

The current macOS tree passes the full offline gate. On 2026-08-15 the
repository owner designated the complete tree as the new local baseline on
`master`, commit `06015abc90e567de4a440f03516ab5de3487767d`. BSD-110 then
validated the exact repair tree derived from that baseline on physical
FreeBSD. The external preservation remains a recovery checkpoint for the
preceding uncommitted state.

## AGT-108 DeepSeek Delegation Target

Completed on 2026-08-15 for macOS arm64. CA now has first-party DeepSeek V4
Flash and Pro price cards sourced from the official pricing page, exact
cache-hit/cache-miss actual USD accounting, and fail-closed eligibility only
for the official API root or `/v1`. Flash is capped at `simple`; Pro is capped
at `standard`. Mocked headless acceptance proves a Sol parent selects Flash for
a simple subagent while preserving `Subagent` accounting. Offline doctor
distinguishes eligible official metered billing from delegation-ineligible
custom endpoints without resolving or rendering the saved key. Strict Clippy,
1,334 main tests, 115 experiment tests, 36 CLI tests, and the optimized arm64
build pass. The release is installed at `/Users/aji/.local/bin/ca` with SHA-256
`3c57a1ea4a4214996409a526fe676c77b72ae44e86b73d43141e1d3faf0ddced`;
the prior binary remains in the timestamped AGT-108 backup. The persisted
parent is restored to `openai-codex/gpt-5.6-sol`. Authorized live run
`745e6c33-25d9-4164-afe5-ce9f88707218` called one simple child, selected
`deepseek/deepseek-v4-flash`, returned `AGT108_DEEPSEEK_OK`, made no fallback
retry, and exposed no credential.

## AGT-107 Browser-Authorized MiMo Token Plan Repair

Completed on 2026-08-15 for macOS arm64. MiMo browser authorization is no
longer treated as inherently PAYG: CA classifies the returned credential and
exact endpoint together, accepts `tp-` only with one of Xiaomi's three Token
Plan URLs, and rejects mixed credential/endpoint forms before probing,
persistence, traffic, or usage recording. Saved Token Plan setup reuses the
Keychain reference and changes only confirmed billing metadata after one
successful probe; new browser Token Plan secrets stay process-only until the
credential and metadata can commit together.

Setup, pricing status, peak forecasts, usage, and documentation now disclose
the 20% discount at 16:00–24:00 UTC (9:00 AM–5:00 PM PDT / 8:00 AM–4:00 PM
PST). Exact accounting retains the 0.8x multiplier only for wholly contained
requests, while delegation forecasts remain peak-only. Offline doctor uses a
metadata-only provider snapshot and never resolves Keychain secrets.

The installed saved Singapore route is Pro annual with a confirmed $528 USD
allocation. Offline doctor reports MiMo `USD Token Plan ready`, `standard`, and
delegation-eligible while the default remains `openai-codex / gpt-5.6-sol`.
Authorized live run `efae409d-c1f5-4b04-8b13-27fb27ef7fe7` called one simple
child, selected `mimo/mimo-v2.5-pro`, returned `AGT107_MIMO_OK`, recorded it
through the `Subagent` path, made no fallback retry, and exposed no credential.
Full gates, hashes, and recovery details are in the completed ExecPlan.

## AGT-106 macOS Inter-Provider Delegation

Implemented on 2026-08-15 for macOS arm64. MiMo browser authorization now
tolerates Firefox speculative loopback sockets with per-connection and overall
deadlines, exact host/path/method validation, bounded invalid connections, and
decrypt-before-success behavior. `/provider` adds masked `tp-` Token Plan setup
with explicit region, plan, term, actual USD, official-list comparison, and
transactional credential-reference/billing rollback.

`src/pricing.rs` owns four first-party source-bound cards, 30-day expiry,
strict allowlisted refresh, exact long-context and Token Plan credit/allocation
math, capability ceilings, and deterministic fail-closed routing. Optional
complexity hints on `task` and `spawn_agent` select among configured eligible
route clients; omitted hints preserve the inherited route. Route purpose is
separate from provider selection, so routed work remains `Subagent` usage.

Headless constructs the multi-route manager but restricts children to the
closed read-only tool subset. Mocked acceptance covers separate Sol-to-Luna
and Sol-to-MiMo delegation, pre-spawn grant rejection, and active-child
shutdown with `incomplete_subagents`. No live OAuth, provider probe, or paid
model call was used for AGT-106 acceptance. Full gate and installed-binary
hashes are recorded in the completed ExecPlan.

## Current Snapshot

`ca` is a single-crate Rust 2021 terminal coding agent. The primary implementation is in `src/`; `oracles/` and `reference/` are evidence, not dependencies.

The source currently contains:

- native FreeBSD 15 amd64 build support, including descriptor-safe symlink inspection for workspace materialization, accurate runtime platform prompts, manifest-safe `freebsd-x86-64` artifact identity, and closed packaged-Git discovery for scheduled materialization. FreeBSD retains the fail-closed macOS-only `required` sandbox boundary, so process tools require an explicit `policy-only` or `off` selection; macOS Keychain, CoreAudio capture, and LaunchAgent lifecycle features remain unavailable there. The current full native unit/CLI, strict Clippy, and release gates pass on the physical `bsd` host.
- automated `/provider` authentication under AUTH-104: four always-visible Xiaomi MiMo, OpenAI Codex (ChatGPT), DeepSeek, and OpenAI API rows; provider-supported MiMo browser/manual and OpenAI browser/device flows; masked bearer-key entry; canonical `credential://ca/...` storage in macOS Keychain or hardened profile-scoped FreeBSD files; candidate probing with authentication rollback; and one shared Codex-aware resolver for normal requests, doctor, probes, and model refresh. Bare mode remains process-only and cannot persist a default.
- repaired MiMo authorization handoff under AUTH-105: the direct published Xiaomi `/authorize` contract, source-aligned OSC 52 tmux/screen passthrough, and an automatic borderless FreeBSD URL screen remove popup-border contamination while retaining masked one-time-result entry.

- interactive ratatui and headless stdin modes in `src/main.rs`;
- an approval-safe process-only `/commit` workflow with exact whole-file
  stage/unstage operations, manual subject editing, Cancel-first confirmation,
  snapshot revalidation, normal Git policy, and uncertain-outcome handling;
- OpenAI-compatible streaming with tool-call assembly in `src/agent/client.rs`;
- built-in filesystem, search, shell, web, messaging, image, task, notebook, GitHub, and subagent tools in `src/agent/tools.rs`;
- approvals, undo snapshots, sessions, project instruction loading, slash commands, pickers, vim input, shell shortcuts, markdown, and diff rendering;
- process-only bounded file mutation history: successful main-session `write_file`, `edit_file`, `apply_patch`, and `delete_file` results retain secret-free typed before/after receipts; `/undo` and `/redo` compare live state and restore one exact UTF-8 file on a blocking worker, fail closed on drift, and leave conversation/session branches untouched.
- process-only `/pause` and `/unpause` at completed provider/tool/finalization boundaries, retaining canonical protocol state exactly once while keeping Ctrl+C cancellation, queueing, goal/workflow ownership, sessions, and other runners distinct.
- a scripted `--test` runner in `src/test.rs` with cases under `tests/`, plus an injectable fake-backend core for Cargo tests;
- a strict long-lived `ca.serve.v1` JSONL service over stdio or one authenticated literal-loopback WebSocket client, with bounded `ca attach`, external protected token sources, process-only sessions/turns, canonical prepared requests and nested runner events, exact client-resolved approvals/interactions, cancellation/status/shutdown, and owner-scoped cleanup without session persistence;
- explicit profile-scoped `ca schedule` one-shot/cron automation with disabled-by-default definitions, exact provider/model/workspace/runtime/tool/grant/network bounds, pre-run atomic claiming, late-run skipping, isolated no-session workers, digest-pinned declarative workflows, and local-file or opt-in signed-webhook delivery;
- explicit profile-scoped `ca plugin` local declarative bundles with strict bounded manifests, disabled-by-default owner-only registries, deterministic content identity, capability/collision previews, exact-confirm activation, existing extension boundaries only, and no native in-process code or source deletion;
- explicit global `ca update` lifecycle with one pinned Ed25519 authority, closed current-target manifests, exact staged artifacts, invoking-executable-only atomic replacement, one retained verified rollback copy, and no automatic network or installation;
- explicit profile-scoped signed plugin catalogs with locally pinned Ed25519 roots, monotonic verified snapshots, per-package/file hashes, private staged disabled install/update, retained verified rollback, and no catalog network or automatic activation/update;
- reviewed ratatui snapshots for startup, streaming, completed-tool, approval, model-picker, and searchable command-palette states at 100×30 and 60×24;
- a typed slash-command registry in `src/tui/commands.rs`, with input actions handed to orchestration rather than executing side effects in the reducer.
- version-4 semantic sessions managed by an injected `SessionStore`, with backward-readable V1-V4 monoliths and storage-v1 bundles whose stable `.json` metadata commit point indexes one generation-scoped append-only JSONL history. New saves/migrations support bounded recent/older pages, exact full-history hydration gates, metadata-only listing/pinning, exact-pair lifecycle operations, stable UUIDs, queued drafts, branches, and profile-scoped active/archive roots.
- confirmed `/resume` deletion for one inactive saved session, with exact storage-key/UUID validation, active-session protection, blocking-worker mutation, predictable filtered selection, and no branch cascade.
- `/resume` Active/Archive organization with pinned-first active ordering, exact UUID-checked pin/archive/restore mutations, Cancel-first archive confirmation, current-session archive protection, and no descendant cascade.
- a stable-ID input queue with explicit editing/selection, queue-by-default busy Enter, Ctrl+S steering, Ctrl+C interruption, success-only crash-safe draining, Session V4 persistence, restored-draft non-submission, and full/preview queue surfaces.
- non-destructive context checkpoints generated by a dedicated no-tools request, with optional per-model windows, complete-turn cuts, request reconstruction, persistence, and `/context` usage reporting.
- process-only complete request estimates split across instructions, skills, built-in and MCP tool schemas, checkpoint, conversation, and attachments; `/context` distinguishes previews from prepared requests and the transcript derives a visible checkpoint boundary without changing canonical messages.
- one typed `PreparedModelRequest` boundary for every production runner plus a process-only `/request` inspector showing the latest bounded semantic request with provider/model/kind, estimates, messages, and tool schemas after credential, structured-field, terminal-control, binary, depth, item, and UTF-8-safe redaction/bounding.
- a defaulted provider boundary: missing config remains byte-identical Chat Completions, while complete Chat Completions, opt-in OpenAI Responses, and opt-in Anthropic Messages adapters own path, authentication, exact reusable request bytes, SSE decoding, usage/tool translation, and safe body-free failures. Bounded opaque continuation state is bound to the actual provider/protocol/normalized-endpoint digest before canonical event consumption.
- explicit exact-pair fallback for interactive main turns: one allowed exhausted pre-output failure may switch to a configured provider/model, pin that route through the tool loop, persist sanitized turn provenance, and restore the configured primary automatically for the next user turn.
- an idle-only `/context-limit [provider/model]` editor, also routed from `/config`, with field-wise inheritance, exact model-profile/context-table persistence, positive/below-window validation, between-turn application, and bare-mode read-only behavior.
- a process-only, read-only-during-work `/usage` ledger for main turns, continuations, checkpoints, reviews, provider-backed subagents, and provider probes, with typed outcomes, detailed token subsets, and exact configured pricing that remains unknown when absent.
- an asynchronous read-only Git change service and full-screen “Working tree changes” list/diff surface for staged, unstaged, deleted, renamed, and untracked metadata.
- a process-scoped asynchronous shell `JobManager` with bounded output/events/stdin/cancellation, responsive `!`/`!!`/`!&`, job-control tools, and a `/jobs` list/detail surface.
- an idle-only `/term` manager for user-installed tmux with safe bounded discovery, detached creation, full-screen attach restoration, client detach, Cancel-first confirmed kill, and no tmux cleanup during normal `ca` shutdown.
- profile-scoped `/theme` and `/statusline` managers with closed validated display choices, reversible palette previews, deterministic ordered fields, unchanged legacy defaults, and an optional process-only custom status worker that fails closed unless a required network-denied sandbox is enforceable.
- truthful bounded tool output: ranged streaming UTF-8 file reads with continuation metadata, independent job stdout/stderr head-tail captures and typed outcomes, decoded/capped WebFetch responses, UTF-8-safe generic omission markers, and labelled headless text previews.
- bounded local PDF text reading through `read_pdf`: exact 1-based inclusive ranges, blocking-worker parsing, file/page/text/result caps, truthful page/continuation metadata, safe encrypted/malformed/panic handling, and no embedded-file/action traversal.
- typed multimodal request content plus bounded normalized image input: exact model capability gates, adapter-owned Chat Completions/Responses/Anthropic serialization, process-only clipboard and `view_image` payloads, and unavailable-safe durable markers.
- injected `RuntimePaths` and a raw-document `ConfigStore`; config/session/history locations share one policy and config patches preserve unresolved secret placeholders.
- immutable named run profiles selected with `--profile`: whole-file config, profile-scoped Keychain accounts and sessions, lazy first-save creation, and visible profile identity. The `bare` profile reads normal provider/safety settings while disabling extensions, config writes, and session persistence.
- an idle-only `/sandbox` manager, also routed from `/config`, that distinguishes saved next-launch mode from the immutable effective process mode, reports backend/root/network facts, requires Cancel-first confirmation for weaker modes, and remains view-only in bare mode.
- an idle-only `/provider` (`/providers`) hub with searchable provider-first list/detail navigation, distinct Add Provider/Add Model/Edit Model/Edit Provider flows, safe credential-source summaries, masked direct API-key entry backed by macOS Keychain, targeted catalog refresh, confirmed provider deletion with deterministic replacement and rollback, and no individual model-delete action. Provider and setup lists expose a 40-row audited provider-template catalog through `Shift+P` and `/provider templates [filter]`, alongside the direct OpenAI and custom paths. Fourteen entries fit the current complete agent adapters; OpenAI direct, DeepSeek, and Xiaomi MiMo include their required tool-continuation state behavior. The other 26 remain visible with typed OAuth, cloud-auth, subprocess, routing, request-control, or provider-state requirements and cannot be mis-saved as supported.
- an explicit `/provider setup [filter]` walkthrough with process-local typed provider health, one isolated representative-model request per check, missing-environment/no-model local failures, cancellation/stale-result protection, secure repair return paths, and a passing-only current/default chooser.
- secret-safe provider-aware diagnostics through offline `ca --doctor` and `/doctor`, with text/JSON reports, config/default/catalog/sandbox/render checks, and explicit `--connect` reuse of the isolated provider probe.
- a searchable idle-only `/config [filter]` hub that exposes safe model/provider/reasoning/review/MCP/skills/path settings, delegates complex actions to existing managers, and atomically persists a dedicated review model without changing the chat default.
- asynchronous live model discovery from configured OpenAI-compatible providers, with bounded provider-specific catalog authentication, agent-compatible Ofox filtering, selected-provider live/fallback/unavailable state, configured/documented fallbacks, and durable live-only selection; DeepSeek and MiMo retain current chat models when authenticated discovery is unavailable.
- explicit `/provider discover` local discovery over exactly 103 approved `127.0.0.1` ports, using a credentialless no-proxy/no-redirect client with short deadlines and bounded concurrency/response/model state; selecting a result pre-fills the existing Add Provider form and only Save persists it.
- an `Always` `/btw [question]` side-question overlay with a protocol-complete frozen context, independent task/generation/cancellation and `UsageKind::Btw` accounting, an explicitly empty tool set, and no hook/MCP/image/queue/canonical/session path.
- a validated profile-scoped global keymap with one- and two-stroke bindings, searchable `/keymap` capture/reset, dynamic labels, fixed Ctrl+R Diff and safety controls, plus a newest-first process-only `/history` draft-restoration overlay.
- a process-scoped asynchronous hook runner for the interactive main session, with bounded/sandboxed child processes, exact lifecycle dispatch, descendant cleanup, default-enabled compatible config, and a command-free `/hooks` inspection/test/toggle/repair manager. Bare mode can inspect but never executes or persists hook changes.
- a bounded, searchable model picker whose advertised `Ctrl+K` shortcut opens on the current model, whose fixed visible window keeps 100-model catalogs navigable with page/jump keys, and whose current/match/no-match cues remain visible at wide and narrow terminal sizes.
- a selection-friendly terminal lifecycle: best-effort vertical mouse capture defaults on but is configurable off; the tested RAII guard owns raw mode, alternate screen, bracketed paste, mouse toggling, suspend/resume, partial failure, panic, and reverse-order cleanup.
- a bounded incremental SSE decoder and shared assistant-turn assembler used by TUI, headless/script, checkpoint, and subagent paths, plus immutable branch-based `/retry`.
- a central typed tool registry owning all built-in schemas, validation, dispatch, profiles, mutation/capability/resource metadata, and typed execution outcomes.
- one process-only central tool-selection policy for interactive, headless, scripted, review, checkpoint, and subagent runners. Repeatable/comma-delimited `--tools`, `--include-tools`, and `--exclude-tools` resolve case-insensitively in registry order without widening profiles, capabilities, grants, approvals, workspace, sandbox, or network authority.
- an injectable structured headless runner with `ca.headless.v1` JSON/JSONL lifecycle events, strict output separation, fail-closed policy, atomic last-message output, and deterministic exit codes.
- a typed structured-interaction engine with exact tool-call continuation, ordered coexistence with approvals/tools, dedicated modal input, and host-only zeroized secret handles.
- exact optional model profiles, configured reasoning request/stream transport, persisted effort selection, and collapsed/expanded reasoning presentation without model-name inference.
- authoritative activity phase and turn outcome state; queued input drains only after `Completed`, not merely when the UI is idle.
- one terminal-text sanitizer for provider/tool/job display boundaries covering CSI, OSC, DCS, APC, BEL, C0 controls, and malformed escape input.
- explicit workspace roots with stable IDs, one-time prepared tool resources, an origin-aware approval broker, required-by-default process sandboxing, distinct network grants, and run/session-owned jobs.
- a strict filesystem skill registry with bounded metadata-only discovery, cached deterministic exact-ID/exact-alias/BM25 suggestions, visible non-activating composer and `/skills` guidance, exact hash-bound activation, project/user precedence, and session-safe restore warnings.
- a lazy transport-neutral MCP manager with hash-bound project trust, distinct network grants, filtered/redacted stdio environments, safe environment-referenced Streamable HTTP headers, collision-safe dynamic tools, bounded text resources/HTTP bodies/SSE events, prompt-default approval, and transport-specific cleanup.
- a default-empty process-scoped LSP manager for explicitly configured existing executables, with lazy sandboxed/network-denied startup, six Main-only semantic-navigation tools, 1-based Unicode-to-UTF-16 conversion, bounded workspace-only results, and exact process-group cleanup.
- version-4 sessions with revisioned typed plans/tasks/decisions/evidence, recorded validation events, user-only approval, CAS-safe mutations, and explicit full/historical branch lineage.
- a process-scoped asynchronous `AgentManager` with stable IDs, configurable limits, summary context, exact child grants, per-child approval waits, bounded events/output, owner-scoped jobs, `/agents`, and interrupted-summary persistence.
- a fresh-context `/review` workflow with bounded argument-safe Git targets, untracked-name-only privacy, read-only reviewer tools, structured findings, and Session V4 review records.
- durable finite `/goal` sessions with bounded evidence-only evaluation, a sole non-executable reporting schema, request-only continuation feedback, exact reported provider-token accounting, explicit pause/resume/clear controls, and typed terminal/limit outcomes that never bypass approvals or user steering.
- a finite `/workflow run <path>` engine for bounded TOML/JSON DAGs, with deterministic dependency scheduling, exact frozen routes, restrictive tool intersections, Cancel-first scoped run grants, local evidence/retry decisions, bounded named outputs, Session V4 restore safety, and explicit pause/resume/clear controls.
- an explicit profile-scoped `/worktree` lifecycle backed by a versioned owned manifest, bounded status/diff, exact workspace attachment, immutable child-agent workspace snapshots, attached-root workflow loading, dirty-tree refusal, Cancel-first clean cleanup, and unconditional branch preservation.
- a reusable Unix disposable-workspace boundary for benchmarks and later scheduled mutations. It opens source/private roots once with no-follow descriptors, bounds observed entries/depth/logical bytes/free-space reservations, rejects special files/hardlinks and unsafe symlink graphs, supports exact scheduled tracked-path allowlists, clones regular files with APFS `fclonefileat` or bounded create-new copying, verifies source-before/source-after/destination manifests, persists owner-only recovery receipts, and requires a workspace-bound verified external artifact before retryable explicit cleanup. Published workspaces survive handle drop; ordinary interactive tasks do not use this path.
- an explicit dependency-aware `/tasks` graph over approved plan tasks, with validated exact child grants and owned-worktree IDs, stable ready-node scheduling through `AgentManager`, append-only bounded attempts, local assistant/tool evidence gates, failure-paused descendants, and confirmation-only start/retry/pause/cancel.
- an explicit process-only `/multi-run` best-of-N workflow for two to five exact provider/model candidates, each in a distinct profile-owned worktree from one frozen base/request, with peer-failure isolation, bounded hashed local evidence, separate Cancel-first read-only judgment and fusion-proposal requests, distinct request/usage ownership, and no automatic attachment, merge, cleanup, default change, or session write.
- a non-default `planner-executor-experiment` feature with a separate offline `ca-reasoning-artifact-experiment` binary. Its `schema`, `validate`, and `cost` commands expose the strict provider-neutral `reasoning_artifact.v1` contract, optionally verify declared repository-path digests, and aggregate explicit per-request price/usage fixtures into exact success-adjusted cost summaries without provider traffic, tool execution, runtime-state mutation, or normal `ca` integration.
- an explicit process-only `/walkthrough` changes explanation over one complete bounded Git snapshot, with local refusal for partial/untracked input, an exact SHA-256 source identity, a sole validated ordered-report schema, and visible stale-source detection without canonical/session/config/Git mutation.
- an explicitly selected `browser` tool for existing local Chrome/Chromium, with a disposable user-data directory, literal-loopback bounded CDP, owner/process cleanup, external network blocked outside separately approved navigation/download windows, and bounded sanitized DOM, normalized screenshot, and no-overwrite download results.
- disabled-by-default explicit `/voice` capture from the macOS default input, with one generation-owned bounded mono WAV, sandboxed local or exact-provider transcription, safe typed failures, cancellation/descendant cleanup, process-only usage, and insertion only as an editable unsent composer draft.
- explicit profile-scoped `ca runtime` management with exact-source Ed25519 trust, monotonic closed catalogs, exact disk previews, streamed per-file size/hash verification, owner-only staging/retained versions, cancellation cleanup, full-byte rollback/entrypoint revalidation, and confirmed one-runtime removal. Managed bytes remain inert and are not discovered by current integrations.

This is an implementation inventory, not a claim of Cursor parity or production readiness.

## Repository State at Harness Creation

- Branch reported by the workspace: `master`.
- The repository now has a root baseline established on 2026-08-13 for the primary source, tests, plans, and project documentation. Generated/local evidence trees (`target/`, `.mimocode/`, `oracles/`, and `reference/`) remain present but are excluded from the root repository; Git diff/status can attribute subsequent project changes safely.
- Historical plans under `.hermes/plans/` describe multiple implementation pushes, but completion claims are not authoritative.
- No project source was modified while creating this harness.

## Verification Snapshot

- 2026-08-15 stabilization review: `cargo test --no-fail-fast` passed 1,305 main tests plus 36 CLI tests with one documented manual-browser ignore. `cargo test --all-targets --all-features --no-fail-fast` passed 1,334 main tests plus 115 experiment tests plus 36 CLI tests, with the documented manual-browser and external-fixture ignores. `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo build --release --all-features`, `git diff --check`, snapshot hygiene, and released `--help`/`--version` smokes passed. No provider, network, manual TUI, clean-Cursor, or physical-FreeBSD check was run during this review.

- WSP-096 disposable-workspace primitive: PASS, 22 focused tests covering actual APFS cloning, forced copying, manifest equality, source/root/staging/cleanup swap races, bounded observed entries and per-filesystem reservations, exact scheduled allowlists, hardlink/symlink/special-file rejection, durable receipt reopen, artifact ancestry/replay refusal, partial-cleanup resume, and retryable exact-inode deletion. Full validation passed 1152 unit tests plus 36 CLI tests, formatting, strict Clippy, release build, help/version, and snapshot hygiene. No user runtime state, Git metadata, oracle/reference content, provider traffic, credentials, installation, or ordinary checkout mutation occurred.
- RND-097 implementation is COMPLETE. The feature-gated controller owns immutable 72-slot preregistration, six isolated arms, exact one-call broker/tool IPC, a hard USD-50 reservation ledger, WSP workspaces and reconstruction proof, hidden verification, safe resume, calibration gating, SC-G recovery, SP rendering, and paired underpowered reporting. Completion gates passed 1,166 default unit tests plus 36 CLI tests, 112 feature experiment tests plus 47 reasoning integration tests, formatting, default/all-feature strict Clippy, both release builds, released 12-fixture verification, help/version, and snapshot hygiene. A fresh authorized run reached the credential-only canonical-route preflight and stopped before its first POST because no configured `openai` Sol/Luna route exists; its durable report has 72 pending slots, zero events, and exact zero settled/outstanding/unknown USD. The paid hypothesis remains unmeasured and no routing policy was enabled.
- `cargo test --no-fail-fast`: PASS, 1130 unit tests plus 36 CLI integration tests after PRV-094 (one manual local-Chromium test remains ignored by the ordinary gate and passed when explicitly invoked under BRW-086).
- RND-095 offline experiment: PASS, 16 focused tests covering strict schema/version/unknown-field handling, dependency DAGs, one-creator/dependent-step path lifecycle, capability intersection, descriptor-relative no-follow path verification, nonblocking special-file refusal, aggregate-I/O bounds, artifact hashing, input bounds, and exact date-stamped declared-rate cost aggregation. The non-default feature keeps the experimental binary out of ordinary `ca` execution; this is validation of a caller-supplied offline prototype, not authenticated provenance, full Git/worktree/task verification, live planner/executor orchestration, or cross-model reasoning-state transfer.
- PRV-094 provider continuation: PASS, 23 provider tests, 32 client tests, 7 provider-state safety tests, 134 provider-filtered unit tests plus 2 CLI tests, and 114 render tests. Reviewed 100×30 and 60×24 snapshots show the 14-ready/26-integration split. A released disposable three-provider walkthrough completed exact two-request tool loops for OpenAI, DeepSeek, and MiMo without external traffic, credentials, user runtime state, or opaque-state output. Post-review regressions also cover plain service history, production subagent route identity, and cancellation that changes a tool-call set.
- focused OpenAI-template suite: PASS, 4 exact input/config/orchestration/render tests; `cargo test provider_ --no-fail-fast` passed 106 unit tests plus 2 CLI tests; TUI input and render suites passed 63 and 112 tests; isolated Responses probe and adapter tests each passed. Reviewed 100×30 and 60×24 snapshots show the separate template/custom actions, masked prefill, and complete duplicate-refusal outcome. A released disposable TUI opened without changing the config hash, saved exact `openai`/official-root/`${OPENAI_API_KEY}`/`gpt-5.6-sol`/`openai_responses`, retained `deepseek/auto`, and stopped missing-key discovery locally without provider traffic.
- full capability audit: PASS, 49/49 exact roadmap rows and archived plans, no unchecked milestones/broken links, every named public contract compiled, closed risky defaults asserted, service v1 JSON unchanged, focused service/schedule/plugin suites green, zero pending snapshots, and released default/named/bare/manager smokes with no runtime-state creation.
- focused self-update suite: PASS, 9 tests across leading command routing, no-write status, exact source/key authority, signed closed current-target manifests, idempotent trust and rotation refusal, stale-lock recovery, exact network paths, body-free HTTP failure, staged-byte manifest binding, target ownership/mode checks, atomic install, and rollback. A released copied-binary walkthrough made exactly one manifest and one artifact GET, restored the copy byte-for-byte, left the repository release hash unchanged, and created no config/session.
- focused managed-local suite: PASS, 9 model-store/supervisor tests plus parser and CLI integration coverage for bare/conflict routing, GGUF header/hash/size/symlink/drift/atomic lifecycle, exact loopback downloads, safe HTTP failures, fixed argv, clean environment, loading/readiness/model checks, bounded logs, mismatch cleanup, process-only provider construction, and source-preserving removal. The released signed-runtime/GGUF walkthrough returned `local-ok`, left config and sessions absent, preserved the source GGUF, and left no server process.
- focused external-runtime suite: PASS, 10 tests across leading/bare CLI safety, exact HTTPS/literal-loopback source authority, signature/revision/target/manifest validation, profile/mode isolation, offline previews, streamed lifecycle, response caps/timeouts/body exclusion, cancellation/staging cleanup, symlink refusal, complete retained-byte verification, rollback, and exact removal. A released disposable lifecycle made exactly two catalog and two artifact GETs across install/update, revalidated both path/rollback results, removed one runtime, and left config/sessions untouched.
- focused voice suite: PASS, 22 tests across closed/redacted configuration, command/modal controls, stale/cancel isolation, bounded WAV generation, local exact-argv sandbox/process cleanup, provider multipart/auth/failure/timeout bounds, process-only usage, safe snapshots, and editable-draft insertion. A released disposable TUI walkthrough verified command discovery, disabled guidance, and the configured Ready modal without pressing Space, so no microphone permission, audio capture, or network request occurred.
- focused browser suite: PASS across typed/closed action schemas, default omission and exact inclusion, action-specific network/path preparation, executable/loopback target validation, owner isolation, cancellation-safe network re-blocking, runtime expiry, bounded DOM/screenshot/download handling, no-overwrite persistence, and descendant process cleanup. The explicit disposable local-Chromium walkthrough loaded one loopback page, captured its DOM and a normalized screenshot, closed cleanly, and left no browser process or temporary profile.
- focused scheduled-task suite: PASS across strict definition/path/route/tool/grant/delivery validation, owner-only atomic profile stores, redacted state, five-field cron calculation, disabled-by-default behavior, isolated released-worker execution, exact provider traffic, bounded local result delivery, and zero saved sessions.
- focused JSONL service/attach suite: PASS across strict stdio and WebSocket framing, protected token sources, wrong/missing/Origin/busy refusal, literal-loopback authority, malformed/duplicate/oversized/binary input, multi-turn history, central tool selection, exact approval/interaction waits, typed provider failure, stale cancellation, secret-safe output/Debug, reconnect, cwd authority, and owned process cleanup. The released listener/attach walkthrough emitted 33 ordered events across five credentialless requests, proved approval-before-mutation and structured interaction, cancelled to idle, reconnected through an environment source, kept daemon stdout empty, and wrote no session.
- focused theme/statusline suite: PASS across default compatibility, validation/preservation/profile rules, safe custom-command execution/cancellation/bounds, input/orchestration, and 22 reviewed wide/narrow snapshots. The released disposable TUI walkthrough proved save/restart, reordering, explicit/periodic status output, hidden command text, and bare no-write behavior.
- focused durable-terminal suite: PASS across exact argv/targeting, bounded parsing, unavailable/error behavior, input isolation, busy/default-Cancel controls, terminal suspend/resume, and 14 reviewed snapshots. The released fake-tmux walkthrough proved attach restoration and left one external survivor untouched at `ca` shutdown.
- focused soft-pause suite: PASS across idle/requested/repeated/resume/cancellation state, busy command discovery, all three continuation boundaries, and six reviewed wide/narrow snapshots. The released disposable TUI walkthrough proved held file side effects and finalization until `/unpause`, plus terminal Ctrl+C cancellation.
- focused undo/redo suite: PASS across exact create/modify/delete transitions, redo order/clearing/capacity, drift/symlink/bounds, typed receipt non-leakage, command discovery, and generation-safe completion. The released disposable TUI walkthrough proved approval-ordered plus direct recording, two real undo/redo cycles, new-mutation invalidation, and external-drift refusal.
- focused lazy-history suite: PASS, 107 session-filtered tests plus the visible-anchor prepend regression and eight reviewed wide/narrow recent/loading/complete/failure snapshots. The released disposable TUI walkthrough proved legacy V4 migration, 64/130 recent loading, exact older-page hydration, full-history provider submission, same-generation append, and exact metadata/log archive/restore.
- focused guided-walkthrough suite: PASS, 16 matching capture/validation/command/input/render/orchestration tests. Fourteen reviewed 100x30/60x24 snapshots cover capture, generation, completion, staleness, empty/refused input, and provider failure. The released disposable TUI walkthrough proved exact-source staleness and explicit refresh across two distinct hashes/sole-schema requests, followed by local untracked-input refusal with zero extra traffic.
- focused semantic-skill suite: PASS, 24 matching unit/render tests plus two CLI integration tests covering rank precedence/ties/bounds/noise, metadata/body isolation, reload, hash refusal, exact management verbs, composer suppression, and eight reviewed wide/narrow snapshots. The released disposable TUI walkthrough proved visible pre-submit suggestions, ranked non-activating guidance, exact canonical activation, one post-activation request containing only that body, unchanged config, and no session write.
- focused multi-run suite: PASS, 16 matching tests covering bounded definitions, route defaults/costs, commands/input safety, request-local route execution, missing-route zero traffic, temporary-Git candidate isolation, structured judge/fusion requests, usage/request ownership, and rendering. Fourteen reviewed wide/narrow multi-run snapshots and a released disposable TUI walkthrough proved three exact worktrees, two distinct diffs, one contained malformed-stream peer failure, sole-schema judgment/fusion, unchanged main/config state, no session, and retained branches.
- focused agent-graph suite: PASS across graph validation/CAS, legacy/defaulted persistence, fork remapping, restore interruption, stable dependency scheduling, exact worktree/grant preflight, child tool evidence, user-only retry/cancel, input locking, and fourteen reviewed wide/narrow `/tasks` snapshots. The released temporary-home walkthrough proved two concurrent roots, local evidence failure, blocked descendants, explicit retry with immutable attempts, exact second-worktree `README.md` isolation, and dependency release only after all gates passed.
- focused worktree suite: PASS across real-Git create/status/diff/cleanup, profile and symlink isolation, immutable child snapshots, attached workflow roots, Session V4 restore/portable export, commands/input/orchestration, and fourteen reviewed wide/narrow worktree surfaces. The released temporary-home walkthrough proved exact attachment, attached-root workflow execution, dirty refusal, default Cancel, clean removal, empty manifest, and preserved branch.
- focused workflow suite: PASS, 24 matching tests covering bounded loading/validation, scheduling, retries/evidence, output/state bounds, grants/tool intersections, session restore/forks, request isolation, commands/input/orchestration, and 16 reviewed wide/narrow workflow states.
- focused provider-message protocol suite: PASS, 11 canonical message tests plus 6 SSE/assembler tests.
- focused approval suite: PASS, 47 queue/policy tests plus production duplicate-denial and network-yolo regressions.
- focused session suite: PASS, 14 temp-root V1-V4 schema/store/delete/pin/archive/branch/profile tests.
- focused session-organization suite: PASS, 8 picker input tests, 10 orchestration tests, 5 render tests, and reviewed wide/narrow active/archive/confirmation/busy/error snapshots.
- focused session-portability suite: PASS, 16 format/filesystem/command/input/orchestration tests plus reviewed wide/narrow export-overwrite, export/import busy, import-failure, and command-palette snapshots; the isolated walkthrough proved Cancel-first overwrite, fresh profile-scoped UUIDs, runtime-state stripping, and rejection of unresolved tool calls without execution.
- focused session-retention suite: PASS, 16 config/core/command/input/orchestration/render tests plus 3 exact batch-store tests and 14 reviewed wide/narrow retention snapshots; the isolated walkthrough proved save-without-delete, exact protected preview, default Cancel, and confirmed three-file deletion.
- focused queue/orchestration suite: PASS, 6 queue-state tests plus persistence, failed-drain rollback, restored non-submission, no-session, retry, rewind, fork, input, and render coverage.
- focused context suite: PASS, 35 context/config/command/input/orchestration/render tests, including 15 context-limit-specific tests and ten reviewed wide/narrow editor snapshots.
- focused usage suite: PASS, 13 usage/parser/config/command/orchestration/render tests plus loopback review, provider-probe, and provider-subagent record assertions; four reviewed wide/narrow empty/populated/running/failure snapshots.
- focused local-provider discovery suite: PASS, 8 scanner tests plus command/input/orchestration coverage, 73 provider-filtered unit tests and 2 matching CLI tests, and eight reviewed scanning/populated/empty/failure snapshots at wide/narrow sizes.
- focused BTW suite: PASS, 9 command/input/loopback/orchestration/render tests plus the protocol-prefix characterization; requests are tool-free and exclude partial output, unresolved tools, hooks, images, and queued drafts; ten editing/running/complete/failure/cancelled snapshots were reviewed at wide/narrow sizes.
- focused keymap/history suite: PASS, 17 parser/config/command/input/orchestration/render tests, fixed Ctrl+R and emergency-control regressions, dynamic-label coverage, and 12 reviewed manager/capture/bare/error/history snapshots at wide/narrow sizes.
- focused hook-lifecycle suite: PASS, 38 runner/config/lifecycle/cleanup/non-leakage tests plus 6 manager input/orchestration/render tests and 16 reviewed wide/narrow snapshots; the isolated walkthrough proved responsive startup, exact Pre/Post/Failure order, repairable fail-closed SessionStart, bare refusal, actual Shift-letter handling, and timeout descendant cleanup.
- focused prepared-request suite: PASS, 27 request-filtered tests plus runner-specific review/probe/subagent/scripted/checkpoint coverage; every production runner uses typed preparation, current Chat Completions JSON is characterized, snapshots are process-only and adversarially bounded/redacted, and ten wide/narrow `/request` states were reviewed.
- focused central-tool-selection suite: PASS across resolver, CLI parsing, interactive/headless/scripted/review/checkpoint/subagent/MCP enforcement, no-tools models, unknown-name zero-traffic behavior, and reviewed wide/narrow failure snapshots. The isolated walkthrough showed exact `read_file`, `search_files` request order, bare omission of `activate_skill`, no provider POST for an unknown name, and unchanged `write_file` approval.
- focused rich-permission suite: PASS across typed/legacy evaluation, Ask precedence, canonical path and symlink denial, safe command prefixes, unsafe-shell fallback, interactive/headless/MCP/subagent routing, config patch preservation, bounded session grants, and `/permissions` orchestration; ten manager snapshots were reviewed at wide/narrow sizes.
- focused stronger-headless-contract suite: PASS across v1 stability, v2 lifecycle, fresh verify-only checks, exit 9, explicit repair authority, bounded local JSON Schema loading/reference rewriting, sole-final reports, local value validation, output-last-message, and zero-provider-traffic failures.
- focused OpenAI Responses suite: PASS across exact typed input/function/output serialization, flat tool schemas, reasoning/text/function deltas, nested usage, completed/failed/incomplete/refusal lifecycle, malformed/oversized/premature streams, auth omission, timeout/cancellation, provider checks, and default Chat Completions byte stability. The isolated loopback used `/v1/responses` and returned reasoning plus `OK` without persistent state.
- focused Anthropic Messages suite: PASS across exact top-level system/text/tool-use/tool-result serialization, flat tool schemas, effort, thinking/text/multiple-tool deltas, cache-aware usage, valid completion/refusal/error lifecycle, malformed/mismatched/oversized/premature streams, versioned conditional key auth, timeout/cancellation, provider checks/catalogs, and both earlier adapters. The isolated loopback used credentialless `/v1/messages`, returned reasoning plus `OK`, and created no session/profile state.
- focused provider-fallback suite: PASS across closed cause classification, exact rule validation/defaulting, provider deletion/model-rename maintenance, pre-output activation, partial-output refusal, one-route/no-chain behavior, cross-protocol backup preparation, tool-loop pinning, separate usage spans, next-turn primary restoration, Session V4 branching, and four reviewed wide/narrow provenance snapshots. The released TUI walkthrough showed the safe marker and backup answer after exactly two primary 503 attempts.
- focused change-review suite: PASS, 7 service/input tests and 4 reviewed snapshots.
- focused job/workspace suite: PASS, including macOS filesystem/loopback sandbox, ownership, traversal, symlink, creation, persistence, and instruction-boundary coverage.
- focused skill suite: PASS, 7 discovery/precedence/symlink/hash/catalog/budget tests plus tool, headless, session, and CLI coverage.
- focused MCP suite: PASS, 20 stdio/Streamable-HTTP trust/lazy/protocol/resource/schema/sandbox/network/cap/redaction/timeout/cancellation/cleanup tests plus 2 loopback CLI tests; the released HTTP walkthrough completed one namespaced tool loop with exact environment-referenced headers and session deletion.
- focused PDF suite: PASS across typed schema/profile/resource metadata, default/ranged extraction, output/file/span/page caps, encryption, malformed input, caught parser panic, workspace escape, terminal sanitization, embedded-file non-traversal, headless continuation, and four reviewed wide/narrow tool-card snapshots; the released loopback walkthrough extracted only page 2 and created no session.
- focused plan/session suite: PASS, 3 work reducer tests, 7 V1/V2/V3/store/branch tests, and typed orchestration/command coverage.
- focused subagent suite: PASS, 6 manager/limits/approval/scope/nested-inheritance/job/shutdown tests plus registry/session/headless coverage.
- focused fresh-review suite: PASS, 3 fake-provider isolation/protocol tests, 3 temporary-Git target/privacy/cap tests, plus registry/command/provider/session coverage.
- focused provider/model/setup suite: PASS, 106 provider-filtered unit tests plus 2 matching CLI tests, including exact template/custom actions, atomic optional-protocol persistence, duplicate/no-write and bare-mode refusal, catalog/config/secret/edit/delete coverage, isolated authenticated/credentialless probes, missing-environment zero-I/O behavior, setup routing, and deterministic finish selection.
- focused doctor suite: PASS, 6 unit/command/render tests plus 3 CLI tests covering offline no-write behavior, missing config, aggregate JSON, explicit loopback connection checks, positional-prompt compatibility, and secret-free output.
- focused config-hub suite: PASS, command/input/orchestration/persistence tests plus four reviewed populated/no-match wide/narrow snapshots; review-model selection preserves defaults, comments, credentials, and unrelated TOML.
- focused render suite: PASS, 112 tests, including reviewed wide/narrow OpenAI-template editor/provider/setup help, unclipped duplicate refusal, setup checklist, checking, working/failure, needs-model, finish, and no-passing-provider snapshots.
- focused input suite: PASS, 63 tests, including separate template/custom shortcuts, setup test/cancel locking, list/detail/finish routing, editor returns, and composer/paste isolation.
- focused profile suite: PASS, 7 named-profile tests, 6 bare/profile rendering and capability tests, 2 profile CLI tests, and four reviewed wide/narrow profile snapshots.
- focused sandbox-hub suite: PASS, 16 command/config/input/orchestration/render tests plus ten reviewed wide/narrow populated/empty/confirmation/busy/failure snapshots and isolated default/bare live-TUI walkthroughs.
- focused fake-backend/scripted-runner suite: PASS, 5 tests.
- APR-003 and UNI-004 introduced no strict-clippy diagnostics after focused cleanup.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS; TOL-016 cleared the prior tool/approval baseline.
- `cargo build --release`: PASS.
- `./target/release/ca --help`: PASS.
- `./target/release/ca --version`: PASS (`ca 0.1.0`).
- Isolated rebuilt-binary TUI checks confirmed the advertised shortcut and edit/key action, 105 merged models during the public Ofox refresh plus both MiMo and DeepSeek Pro fallbacks, the Add Provider direct-key field, bracketed paste rendering as a fixed eight-dot mask, and cancellation before persistence. The temporary home was removed; no user config, Keychain item, installed binary, credential-dependent DeepSeek/MiMo request, or broad Cursor visual parity is claimed.

Update these facts from actual command output after each validation pass.

## High-Risk Open Findings

- Partial-config defaults: COMPLETE under `CFG-109`. Omitted and partial
  `[agent]` tables now share the semantic defaults used by production config;
  focused tests cover no-rewrite loading, explicit-value preservation,
  serialized defaults, and the provider snapshot behavior.
- FreeBSD recovery regression: COMPLETE under `BSD-110`. Recovery V2 preserves
  directory identity, modes, and empty directories; FreeBSD symlink refusal,
  platform sandbox assertions, strict Clippy, complete tests, and release
  gates pass on the physical host.

1. Tool protocol continuity: COMPLETE under `PRO-002`. Interactive state remains canonical, and provider conversion emits one ordered tool-result message per resolved call while rejecting unresolved or duplicate results before I/O.
2. Approval coordination: COMPLETE under `APR-003`. Multi-call batches use an ordered queue, async execution is guarded against duplicate/stale completion, cancellation advances coherently, persistent denials retain error status, and provider continuation occurs once after exhaustion.
3. Event-loop blocking: COMPLETE for direct shell/job execution under `ASY-006`; nested runtime calls are removed and long-command responsiveness is tested. Other synchronous orchestration helpers remain separate investigation targets if reproduced.
4. Unicode editing: COMPLETE under `UNI-004`. Scalar byte-boundary helpers (`previous_char_boundary`, `next_char_boundary`) centralize the affected movement/edit paths; normal movement, backspace, delete, insertion, paste, Unicode Ctrl+W/@, and Vim a/h/l/x/w/b operate on UTF-8 scalar boundaries without panics.
5. Determinism: Cargo tests can now drive the extracted scripted-runner core through `ChatBackend` without credentials or network. The checked-in `tests/*.ca` scenarios still enter the live-provider `run_test` wrapper when invoked through the CLI.
6. Visual parity: reviewed snapshots now cover startup, streaming, completed tool, model picker, command palette, approvals, Unicode cursor positions, sessions/timeline, queues, context, and working-tree list/diff states at two widths. No broad Cursor parity claim has been made.
7. CLI safety: COMPLETE under `CLI-007`. Help/version/unknown-option handling occurs before runtime state or provider initialization, with local subprocess coverage.
8. Environment portability: COMPLETE under `ENV-009`. Production config, sessions, and history use injected runtime paths; missing `HOME` fails without a developer/cwd fallback, and raw config patches preserve secret placeholders.
9. Stream framing: COMPLETE under `SSE-008`. Byte-oriented framing, typed failures, complete finish/tool validation, partial-text preservation, queue failure behavior, and immutable `/retry` branching have hermetic coverage.
10. Sessions: COMPLETE under `SES-011` with deletion extension `SES-039`. Version 2 keeps canonical messages, stable IDs, atomic saves, parent metadata, and optional checkpoint metadata; v1 remains readable and upgrades on save. Confirmed deletion removes only one inactive exact-UUID file and never rewrites descendants.
11. Queue/steering: COMPLETE under `QUE-012`. Explicit turn state gates one-at-a-time draining; steering aborts tracked work and strips unresolved tool-call metadata before the next provider request.
12. Context: COMPLETE under `CTX-013` and transparency extension `CTX-038`. Canonical transcripts remain intact; request context is checkpoint plus suffix, failed/stale checkpoint generations cannot overwrite current context, and full-request category estimates remain process-only.
13. Terminal selection/lifecycle: COMPLETE under `TUI-015`. Mouse capture is not enabled while unhandled, and fake-control tests cover reverse cleanup, partial failure, and panic unwinding.
14. Cursor evidence: `PAR-005` recorded a blocked clean-runtime observation. The dated wrapper is modified, and the clean bundle is not a standalone CLI; every parity row remains `Unverified` until a stock versioned runtime is available.
15. Tool registration: COMPLETE under `TOL-016`. All 47 built-ins share one generated schema/validator/handler/profile record; unknown and colliding dynamic tools fail closed, typed outcomes distinguish denial/cancellation/failure, and distinct call IDs are never deduplicated.
16. Headless automation: COMPLETE under `HED-017`. Text remains default; aggregate JSON and JSONL share stable versioned events, approvals fail closed, structured stdout is diagnostic-free, and exits 2–8/130 plus broken-pipe 0 have hermetic coverage.
17. Structured interactions: COMPLETE under `INT-018`. Canonical requests resolve original call IDs without synthetic user messages; mixed batches retain order, modal drafts do not touch the composer/queue, and masked host input is nonserializable/redacted/zeroized.
18. Model reasoning/activity: COMPLETE under `MOD-019`. Exact profiles own capabilities and provider dialect fields, unprofiled models send no reasoning option, reasoning remains separate/collapsed by default, active phases are truthful, and queue drain requires an explicit completed outcome.
19. Workspace/process policy: COMPLETE under `WSP-020`. Stable roots contain all registry path/cwd resources before approval, prepared arguments are executed unchanged, persistent deny and network grants remain distinct from yolo, required sandbox mode fails closed, and job control is run/session-owned.
20. Filesystem skills: COMPLETE under `SKL-021`. Strict project/user/opt-in compatibility discovery exposes bounded metadata only; exact activation is approval-aware and budgeted, while session restore requires matching ID/path/SHA.
21. Local stdio MCP: COMPLETE under `MCP-022`. User/project configs remain inert on local exits, project servers require path-and-hash host trust, dynamic tools are typed and prompt-default, text resources and all outputs are bounded/sanitized/redacted, and sandboxed process groups are closed on every tested path.
22. Durable work state: COMPLETE under `PLN-023`. Session V3 upgrades lazily in the stable file, plan/task mutations use revision CAS, validation evidence must reference recorded events, approval is user-only, and branches either regenerate lineage IDs or visibly omit unavailable historical work state.
23. Asynchronous subagents: COMPLETE under `AGT-024`. The legacy raw execution path fails closed, children have stable bounded state and exact non-widening grants, approval waits isolate one child, jobs are owner-scoped and cleaned on cancellation/shutdown, and restored active summaries become interrupted without resuming.
24. Fresh-context review: COMPLETE under `REV-025`. Working/base/commit/custom inputs are argument-safe and capped on file boundaries, untracked contents remain private, the provider context excludes chat/work/skills/MCP/agents, only read tools plus the typed reporter are exposed, and successful findings persist with concise follow-up history.
25. Provider management: COMPLETE under `CFG-033`. `/provider [filter]` and `/providers` expose safe provider summaries, all configured/fallback/live models, provider-only settings, secure add/edit flows, affected-provider catalog refresh, and transactional provider deletion with deterministic current/default replacement.
26. Provider credential checks: COMPLETE under `CFG-034`. `/provider setup [filter]` checks only on explicit input, returns process-local typed outcomes, keeps resolved credentials and provider bodies out of state/output, selects only a passing pair, and routes normal 401/403 chat failures through setup before immutable `/retry`.
27. Named run profiles: COMPLETE under `CFG-041`. Default paths and Keychain references remain backward compatible; named config/credentials/sessions are isolated; missing named config is lazy; `bare` disables persistence, MCP, skills/activation, and hooks while keeping provider checks and in-memory branching available.
28. Sandbox management: COMPLETE under `SAN-042`. `/sandbox` and `/config` report saved/effective modes, backend readiness, writable roots, and truthful network enforcement; weaker modes are Cancel-first and next-launch-only, while bare is view-only.
29. Context-limit editing: COMPLETE under `CTX-043`. Exact model settings inherit field-by-field, validation happens before writes, profile capability fields and TOML comments survive patches, saves apply between turns, and bare remains view-only.
30. Process usage/cost accounting: COMPLETE under `USG-044`. Every existing TUI provider request has one safe provider/model record with duration, typed outcome, and optional detailed usage; exact configured decimal prices never fall back to an inferred table, and request/response/credential content is excluded.
31. Local provider discovery: COMPLETE under `PRV-045`. The explicit scan has fixed loopback authority, no proxy/redirect/credential path, bounded work and responses, process-only stale-safe state, and a deliberate transition into the existing Add Provider form.
32. Ephemeral side questions: COMPLETE under `BTW-046`. `/btw` remains available during active main work, freezes a protocol-valid canonical prefix, uses no tools/hooks/MCP or persistent state, has independent Esc cancellation/stale generations, and accounts for completion/failure/cancellation without changing the main turn.
33. Keymaps and history search: COMPLETE under `KEY-047`. One validated profile-scoped resolver owns only approved global composer actions; conflicting/reserved/prefix bindings fail before activation, Ctrl+R and safety/modal controls stay fixed, and `/history` restores process-only drafts without persistence or submission.
34. Session V4/no-session/queue persistence: COMPLETE under `SES-048`. V1-V3 files load lazily, queue identity/order persists in V4, restored drafts never auto-submit, and `--no-session` disables only storage while retaining extensions and in-memory branch operations.
35. Session archive and pinning: COMPLETE under `SES-049`. `/resume` separates Active and Archive views, pins without changing recency, transfers exactly one UUID-validated inactive file without overwrite or reserialization, preserves descendants, and isolates default/named profile archives.
36. Session import and export: COMPLETE under `SES-050`. `/export` writes bounded Markdown or normalized portable JSON without serializing runtime authority; `/import` accepts only strict local portable JSON, rejects unresolved/executable state, assigns a fresh profile-scoped UUID, and opens `/resume` without replacing the current session.
37. Session retention: COMPLETE under `SES-051`. Optional age/count rules are disabled by default and never prune automatically; exact Cancel-first previews protect current/pinned/archived sessions, and confirmed batches preflight every file before non-cascading unlink with truthful partial-failure reporting.
38. Complete hook lifecycle: COMPLETE under `HOK-052`. Interactive main-session hooks execute asynchronously through one bounded runner, exact lifecycle boundaries cannot duplicate dispatch, fail-closed startup stays repairable in `/hooks`, and timeout/cancellation/shutdown clean the full process group. Other runners remain hook-free by explicit scope.
39. Prepared request visibility: COMPLETE under `REQ-053`. Every current model runner constructs one typed semantic request before I/O; the adapter preserves existing Chat Completions serialization, while a single latest process-only `/request` snapshot is independently bounded and sanitized without retaining runtime endpoint/header/credential authority.
40. Central tool selection: COMPLETE under `TOL-054`. One process-only resolver applies replacement, inclusion, exclusion, profile/capability/grant restrictions, and final built-in/MCP validation before provider traffic; provider calls outside the advertised set fail closed and selection never grants execution authority.
41. Rich permission policy: COMPLETE under `APR-055`. One evaluator now applies Deny > session grant > Ask > Allow across interactive, headless, MCP, and provider-subagent calls; unsafe shell syntax never auto-runs, network remains separately authorized, legacy rules are preserved, and `/permissions` safely inspects/removes persistent and process grants.
42. Stronger headless contracts: COMPLETE under `HED-056`. Ordinary output remains `ca.headless.v1`; opt-in v2 adds fresh verify-only checks with a distinct failed-report exit, explicitly authorized repair, and bounded locally validated structured JSON through a sole-final report tool.
43. Protocol-neutral provider boundary: COMPLETE under `PRV-057`. Missing protocol config remains exact Chat Completions without a rewrite; the adapter owns endpoint suffix, conditional auth, stable retry bytes, SSE/event/usage translation, and body-free failures while all runners retain the prepared-request/canonical-event contract.
44. OpenAI Responses: COMPLETE under `PRV-058`. `protocol = "openai_responses"` opts one provider into typed `/responses` input and event translation, exact function continuation, detailed usage, shared deadlines/cancellation, and payload-free failures; the omitted protocol remains frozen Chat Completions.
45. Anthropic Messages: COMPLETE under `PRV-059`. `protocol = "anthropic_messages"` opts one provider into versioned `/messages` key authentication, top-level system/client-tool translation, thinking/text/tool/cache-usage events, shared deadlines/cancellation, and payload-free failures; Chat Completions and Responses remain unchanged.
46. Codex/ChatGPT authentication: `AUTH-060` was superseded by the explicitly approved `AUTH-061` shared-cache integration. The exact `openai-codex` provider now accepts the documented owner-only `$CODEX_HOME/auth.json` cache, adds the matching ChatGPT account header, sends the Codex-specific `store: false` request shape, accepts the documented backend request-ID header, and preserves strict metadata checks while allowing `auto` service tier to resolve to its terminal tier. Browser cookies and use by any other provider remain forbidden. Native FreeBSD authentication passed as unprivileged user `aji` with `FINAL_AUTH_OK`.
47. Explicit provider fallback: COMPLETE under `FAL-061`. Exact configured pairs may switch once only for allowed exhausted pre-output failures, pin the selected provider/model through tool continuations, persist secret-free per-turn provenance, and return to the unchanged primary on the next user turn; exact-provider automation remains unaffected.
48. Streamable HTTP MCP: COMPLETE under `MCP-062`. Explicit HTTPS or literal-loopback HTTP servers reuse exact project trust, distinct network grants, namespaces, approvals, calls/resources, reconnect, and shutdown while rejecting unsafe authority before I/O and bounding/redacting every HTTP response surface.
49. MCP OAuth: COMPLETE under `MCP-063`. Explicit Streamable HTTP OAuth uses pinned discovery, browser PKCE or an advertised public-client device flow, profile/server-scoped Keychain storage, typed authorization status, refresh rotation, and exact disconnect; catalog loading never launches authorization and no remote or credential data crosses the safe manager boundary.
50. Deferred MCP tool discovery: COMPLETE under `MCP-064`. Ordinary interactive/headless requests start no MCP server and advertise only the bounded search built-in; explicit search starts at most eight eligible servers, atomically activates at most ten exact schemas for later turns, invalidates affected activations on catalog/lifecycle changes, and feeds only that active set into central policy plus `/context`/`/request` accounting.
51. Semantic navigation: COMPLETE under `LSP-065`. Only explicitly configured enabled executables produce six Main-profile schemas; the lazy stdio manager converts exact user positions, synchronizes bounded text, rejects every non-workspace location, and reaps owned process groups across timeout, malformed response, cancellation completion, and shutdown.
52. PDF text reading: COMPLETE under `DOC-066`. The read-only `read_pdf` tool parses bounded workspace files off the async runtime, extracts only exact selected pages through a capped UTF-8 writer, reports honest continuation metadata, and fails safely for encrypted, malformed, oversized, panic-prone, or out-of-authority inputs without opening attachments/actions.
53. Typed multimodal content and image inspection: COMPLETE under `MM-067`. Typed request blocks preserve text-only wires while exact image-capable profiles gain bounded adapter serialization; normalized clipboard and `view_image` payloads remain process-only, durable surfaces receive safe unavailable markers, and the tool payload reaches exactly one continuation without widening authority.
54. Project and user memory: COMPLETE under `MEM-068`, with defaults extended by `LRN-134`. User and canonical-workspace stores are profile-scoped and missing files remain inert. Missing recall/proposal switches now default on while explicit `false` remains authoritative; injected indexes are independently capped and marked stale. Ordinary model-generated entries remain process-only until explicit Cancel-first approval in `/memory`, and automatic maintenance can mutate only separately marked Dream-owned records.
55. Goal sessions: COMPLETE under `GOA-069`. Session V4 now carries bounded finite goal state; fresh evaluator requests expose only a non-executable typed report schema, request-only feedback can continue main work, and every approval/user/error/limit boundary pauses or terminates without widening authority.
56. Declarative workflow engine: COMPLETE under `WFL-070`. Bounded local TOML/JSON DAGs validate before traffic, deterministic steps reuse the canonical provider/tool/approval loop with exact route/tool/grant authority, bounded outputs and local evidence/retries persist safely in Session V4, and interruption/restoration require explicit resume.
57. Worktree lifecycle: COMPLETE under `WT-071`. Profile-owned Git worktrees have derived paths and collision-resistant branches, repeat identity/cleanliness validation, bounded inspection, explicit attachment, immutable child inheritance, attached-root workflow loading, and confirmation-only cleanup that never force-removes a dirty tree or deletes its branch.
58. Dependency-aware agent graph: COMPLETE under `AGT-072`. Approved plan tasks may carry bounded acyclic dependencies, exact child grants, one existing owned worktree, attempts, and local evidence gates; explicit `/tasks` controls schedule only ready nodes through `AgentManager`, pause on failure, preserve failed attempts, and require user-confirmed retry before releasing descendants.
59. Multi-run and best-of-N: COMPLETE under `RUN-073`. An explicit process-only manager freezes one request, schedules two to five exact-route candidates through `AgentManager` in separate owned worktrees, records bounded local evidence, and gates fresh structured judgment/fusion behind separate confirmations without changing canonical conversation, defaults, sessions, the attached workspace, or candidate branches.
60. Semantic skill suggestions: COMPLETE under `SKL-074`. Exact IDs, declared aliases, and bounded lexical BM25 rank safe discovered metadata for the idle composer and `/skills` pager; results are explicitly non-activating, canonical-ID-only for action, process-local, and unable to load bodies or alter prompt/session/approval/hash boundaries.
61. Guided changes walkthrough: COMPLETE under `CHG-075`. `/walkthrough` and `/explain-changes` accept working/base/commit targets, refuse every incomplete snapshot before traffic, obtain one bounded ordered report through a sole non-executable schema, retain only safe process state, and visibly mark exact source changes stale until explicit recapture.
62. Lazy history storage: COMPLETE under `SES-076`. New session writes use stable metadata plus generation-scoped append-only JSONL history, bounded paging, full-hydration semantic gates, and backward-readable V1-V4 migration.
63. Redo: COMPLETE under `SES-077`. Exact process-only file-mutation receipts support drift-safe `/undo` and `/redo`, and any new mutation clears the redo path.
64. Soft pause: COMPLETE under `TUR-078`. `/pause` defers only at completed provider/tool/finalization boundaries, `/unpause` resumes exactly once, and cancellation remains distinct.
65. Durable terminals: COMPLETE under `TRM-079`. `/term` manages exact user-owned tmux sessions while terminal restoration is explicit and normal `ca` shutdown never kills them.
66. Themes and status surfaces: COMPLETE under `TUI-080`. Closed profile-scoped theme/status choices preserve legacy defaults, and optional custom status output is bounded, process-only, network-denied, and command-hidden.
67. JSONL stdio service: COMPLETE under `SRV-081`. `ca serve` runs one bounded process-only `ca.serve.v1` session with canonical requests/events, exact client approval and interaction responses, typed lifecycle/errors, and deterministic owned-resource cleanup without saved-session writes.
68. Loopback WebSocket and attach: COMPLETE under `SRV-082`. `ca serve --listen` accepts one externally authenticated literal-loopback client and `ca attach` bridges the unchanged bounded JSONL contract; token values remain outside CLI/state/output, reconnect starts a fresh process-only lifecycle, and stdio remains unchanged.
69. Scheduled tasks: COMPLETE under `SCH-083`. Leading `ca schedule` manages inert profile-scoped definitions, serializes exact due claims, skips missed work without replay, executes pinned prompt/workflow requests through isolated no-session scheduled workers, and bounds delivery to exact local files or explicitly authorized signed webhooks.
70. Local plugin bundles: COMPLETE under `PLG-084`. Leading `ca plugin` registers strict local manifests inertly, derives capabilities, requires exact-confirm collision-free unchanged activation, composes only existing skill/MCP/hook/workflow/theme types, and never loads native code or deletes source bundles.
71. Signed curated catalog: COMPLETE under `PLG-085`. `ca plugin catalog` pins local Ed25519 roots, verifies exact monotonic snapshots and package/file hashes, stages private versions, keeps install/update/rollback disabled, protects local-source plugins, and performs no automatic network, update, activation, or deletion.
72. Browser automation: COMPLETE under `BRW-086`. The `browser` tool is absent by default, drives only an existing local Chrome/Chromium through an owner-bound ephemeral loopback-CDP process, preserves distinct approval/network authority, and bounds/sanitizes every DOM, screenshot, download, runtime, and cleanup surface.
73. Voice input: COMPLETE under `VOC-087`. `/voice` captures only after an explicit Space press, owns bounded audio for one generation, transcribes through one configured local/provider backend, and inserts only an editable unsent draft with no audio/session persistence.
74. Signed external runtime manager: COMPLETE under `EXT-088`. Leading `ca runtime` pins exact source/trust authority, verifies closed signed multi-file catalogs, streams confirmed installs/updates into private retained versions, revalidates rollback/path/remove, and performs no automatic discovery, download, execution, or integration activation.
75. Managed llama.cpp and GGUF: COMPLETE under `LOC-089`. Leading `ca local` manages exact private GGUF bytes; explicit local runs resolve only a reverified EXT-managed `llama-server`, supervise a fixed sandboxed loopback process, inject a credentialless provider only in memory, write no session/config, and kill the entire owned process group on exit.
76. Explicit self-update: COMPLETE under `UPD-090`. Leading `ca update` separates signed manifest verification, exact artifact staging, invoking-executable replacement, and verified rollback behind explicit commands and confirmations, with no background network or installed-binary mutation during validation.
77. Full program completion audit: COMPLETE under `AUD-091`. The durable 49-item matrix ties every requirement to current source, focused tests, and isolated walkthrough evidence; exact inspectable `ServeOp`, `ServeEvent`, `ScheduleDefinition`, `PluginManifest`, and `AgentDependencyEdge` public boundaries are present without changing wire/runtime authority.
78. OpenAI provider template: COMPLETE under `CFG-092`. `/provider` and `/provider setup` expose a distinct explicit-save template for the official API root, `OPENAI_API_KEY`, `gpt-5.6-sol`, and `openai_responses`; opening is inert, existing `openai` is never overwritten, generic adds remain protocol-free, and setup defaults do not switch before finish.
79. Provider template catalog: COMPLETE under `CFG-093`. `Shift+P` and `/provider templates [filter]` expose all 40 audited Hermes guide rows. CFG-093 initially classified 11 rows ready, direct OpenAI limited, and 28 rows inert; PRV-094 subsequently superseded that behavior classification.
80. Reasoning-state continuation: COMPLETE under `PRV-094`. Canonical OpenAI Responses retains and replays the bounded completed output sequence without `previous_response_id`; exact DeepSeek and MiMo Chat routes retain and replay byte-faithful `reasoning_content`. State is bound to the actual provider/protocol/normalized-endpoint digest, persists in direct sessions, stays absent from portable/checkpoint/request/UI surfaces, and cannot cross routes. The catalog now has 14 ready rows and 26 rows requiring another integration.
81. Strong-planner/cheap-executor experiment: COMPLETE under `RND-095` for the approved offline scope. A non-default separate binary validates one immutable visible `reasoning_artifact.v1`, optional declared dependency-path freshness, and explicit `planner_executor_cost_runs.v1` fixtures; it performs no provider call, command execution, live routing, recovery, or session/TUI integration. Live efficacy and dollars per verified success remain benchmark questions, not implementation claims.
82. Durable automation and LaunchAgent support: COMPLETE under `SCH-099`. V1 schedule bytes remain frozen while strict V2 definitions, deterministic UTC/DST occurrence IDs, immutable hash-chained journals, claim CAS, root-scoped leases, conservative dead-owner interruption, exact SKL-098 pins, pinned Git/WSP mutation isolation, direct clean-environment script jobs, reconstruction-proven recovery artifacts, offline history/doctor, drain/daemon management, and exact-confirmation secret-free LaunchAgent lifecycle are available.
83. Typed Himalaya mailbox tools: COMPLETE under `EML-100`. Exactly six explicitly selected v2.0.x tools provide bounded account/list/read, immutable draft, exact send, and frozen reply operations. Outbound communication requires exact one-use recipient/draft/call/session approval; private content is independently omitted from durable and routine observability surfaces, and post-start ambiguity is never retried.
84. Dedicated draft-first email gateway: COMPLETE under `EML-101`. A distinct empty-allowlist dedicated account bootstraps at UID high-water, filters automation before one fresh zero-tool request, and appends deterministic plain-text `\Draft` messages only. Profile/account Keychain isolation, bounded MIME without attachment bodies, hash-chained UID state/private recovery, exact Message-ID ambiguity settlement, offline/connect doctor, SIGTERM-aware daemon, exact-confirmation LaunchAgent lifecycle, and canonical scheduled EmailDraft recipients are enforced.
85. Automated provider authentication: COMPLETE under `AUTH-104`. `/provider` connects or reconnects first-class MiMo, OpenAI Codex, DeepSeek, and direct OpenAI routes using only their supported authentication contracts, hardened platform storage, transactional probes, starter-model selection, and shared runtime credential resolution. macOS and physical-FreeBSD full gates pass; existing Codex accounts on both systems passed doctor-route and ordinary marker requests without exposing credentials.
86. MiMo authorization handoff repair: COMPLETE under `AUTH-105`. Xiaomi receives the direct MiMoCode `/authorize` URL; FreeBSD automatically exposes it outside the alternate screen as a raw logical line, Ctrl+Y repeats the borderless handoff with tmux/screen OSC 52 passthrough, and the bordered popup no longer renders the long URL. Full macOS and physical-FreeBSD gates and a canceled native PTY walkthrough pass; verified binaries are installed in both existing locations.
87. Reliability-first oracle roadmap: OBS-117 through PRJ-126 are COMPLETE in the current uncommitted delivery tree. Provider tracing is explicit and profile-isolated; `/init` is one-request and Cancel-first; post-edit LSP diagnostics are best-effort and version matched; `/audit` is deterministic and read-only; `/raw` emits only a privacy-filtered visible transcript; `/usage` keeps recognized provider-account scopes separate from the local ledger; retained compaction preserves complete exchange suffixes; one-turn routes restore at the top-level lifecycle boundary; command-output compression stays presentation-only; and project scratch is bounded/profile-isolated. TUI-127 is also complete as a non-parity extension. No task is active.

These are source-derived investigation targets, not confirmed bug reports until a focused test reproduces them.

## Documentation Conflicts

- The active checkout is `/Users/aji/project/ca`; use repository-relative paths in documentation and code so moved checkouts remain valid.
- Project notes say MiMo is primary and DeepSeek is expired, while `src/config.rs` still contains DeepSeek defaults and environment fallback logic. Treat runtime provider choice as external configuration; do not encode a new default without a dedicated decision.
- `ROADMAP.md`, `SPEC.md`, `TODO.md`, and `TASKS_FOR_AGENTS.md` mix target behavior with old completion state. Their status banners now route execution to this harness.
- Session/config locations described by old docs are not uniformly accurate. Source currently controls them; see `ENVIRONMENT.md`.
- The Cursor reverse-engineering checkout contains patched working files and clean backups. Do not assume the patched tree represents stock Cursor behavior.

## Stable Decisions

- Full parity means observable behavior in the scope declared by `PARITY.md`, not source equivalence.
- Cursor reverse-engineering evidence is the primary parity oracle.
- Codex is a secondary source of engineering patterns: deterministic TUI snapshots, explicit state-machine docs, narrow module instructions, and layered validation.
- Oracle trees are read-only and must not become build dependencies.
- New behavior needs deterministic tests before a broad parity claim.
- Complex or protocol-sensitive work uses an ExecPlan.
- The previously preserved working-tree delta was integrated as baseline
  commit `06015abc90e567de4a440f03516ab5de3487767d`. The separate preservation
  archive remains recovery evidence, not current integration work.
- Provider templates are explicit user actions, not startup seeds or load-time migrations. Template-only protocol choice is typed through the editor/save boundary and must not be inferred from provider IDs or endpoints.
- Opaque provider continuation state is scoped by the actual provider ID, protocol, and normalized endpoint digest selected by the runtime client. Direct sessions preserve it; portable exports, checkpoint prompts, request snapshots, logs, and UI surfaces omit it.
- Cross-model handoff uses provider-neutral visible work product, never opaque provider continuation state. `reasoning_artifact.v1` is an intentional `ca` research extension; the offline prototype validates caller-supplied provenance and repository anchors, while any future live harness must inject those fields and execution authority itself.

## Next Handoff

LRN-177 is complete on top of LRN-175/LRN-176. The real heterogeneous archive
now completes direct learning without stale-ID poisoning, foreign knowledge is
included in the search and reusable-asset inventory, and the final source
receipt includes Hermes. No learning task is active. Do not rerun `/learn`,
undo run `020c3875-ef34-4a0e-b034-51708fe22b0a`, create an intermediate commit,
or install the release binary without a new explicit request.

OBS-117 through PRJ-126 and the user-requested TUI-127 extension also remain
complete in the uncommitted delivery tree. TUI-127's focused/default/all-feature
format, Clippy, release, snapshot, and disposable native PTY gates pass;
physical FreeBSD was not requested or run for that extension.

Cursor parity remains unverified without a clean runnable oracle. The RND-097
paid hypothesis remains unmeasured and must not be reopened without its
credential, pricing, preregistration, and spend gates.

## Update Protocol

After meaningful work:

1. replace stale verification results with actual commands and outcomes;
2. move confirmed bugs from “investigation target” into `NEXT_STEPS.md` with evidence;
3. record durable decisions and unresolved conflicts here;
4. update `PARITY.md` when oracle scope or evidence changes;
5. update `ARCHITECTURE.md` when ownership or data flow changes;
6. remove obsolete statements rather than appending contradictory history.

### HRS-253 current completion-check checkpoint

The normal host default workspace run is terminal (session 20670): ca bin
2607 passed/0 failed/24 ignored; ca CLI 38 passed; role-run CLI 17 passed;
codex-team lib 575 passed/0 failed/1 ignored. Three pre-existing integration
targets remain red: mcp_sdk (missing repo_query/delegate behavior),
protected_baseline_cli, and remote_cli (unrecognized remote command). These are
not repaired by weakening expectations. Log: `/tmp/ca-hrs253-workspace-current-default.log`.
The prepared-request schema regression passes (session 57089), now counting and
asserting the already-registered install_binary tool. Strict default workspace
Clippy passes (session 65989, `/tmp/ca-hrs253-current-clippy2.log`). Removed
unreferenced spinner code, derived BlockStyleConfig’s identical default, renamed
an unused test binding, shared the build identity module instead of loading it
twice, simplified the idle settings initializer, and named the verifier capture
result type. No semantic verifier change. The first current Clippy run failed on
the latter three HRS-253 issues and its log remains. All-feature lint and test
runs have been launched; results pending. Automatic engineering environment
choice remains pending; no sandbox weakening, install or runtime config change.

Current all-feature results: 3430 passed, 9 failed, 26 ignored; the same three
codex-team integration targets remain red (`/tmp/ca-hrs253-workspace-current-all.log`,
session 66148 terminal). Default totals: 3281 passed, 9 failed, 25 ignored.
Strict default and all-feature Clippy now pass (`/tmp/ca-hrs253-current-clippy2.log`,
`/tmp/ca-hrs253-current-clippy-all2.log`). The standalone experiment explicitly
expects unused portions of its shared materialization module; the main binary
remains strictly linted. Repository-wide format and diff checks pass after only
the three formatter-reported files were formatted (event.rs, render.rs,
worktree_manager.rs); no semantic change in those formatting hunks. Logs:
`/tmp/ca-hrs253-current-format2.log`, `/tmp/ca-hrs253-current-diff.log`.

Next independent work: real PTY idle discovery/generation/foreground-pause test
in a disposable HOME/config/vault using a local fake provider and synthetic
failure evidence, without waiting for the engineering-environment choice. The
production 300-second idle clock should run unchanged. Use the real dirty-source
snapshot, then pause at generation settlement before engineering gates. Session
20543 is currently building a fresh default ca and replacing only the private
signed worker at the task’s work/hrs253-worker/ca; log
`/tmp/ca-hrs253-idle-pty-build.log`. All prior named sessions this turn are terminal.
Do not overwrite that worker while any new process test runs. No PTY test has
started yet and no release or install has run.

Fresh default debug build/signature verification passed; session 20543 is
terminal (`/tmp/ca-hrs253-idle-pty-build.log`). The private worker is current,
but no PTY test has started and no installed executable was changed. All named
sessions in this checkpoint are terminal. The next independent test should use
a disposable profile and the unchanged real five-minute idle interval.

### HRS-253 real-clock PTY probe in progress

Added an ignored typed fixture seeder in harness_integration.rs: it writes only
a new ca-hrs253-idle-pty-* temporary home, seeds synthetic current-build failures
and a fixture pricing catalog, configures a localhost provider, and verifies
discovery selection. It is setup evidence, not a lifecycle pass. Initial fixture
compile missed PathBuf; fixed. Fresh matching default worker build and seeder
passed. The first Python attempt was stopped while moving the TUI workspace out
of the source checkout; it briefly launched PID 19609, which is verified exited.

Session 20678 runs the corrected PTY driver. Root:
`/private/var/folders/nx/1xjt7xc96zg_d9w7glkcyvhr0000gn/T/ca-hrs253-idle-pty-7v4lzhg7`.
TUI PID 19622 is live; it uses a separate empty trusted workspace, temporary
HOME/config/data/vault, and only a localhost fake provider. The production idle
interval stays 300 seconds. Script:
Codex task `work/hrs253_idle_pty.py`; log `/tmp/ca-hrs253-idle-pty-run2.log`.
Expected next: fake generation response, an actual keyboard draft before response
settlement, foreground pause with one settled request and zero trials, preserved
original Git index, then graceful exit. Results pending. Do not replace the
private worker while this test is live. Sessions 5805 and 2615 are terminal.

### HRS-253 real-clock PTY result and terminal probe fix

The previous PTY attempt (session 20678, run2 log) failed after 440 seconds:
no generation request, no cycle, forced shutdown. A one-second sample of its
owned TUI process showed ca-terminal-theme-probe blocked in File::read while
input polling was suspended. The 400ms deadline bounded poll but not the read.
terminal_awareness.rs now opens /dev/tty nonblocking, checks POLLIN, and retries
WouldBlock/Interrupted only within that deadline. The two existing terminal
awareness tests pass; the previously failing real PTY scenario now passes.

Session 10860 is terminal, exit 0. Fresh matching worker/seeder, log
`/tmp/ca-hrs253-idle-pty-run3.log`, fixture ca-hrs253-idle-pty-jn_sd424:
one local generation request at 334.210 seconds, foreground pause at 336.404
seconds, one settled charge, zero trials, no outstanding reservations, matching
idle ownership, unchanged original Git index, graceful TUI exit status 0 and no
forced shutdown. Initial background maintenance and preparation account for
elapsed time beyond the unchanged 300-second inactivity threshold. This proves
idle discovery/generation and foreground pause, not a complete idle evaluation
or real-provider improvement. No installed binary or user profile was changed.
The automatic engineering-environment question remains pending.

Post-fix validation is complete: ca binary tests 2607 passed, 0 failed,
25 ignored (`/tmp/ca-hrs253-terminal-final-tests.log`, session 47541 terminal);
all-target/all-feature strict Clippy passed (`/tmp/ca-hrs253-terminal-final-clippy.log`,
session 37768 terminal), whole-repository formatting passed
(`/tmp/ca-hrs253-terminal-final-format.log`), and diff check passed. No test or
build sessions remain running from this checkpoint. The previous workspace-wide
nine unrelated integration failures remain recorded; this focused rerun does not
supersede those failures. No release/install has run. Next work remains automatic
engineering environment, escaped-descendant containment, provider context proof,
recovery completeness and full source/idle validation.

### HRS-253 settled budget exhaustion and admission

A real admission regression is being repaired: a cycle with no promotion decision
and no unresolved provider/time reservations still blocked every later cycle when
its stage was BudgetExhausted. A focused idle-admission regression failed against
the previous compiled implementation (`/tmp/ca-hrs253-exhausted-before-actual.log`).
The initial --exact invocation matched zero tests and is not evidence of a pass.
Both explicit freezing and idle admission now share the host-owned
harness_control::blocks_new_cycle predicate. A settled pre-decision exhausted
cycle releases admission; its stage, charges, evidence and history remain intact.
Idle cooldown and unused-evidence selection are unchanged. Unknown reservations,
recovery states, and exhausted cycles with a saved decision remain blocking,
because a decision-bearing cycle could have interrupted deployment.
Focused HRS-253 checks are running in session 69490,
`/tmp/ca-hrs253-exhausted-after.log`; no worker executable was replaced.

The admission fix passed 94 focused HRS-253 tests (20 opt-in checks ignored)
and all 2608 ca binary tests (25 ignored). Logs:
`/tmp/ca-hrs253-exhausted-after.log`, `/tmp/ca-hrs253-exhausted-ca-tests.log`.
Session 69490 terminal success. Session 59629 terminal failure in Clippy only:
HarnessStage became test-only in harness_freeze.rs after sharing admission;
moved that import into the test module. Strict lint/format/diff rerun is session
3550, `/tmp/ca-hrs253-exhausted-clippy2.log`. No behavior changed in the import fix.

Lint rerun 3550 exposed the integration-test sibling's implicit dependency on
its parent's HarnessStage import; that test module now imports the enum explicitly.
Session 3550 terminal failure; final strict lint/format/diff rerun is 45428,
`/tmp/ca-hrs253-exhausted-clippy3.log`. These are import-only follow-ups to the
passing 2608-test run. The private signed PTY worker still contains the previous
source identity: rebuild it before creating any new current-source process
experiment, and never overwrite it while an older experiment is running.

Final admission-fix checks passed: all-target/all-feature strict Clippy, whole
repository format, and diff check (`/tmp/ca-hrs253-exhausted-{clippy3,format,diff}.log`).
Session 45428 is terminal, exit 0; all sessions launched for this fix are terminal.
Behavioral coverage: 94 focused tests and 2608 ca tests passed; final follow-ups
only made test enum imports explicit and were checked in all-target Clippy.
No release/install, user-vault mutation, paid request, or isolation-policy change.
The broader recorded workspace integration failures and automatic engineering
sandbox incompatibility remain unresolved. Goal remains active.

### HRS-253 completion audit after release build

The original six-part request remains the completion scope. Current evidence:

| Requirement | Current implementation/evidence | Completion limit |
| --- | --- | --- |
| Trustworthy success measurements | session_outcome separates completion, scoped verification and explicit feedback; interruption, mutation invalidation, legacy and storage-failure regressions pass in the 2608-test ca run. | Runtime mechanism verified; does not label model claims or ordinary exit as acceptance. |
| Actual runtime fingerprints | harness_registry hashes prepared instructions/tools, effective route/options and compiled identity; runtime wiring covers TUI/headless/service/delegation. Current-build identity regression passes. | The older disk-inventory CLI remains explicitly separate. |
| Inspectable Analyze evidence | learning validates event IDs, independent task counts and supplied session references; invented-reference regression passes. | Causal explanations remain hypotheses; no real-provider efficacy claim. |
| Matched experiments and trends | Frozen 9-case/54-trial design, held-outs, host verification, regressions/cost/time gates and chronological comparable-cohort rates. Controlled guidance/hint end-to-end runs and current focused checks pass. | Actual ca source-candidate engineering matrix does not pass its current execution environment. |
| Candidate lifecycle | Discovery, private source/guidance candidates, budgets, trials, signed activation, journaled rollback and controls exist. Guidance/hint controlled lifecycle tests pass. Real-clock TUI creation/generation/pause/clean exit passes. | Full automatic source and idle evaluation still unverified; unknown-charge recovery remains conservative. |
| Measured execution guidance | Candidate policy/exposure binding, completed-call thresholds and bounded reminders implemented; controlled 54-trial hint activation/rollback passed. | A deterministic fake provider proves the mechanism, not benefit on real tasks. |
| User-approved idle/budget/source adoption | Five quiet minutes, cooldown, one outstanding cycle, $20/7200 active seconds, clean branch adoption and preserved source/binary receipts. Settled pre-decision exhaustion now permits later admission. | Escaped process-group descendants and provider-specific context proof remain unresolved; no paid live cycle yet. |
| Validation and installation | Current ca tests, strict all-feature lint, formatting and diff checks pass. Full release workspace build and signed artifact smoke checks now pass. | Previously recorded nine workspace integration failures remain; installed executable is unchanged. |

Release command `cargo build --release --workspace --all-features` passed in
2m19s, session 21770 terminal, `/tmp/ca-hrs253-release-validation.log`.
An independently copied/ad-hoc-signed artifact passed strict signature verification,
mode 0755/uid 501, --version, --help and --internal-harness-build-identity under
a new temporary home with a cleared environment. Its hash remained unchanged
through the smoke checks. Session 25114 terminal; `/tmp/ca-hrs253-release-smoke.log`.
Private candidate SHA-256: 7bbfdf18bb787595945ebddf6df0851c3f3037635bc21522b99967d17ce73fed.
Compiled identity: 7f4084a6dd1909010d68bf839f673ea952d5322de7baaaf8ba8ba45081c3634d.
Artifact: Codex task work/hrs253-release-validation/ca; machine-readable smoke
receipt: work/hrs253-release-validation/result.json. No installation performed.

Reinspection confirms engineering and worker cleanup currently signal only the
original process group. The passing engineering timeout/cancellation regression
spawns a direct child that execs sleep, so it does not prove cleanup after setsid.
This unresolved containment requirement belongs in the engineering-environment
choice, not in a claim based on the direct-child test. The requested macOS VM vs
isolated-home host choice is still unanswered. No environment policy was changed.
All processes launched in this continuation are terminal. Goal is not complete.

### HRS-253 blocked audit — execution environment decision

The same automatic engineering-environment blocker has persisted through more
than three consecutive goal turns. Independent terminal, admission, lint and
release validation progressed in those turns and is now finished. Reinspection
of the production profile still shows network_allowed=false and
hide_other_processes=true; the retained real-workspace log still demonstrates
localhost bind EPERM and nested sandbox_apply EPERM. The signed private release
still matches its recorded SHA-256. No validation handle remains live.

Completion remains unproven. Proceeding with full automatic source evaluation
requires resolving the outstanding dedicated macOS VM versus isolated-home host
choice. This is an implementation/isolation decision requested from the user,
not a claimed user prohibition on host execution. It also determines the process
containment work; direct-process-group tests do not cover escaped descendants.
Provider-specific context proof, conservative unknown-charge recovery, the
recorded workspace test failures, full source/idle lifecycle and installation
remain open and are not waived. No deployment, configuration change or broader
execution grant was inferred from the unanswered question. The goal is being
marked blocked pending that decision; do not mark the plan complete.

The user selected a dedicated macOS VM on 2026-09-23. Resume HRS-253; the previous
choice blocker is resolved. See the active plan's new VM implementation section.
The goal tool still reports blocked and does not expose an active/resume status;
this does not prevent carrying out the explicit new user instruction.

VM provisioning progress: Tart 2.32.1 installed privately under
/Volumes/vm/ca-harness-tools; official archive SHA and Developer ID signature
verified. Official Homebrew formula failed on obsolete dependency syntax, so
no formula was edited. Separate TART_HOME /Volumes/vm/ca-harness-tart contains
ca-harness-base cloned from ghcr.io/cirruslabs/macos-tahoe-base at OCI digest
sha256:1b093499716409d29e8b5336844528e1cae375db97d2ad8e5aeff78cf0da201e.
Image download session 96775 completed. Guest macOS matches host 26.6.2/25G83.
VM configured 4 CPUs, 12288 MB, 192 GB disk; guest agent grew APFS to use it.
Toolchain 1.97.1 and its dynamic dependencies were copied from installed host
package artifacts, not user configuration. Cargo vendor completed using the
locked manifest; missing cross-platform crates required a registry download.

Guest ca-runner uid 550 is non-admin and cannot sudo. The public base-image
admin password was replaced without retaining its value; SSH/screen sharing
and GUI autologin disabled. Guest root-owned packet filter exempts loopback and
blocks other traffic. Clipboard/audio disabled; bootstrap share was read-only.
The guest probe passed localhost sockets, nested sandboxing, offline Rust/serde
compile/test, denied sudo, blocked network probes and an actual rejected write
to the host share (/tmp/ca-hrs253-vm-probe2.log). Mode-bit-only write checks were
insufficient on the read-only share; the original probe failed before replacement
with an actual write attempt. These are environment checks, not ca suite results.

Cold boot currently needs repair: the added admin-user RPC daemon did not
reconnect without GUI login. Guest filesystem was inspected only after stopping
the owned VM; its isolated disk was attached, inspected and detached. No RPC log
was created in /var/log. Its log path was moved to /tmp and HOME set explicitly;
second cold boot session 32214 is running. All previous VM boot/exec handles
are terminal. Cold-boot success remains unproven. No candidates executed yet.

Added ignored hrs253_export_current_source_for_vm test in harness_engineering.rs.
It uses the production bounded snapshot copier and emits an input receipt only;
the export test passed (session 86424). Current archive/source receipt lives in
Codex task work/hrs253-vm-bootstrap/current-source.json. This is not a gate pass.
Read-only virtual shares cached edits to bootstrap scripts during setup; use
unique immutable paths for published inputs. No user runtime config/install or
original Git branch mutation occurred.

VM cold-boot issue fixed: RPC daemon now writes to /tmp with explicit HOME;
headless cold boot and /opt/ca-harness/probe.py passed with no shared directories
(`/tmp/ca-hrs253-vm-cold-probe2.log`), and root verified PF enabled with the exact
block-all rule. Base stopped and cloned to ca-harness-validation-20260923.

First whole-workspace attempt stopped before compilation because some vendored
crate source files had mode 0640 after ownership hardening. Granted read access
without granting write access; fixed the base as well as the validation clone.
The first source tar also carried AppleDouble ._* metadata; Git correctly rejected
the dirty transferred tree. Repacked via Python tarfile (no host xattrs), retained
the failed guest tree separately, verified the new archive SHA, and now the clean
source precondition passes. No source-content or test expectations changed.

Current VM workspace run: driver session 1923, VM run session 39178;
`/tmp/ca-hrs253-vm-workspace2.log`, driver5.log. It runs the fixed test_default
command offline as ca-runner, with two build jobs and one test thread. It has an
1800-second deadline and stops the whole owned VM in finally. This is an environment
probe, not a production promotion receipt. An independently seeded detached child
(pid 3307, separate session 3307, parent session 304) writes a bounded heartbeat;
its nonce/marker receipt is work/hrs253-vm-bootstrap/detached-probe.json. Verify
shutdown/reboot removes it before claiming escaped-descendant coverage.

The stopped base was separately repaired through its own disk image and detached
before reboot. New working directory /var/empty avoids inherited private-admin
cwd errors for the restricted runner. Final base boot probe is next. Host Clippy,
format and diff checks for the ignored export helper pass (session 22961 terminal;
/tmp/ca-hrs253-vm-export-{clippy,format,diff}.log). No production evaluator/backend
has yet been changed to Tart; VM setup alone is not full integration.


### VM validation update — 2026-09-23

Cold-boot repair and template provisioning are complete. The final base probe
passed restricted-account checks, no host shares, Rust/ripgrep/system-Python
versions and PF status/rules, then stopped the base. Receipt:
Codex task work/hrs253-vm-bootstrap/base-ready.json.

The first full VM run completed in 595 seconds with 3,266 passes, 25 failures,
and 27 ignored tests. Thirteen additional failures were missing ripgrep and two
were Homebrew Python being denied by the sandbox. Installed root-owned ripgrep/PCRE2
from the host package artifacts into base and validation clone, then changed
only guest PATH order to prefer system Python. The corrected run records
3,281 passes, 10 failures, 27 ignored; all 2,608 ca unit tests pass. Nine failures
match the recorded host integration failures. The additional MCP asynchronous
step fixture emits its result immediately without consuming the launch barrier;
it races the controller's write and fails with Broken pipe. No production gate
or test expectation was relaxed. Logs: /tmp/ca-hrs253-vm-workspace{2,3}.log;
receipts: work/hrs253-vm-bootstrap/workspace-result{2,3}.json.

Detached-process cleanup passed: stop returned zero, Tart listed the VM stopped,
and after reboot the unique child command was absent and its heartbeat unchanged.
Receipt: work/hrs253-vm-bootstrap/detached-probe-verified.json. Initial inspection
falsely matched its own query text; the corrected predicate matches only the
child's final separate nonce argument. All earlier VM handles are terminal.

All-feature protected-source probe is running: VM handle 72856, driver 82701,
/tmp/ca-hrs253-vm-workspace-all.log, /tmp/ca-hrs253-vm-all-driver.log. The source
was moved beneath root-owned /opt/ca-harness/project, with 2,948 input entries
read-only to ca-runner; only target and the separate test home are writable.
The source parent is protected too, preventing rename-and-replace of the frozen
root. A narrowly scoped Git safe.directory option permits its status check.
The driver stops the whole VM on finish/error/1800-second timeout. This remains
an environment probe, not production promotion evidence. Tart has not yet been
wired into harness_engineering or harness_execution; config, installed binary,
original Git branch and real-provider execution remain unchanged.


### VM environment validation settled — 2026-09-23

The prepared base and validation VM are both stopped. No live VM/test/build
handle remains. Current evidence is in the Codex task's
work/hrs253-vm-bootstrap directory; bootstrap.json records the prepared state
and explicitly says production_backend_wired=false.

Corrected default workspace run: 3,281 passed, 10 failed, 27 ignored. All 2,608
ca unit tests passed. First all-feature protected-source run: 3,413 passed,
27 failed, 28 ignored. That run exposed two setup mistakes: removing owner write
bits made copied fixture workspaces read-only, and moving the source while
reusing build output left embedded CARGO_MANIFEST_DIR paths pointing to its old
location. Kept that output under /Users/ca-runner/target-before-relocation and
rebuilt from fresh output at the final /opt/ca-harness/project path. Input files
are root-owned with normal 0644/0755 modes; the non-admin runner cannot open them
for writes, chmod them or replace their root-owned parent. Private copies remain
writable to their new owner. Build output is owned by ca-runner.

Corrected all-feature workspace run: 3,429 passed, 11 failed, 28 ignored, in
521.56 seconds (two build jobs, four test threads). All 120 experimental-workflow
tests now pass. Failures are the nine previously recorded integration failures,
the asynchronous-step launch-barrier fixture, and
voice::tests::cancelling_local_transcription_kills_descendants under parallel
load. The voice test passes three later isolated runs without code changes;
that does not erase its broad failure. Earlier serialized all-feature ca suite
passed all 2,637 tests. Logs: /tmp/ca-hrs253-vm-workspace-all{,-corrected}.log and
/tmp/ca-hrs253-vm-voice-focused.log. These are environment probes; the manual
all-feature command did not include the fixed matrix's --all-targets option.
No full engineering admission or source-efficacy result is claimed.

A direct guest-local AF_VSOCK probe reached the administrative guest-agent port.
Prepared a private host-only agent from upstream commit
26268c6d77d1c1059cd4a0fb88ae170c64cd2d9a: reject connections unless the kernel's
peer CID is VMADDR_CID_HOST. The upstream reference clone remains unchanged.
The changed agent's vsock and RPC tests pass, its binary is ad-hoc signed and
verified, and the original license is retained. Direct Go dependency fetching
timed out; fetched pinned module artifacts over official-proxy HTTPS with curl,
then built offline with Go checking the existing go.sum hashes. The first
installation did not survive immediate VM shutdown; explicitly fsynced the
installed files and ran guest sync before stopping. The next cold boot verified
the exact binary and daemon path. Keep that durability step in provisioning.

Both VM images now reject guest-local RPC before returning application data.
Host RPC still works after rejection. The non-admin guest also cannot bind the
host CID (errno 49). PF remains enabled with the block-all/non-loopback policy;
no host directories are shared. This closes the observed administrative control
path for these probes, not a blanket claim about all VM escape possibilities.
Verified receipt: work/hrs253-vm-bootstrap/host-only-agent-verified.json;
validation repeat: final-validation-probe.json. Binary SHA-256:
ab2159ca9ac4bd5ab3f49b0c0715561c8688549fde6e7cdf8418b23d1f25a3cd.
Managed binary, source patch, test, license and build/verification receipts:
/Volumes/vm/ca-harness-tools/guest-agent-host-only-ab2159ca9ac4.

Production harness_engineering/harness_execution still use their existing
backend. Next HRS-253 work is to freeze the VM environment in plans, route every
candidate execution and verification boundary through owned disposable VMs,
and validate bounded artifact transfer/cancellation and the complete source/idle
lifecycle. Do not turn the environment checks into promotion receipts. No user
provider settings, installed ca binary, original Git branch or real-provider
calls were changed. The overall plan remains ACTIVE and incomplete.


The owned VM controller is now implemented in src/harness_vm/lease.rs and its
embedded supervisor. Seven focused checks pass (two opt-in probes skipped),
including nine Python lifecycle cases and Rust finish/Drop/drift coverage.
Wrong guest-agent admission failed before the startup identity check and now
passes. Strict all-feature workspace Clippy, format and diff checks pass. The
actual Rust clone/boot/agent-attestation/stop/delete probe passed in 10.81 seconds;
/tmp/ca-hrs253-vm-rust-lease-real.log. Candidate paths still reject VM plans until
bounded transfer/import and complete execution routing are wired. See the active
plan's Owned VM controller entry for exact receipts and remaining work.
