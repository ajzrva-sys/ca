# Testing and Validation

## Current completion gate and native platforms (2026-10-23)

Run the unchanged completion entry point from the repository root:

```sh
cargo fetch --locked
./scripts/verify.sh --output scratch/verify/my-change
```

The fetch prepares the dependency cache; the gate itself uses locked offline
Rust commands and disposable application state. Use Python 3, the pinned Rust
toolchain with rustfmt/Clippy, Git, loopback sockets, PTYs and subprocess support.
Native FreeBSD required-isolation checks additionally need the configured shared
jail service; see [FreeBSD setup](docs/FREEBSD.md).

Read `report.json`, `evaluation.json` and actual failing logs. A timeout,
unavailable prerequisite or incomplete report is not a pass. Required stages
include verifier contracts, formatting, strict lint, full default and all-feature
workspace tests, release build, native CLI/terminal fixtures and independent
evaluation. No historical VAL-288 count is an acceptance baseline for new source.
See [the verification workflow](docs/VERIFICATION.md).

For verifier-infrastructure changes, also retain the intentional-defect receipt:

```sh
./scripts/verify.sh --self-test
```

Current native/publication inventory: `c4ca6055a1f8967fef89a0c676fb19dbd5172143d89bf16fb6ead840f492d155`
(1,169 files). Combined macOS gate inventory: `69d36efaca22361e14d0281afe7b4f47e483a79dd93c51fc11917e289c01a3d1`
(1,168 files). The public notice generator and FreeBSD-only `cfg(test)` MCP
helper account for the differences. The release runtime prefix is byte-identical;
artifacts retain distinct source identities, including source in build identity.

The final native 22-case MCP module, 62-case filter run and formatting pass.
Native v8 passes all eight stages/evaluator, with source unchanged before/after:
default 4,297 passed, zero failed, 43 ignored over 29 targets; all features
4,451 passed, zero failed, 44 ignored over 30; zero filtered. Release/runtime
stages pass. The copied/stripped native candidate passes metadata/`$HOME`, real automatic
verification for plain/internal-document-symlink source, and native jail fixtures.
Native ca `0.1.41` also passes a real installed required-jail provider/tool loop
(printf exit0, final `FREEBSD_OK`), distinct from fixture proof. Its earlier
symlinked-auth setup failure is retained. It is installed once with recovery
and ordinary-user PATH, hash,
mode/ownership and version/help readbacks; installed SHA-256
`60563263dde655bcb16fa949189c9c09eda425b43dc324a56a19cce3de0aa074`. Preceding
native v7 did not pass: two MCP environment fixtures encountered `sysctl`
`ENOMEM` before cleanup assertions, followed by controlled interruption and
`infrastructure_failed`. Bounded diagnostics did not reproduce those failures. Selecting
native `/bin/sh` for POSIX test fixtures preserves assertions/runtime behavior
and is not a proven Bash/kernel causal repair. The final unchanged full gate passes; the original failure and its causal
uncertainty remain retained.

The final combined macOS full gate passes all eight stages and evaluator:
4,328 default tests passed, zero failed, 43 ignored across 29 targets; 4,484
all-feature tests passed, zero failed, 44 ignored across 30; zero filtered.
All nine native terminal cases pass. Signed ca `0.1.41` was installed exactly
once, with recovery/hash/signature/mode/ownership/version/help readbacks and a
successful live DeepSeek no-session prompt through the normal PATH entry point.

Codex's immutable release and installed entry point pass ten native workflows,
47 jail checks, 174 code-mode units and installed direct/Sol-default jail paths.
A live installed supported-model loop also executes a real sandboxed printf
command and returns `FREEBSD_OK`; an earlier unsupported-model HTTP 400 is
retained. This is bounded live proof, not a coding benchmark. Its broader scoped
core still records 1,263 passed, 17 failed and 11 ignored.
These are distinct from a clean complete Codex suite.

Current evidence, prior macOS terminal failure and installation readbacks belong
in [publication validation](docs/PUBLICATION_VALIDATION.md). Loopback fixtures,
installed readbacks and live provider requests are distinct proof levels.
The coding benchmark was canceled before scored native attempts; usage-export
qualifiers remain fixture proof only. Everything below is historical.

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


## 2026-10-06 BUG-262/BUG-263 compaction and unavailable-tool correction

The previous installed signed artifact reproduces the incomplete-compaction
footer in both Chat Completions and Responses, and the unavailable run_command
footer after bounded Ask-mode correction. Normal Agent mode already advertises
and executes the harmless command. Failure-first prompt, exact-cause decoder and
catalog-feedback regressions fail before repair. Final context/decoder/correction,
authority and narrow-render tests pass; before-images and failing logs remain in
`scratch/compaction/` and `scratch/unavailable-tool/`.

Full default workspace: 26 targets, 3,809 passed, zero failed, 43 existing ignored,
zero filtered. The default gate succeeded within session 53174; that job later
exited 101 at the first all-feature gate. Full all-feature workspace: 27 targets,
3,958 passed, zero failed, 44 existing ignored, zero filtered. The final complete
all-feature rerun uses one test thread (session 26568 exit 0). Child writer-probe
summaries are excluded from aggregate counts. Formatting, strict all-target/
all-feature workspace Clippy and the optimized all-feature workspace release
build pass in that session. Parallel release build session 73704 also exits 0.
`scratch/compaction/validation-gates.json` binds each command/log to the frozen
source; `default-workspace-summary.json`, `all-feature-workspace-summary.json`
and `source-changes.patch` are retained beside it.

The initial four-thread all-feature run failed only
`session::tests::failed_over_bound_legacy_migration_preserves_original_bytes`,
with WouldBlock instead of InvalidInput. A cold 25-ms identity-scan deadline is
an inferred cause from the unchanged source; the failure log did not include the
error string. The fixture passes focused and the complete single-thread workspace
rerun passes without exclusions or session-source changes. The initial failure
log/summary are retained as `all-feature-workspace-first-failed.log` and
`all-feature-workspace-first-failed-summary.json`; `migration-focused-rerun.log`
records the separate focused pass.

Six signed release terminal cases pass against SHA-256
`f20a97ab23bdec8e01c5f656bacd872cec5488181e73afc78c4d545831971fa0`:
two compaction protocol cases and four unavailable-tool/mode cases. Sessions
53296/22260 exit 0. Compaction checks cover fresh output-limit recovery, shared
quality-repair budget, exhaustion preserving the checkpoint, non-retry for
filter/refusal/unknown/EOF/HTTP errors and Escape cancellation without publication.
Unavailable-tool checks cover compliant correction, repeated bounded failure,
exact catalogs, forbidden-command exclusion, frozen Ask retry, new Agent command
execution and the actual Plan→Ask→Debug→Agent transition. All cases restore the
terminal; 60-column Ask/Plan recovery guidance remains visible. Loopback providers
and disposable homes/sessions/workspaces establish these paths without a live
provider call or production settings/session changes.

Complete compiled-input inventory: 1,073 files, SHA-256
`75c9905f9ab0f2b3942f8ea2342bf6f51c6b4df4327bf9f6f163474b9167fe69`.
The initial 1,067-file inventory omitted six existing inputs. All six match the
prior BUG-261 validated hashes, all original rows remain unchanged, and actual
build roots/protected source are covered. `input-inventory-completion.json`
records the correction; the initial manifest/candidate metadata remain preserved.
Only inventory metadata changed; source and executable bytes remain identical.

Relevant commands, run from `/Volumes/projects/ca/ca`:

```sh
cargo test --locked --offline -p ca --bin ca context::tests -- --test-threads=4
cargo test --locked --offline -p ca --bin ca agent::provider::tests::responses_decoder -- --test-threads=4
cargo test --locked --offline -p ca --bin ca unavailable_tool -- --test-threads=4
cargo test --locked --offline --workspace --no-fail-fast -- --test-threads=4
cargo test --locked --offline --workspace --all-features --no-fail-fast -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings
cargo build --locked --offline --release --workspace --all-features
python3 tools/compaction_pty.py --binary scratch/compaction/release/ca --receipt scratch/compaction/release-pty.json
python3 tools/unavailable_tool_pty.py --binary scratch/compaction/release/ca --receipt scratch/unavailable-tool/release-pty.json
bash scratch/compaction/install-validated.sh
python3 scratch/compaction/verify-installed.py
```

Background session 1017 completed the post-validation build and atomic install
exit 0. Candidate/staged/installed hashes match; the installed destination is
`/Users/aji/.local/bin/ca`. Independent PATH/hash/owner/mode/signature/version/help
and SHA-labelled rollback-copy checks pass in
`scratch/compaction/installed-readback.json`. `install.log` and
`post-validation-build.log` retain the background evidence. Restart ca to load
the fixes; running user sessions and settings were not changed.
Completed plans: `docs/exec-plans/completed/BUG-262-compaction-output-budget.md`
and `docs/exec-plans/completed/BUG-263-unavailable-tool-correction.md`.

## 2026-10-06 BUG-261 recovery save ownership

Previous installed signed artifact naturally reproduces the exact failure footer
in a disposable persistent TUI. Failure-first held-writer and duplicated-descriptor
regressions also fail before the repair. Final six BUG-261 tests, frozen-request
HTTP fixture, bounded diagnostic and wide/narrow render tests pass. Full default
workspace: 26 targets, 3,796 passed, zero failed, 43 existing ignored, zero filtered.
Full all-feature workspace: 27 targets, 3,945 passed, zero failed, 44 existing
ignored, zero filtered. Both complete runs finish in session 90557 exit 0.
Child writer-probe summaries are excluded
from workspace aggregate counts. Format, strict all-target/all-feature workspace
Clippy and optimized all-feature workspace release pass.

Eight signed release real-terminal cases pass against SHA `05d2f106c62c65a27a922de30ae45ee3af44a78bd43a4cbdcd40cbfece6c1751`:
wide/narrow recovery retains the durable source partial and completes a separate
branch using identical frozen HTTP bytes exactly once; genuine directory write
denial proves no branch/replacement and useful permission error in `/errors`;
five existing Enter scenarios still pass (63–70ms interrupt). All terminal exits
are 0/restored. Fake loopback providers and disposable homes/sessions/workspaces
are used; no live provider/autobrr operation or user settings/session changes.

Final compiled inputs: 1,073 files, SHA-256
`415e996d5cc9c3c829e173ed91490616f9e28fe2afdeae2d0a450e45aa79581c`.
The initial complete default run failed only a fixture receive-order assumption:
the adapted completion path now sends normal retirement before branch readiness.
The fixture now processes retirement/owned metadata save as production does and
passes focused and complete main reruns. Its failed log/aggregate are retained,
with no package/target/test exclusions. The current main/default workspace passes
the retention/archive checks previously left failing in BUG-260.

Commands and evidence under `scratch/recovery-branch/`:

```sh
cargo test --locked --offline -p ca --bin ca bug261 -- --test-threads=4
cargo test --locked --offline -p ca --bin ca bug257_replacement_preserves_source_and_frozen_request_once -- --test-threads=1
cargo test --locked --offline --workspace --no-fail-fast -- --test-threads=4
cargo test --locked --offline --workspace --all-features --no-fail-fast -- --test-threads=4
cargo fmt --all -- --check
cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings
cargo build --locked --offline --release --workspace --all-features
python3 tools/recovery_branch_pty.py --binary scratch/recovery-branch/release/ca --receipt scratch/recovery-branch/release-pty.json
python3 tools/enter_interrupt_pty.py --binary scratch/recovery-branch/release/ca --receipt scratch/recovery-branch/enter-release-pty.json
```

Release build session 55090 and recoverable install session 28574 completed;
`release-build-parallel.log`, `install.log`, `release-candidate.json`, both terminal
receipts and `installed-readback.json` bind validation to the installed bytes.
All test runs requiring loopback/process access used full-access disposable
fixtures; no sandbox bind errors were mistaken for product failures.

## 2026-10-06 BUG-260 leading-path Enter repair

Installed-artifact failure and Ctrl+S workaround were reproduced in disposable
actual terminal sessions. Final input suite: 127 passed, zero failed. Busy
control render snapshots, formatting, strict all-target/all-feature workspace
Clippy and all-feature workspace release build pass. Five signed release
terminal cases pass: exact screenshot draft at two widths, explicit queue then
interrupt, actual empty path menu then Enter, and idle absolute-path submission.
Interruption takes 69–92 ms; exact message arrives once, foreground PID exits,
heartbeat stops, completion marker stays absent, and terminals exit 0/restored.

Commands and logs under `scratch/enter-interrupt/`:

```sh
cargo test --locked --offline -p ca --bin ca tui::input_tests -- --test-threads=4
cargo test --locked --offline -p ca --bin ca tui::render_tests::busy_interrupt_controls_survive_status_messages -- --test-threads=1
cargo test --locked --offline --workspace --no-fail-fast -- --test-threads=4
cargo test --locked --offline -p ca --bin ca -- --test-threads=4
cargo fmt --all -- --check
cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings
cargo build --locked --offline --release --workspace --all-features
python3 tools/enter_interrupt_pty.py --binary scratch/enter-interrupt/release/ca --receipt scratch/enter-interrupt/release-pty.json
```

Workspace limit: restricted run fails 11 targets due to denied loopback/process
operations (loopback bind independently raises EPERM). The full-access default
workspace run fails only the main target: 2,941 passed, four failed, 42 ignored;
every other target passes. The two Himalaya timeout and two session writer-lease
fixtures all pass focused reruns. The later complete main-target rerun passes
2,944 with one recurring retention writer-lease failure, 42 ignored. Relevant
modules/test bytes match AUT-259's prior validated compiled-input manifest.
The recurring failure remains unresolved; full workspace is not green.
No exclusions or unrelated fixes were introduced. Failed logs are preserved.

Signed candidate/installed SHA:
`e8a58abcabed6cc9f5018031cfdafb6fcb9e634200e55a3c4167e9089ef75d65`.
Build session 44472, install session 30464; logs `release-build.log` and
`install.log`. Installed readback verifies PATH, identical candidate/installed
hashes, signature, owner/mode, version/help and prior SHA-labelled backup.
`release-pty.json` binds the actual terminal checks to those same bytes.
No running user session or runtime setting was changed.

## 2026-10-06 AUT-259 final unfiltered evidence

The complete default and all-feature workspace suites pass at compiled-input
SHA-256 `130cc08ec3d36ff2c8dd4874a5368dc5f5d28298419267693dc87a7febc8fdfa`
(1,073 captured inputs). No package, target or test-name exclusions are used.
Default: 26 targets, 3,783 passed, zero failed, 43 existing ignored, zero
filtered. All features: 27 targets, 3,932 passed, zero failed, 44 existing
ignored, zero filtered. The ignored tests are retained in each target summary;
they are not counted as passes. Both strict Clippy configurations, formatting
and the all-feature release workspace build pass at the same input digest.

```sh
cargo test --locked --offline --workspace --no-fail-fast -- --test-threads=4
cargo test --locked --offline --workspace --all-features --no-fail-fast -- --test-threads=4
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo build --locked --offline --release --workspace --all-features
```

Durable gate JSON, raw logs and per-target matrix summaries are in
`scratch/automation-absorption/final-validation/`. Each gate records its exact
command, actual completion, log hash and unchanged compiled-input binding.
Earlier failing diagnostic matrices are preserved in dated subdirectories;
they are superseded by these unfiltered final runs, not converted into passes.
The real CLI/MCP targets run in both matrices, including eight stdio MCP cases.

Actual signed release terminal checks pass: a workflow and ordered mixed Git
query/required-isolation Cargo verification execute without provider requests;
32 ordered keystrokes per run have p95 19.572 and 20.568 ms. Automatic frozen
continuation recovery, explicit `/retry` and an ordinary native automation pass.
Completed writes retain identical bytes, inode and modification time. Both
terminal fixtures exit successfully and restore terminal state. Killing a
compatibility observer retains the owned worker/resource claim; reconciliation
returns its complete receipt and duplicate dispatch repeats no effect.

Matching signed release receipts are `pty-release-mixed.json`,
`pty-continuation-release.json` and `compatibility-parent-crash-release.json`
under the same evidence root. Debug equivalents also pass. These bounded
automation/continuation checks do not establish an eight-hour soak or the
separate real-model recall/latency gates from earlier work.

The signed ca and matching codex-team CLI were installed recoverably through
managed session 9381. `paired-install.log`, `install-receipt.json` and
`installed-readback.json` retain atomic replacement, destination signing and
independent PATH/hash/signature/mode/owner/version/help/backup verification.
Seven focused installer recovery checks pass in `installer-regressions.log`;
the pre-replacement filename/help refusal is preserved as a diagnostic log.
No existing session or runtime profile was changed.

The historical sections below describe their dated artifacts and delivery
boundaries. Their workspace-failure and installation statements do not describe
the current AUT-259 candidate; preserve them as prior evidence.

## 2026-10-06 01:34 UTC ORC-258 final integrated evidence

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

Exact build evidence: `/Volumes/projects/ca/ca/scratch/oracle-improvements/final-validation/build-20261006T013145Z`; default and all-feature summary JSON retain
per-target counts, explicit exclusions and raw logs. Signed release evidence:
`/Volumes/projects/ca/ca/scratch/oracle-improvements/final-validation/release-20261006T013149Z`; actual TUI, short PTY, render/signature and installation-readiness
receipts bind the preserved candidate SHA. Failed/superseded runs remain in
`final-validation/audit-repairs/` and older release directories.

Focused failure-first audit positives: seven automation/runtime, 28 memory/
benchmark, 91 ownership/session/AUT and 32 child-manager cases. The default
matrix's timeout fixture failure was independently reproduced three times, then
repaired in test-only code: observe registered JobId, allow two-second runtime,
kill/reap owned jobs before assertions, five-second overall fixture bound.
Production timeout and job cleanup code is byte-identical to its pre-repair state.
The repaired case passes focused and both full matrices; no extra exclusion.
Four additional focused input/lifecycle and PID-publication fixtures also pass.
The PID fixture waits for a complete valid published PID; its production process
timeouts and cleanup behavior are unchanged.

Identity inventory fixtures cover bytes/candidates/deadlines, restored-mtime
rewrites, replacements, exact target validity, FIFO refusal, turn worker warming
and recorded-result AUT delivery without replay. Off-loop cold warming and
cooperative short proof limits do not establish whole-save wall-clock bounds.
The eighth actual TUI case uses a cold >8 MiB unrelated legacy inventory,
observes exact save refusal followed by successful worker-backed turn completion,
and measures post-warm p95. It does not separately measure in-flight warm-key p95.

Native read measurement: four independent 8,704,000-byte files, seven warmed
paired runs, exact output equality. Serial p50/p95 522.376/526.590 ms, parallel
498.618/500.584 ms (1.048x median) on that debug workload. This is not a general
application speed claim. Lazy schema Main 30,938→7,150 bytes (76.89%) and broad
Child 21,013→7,150 (65.97%), with pinned contract fixtures also >=30%.
Synthetic lexical/hybrid fixture metrics and timing remain separate from real
embedding quality. The CLI's successful exit never substitutes for acceptance.

Fresh-path continuation procedures (do not launch a duplicate soak):

Fresh signed CLI recall evidence is in
`scratch/oracle-improvements/final-validation/release-20261006T013149Z/memory-offline-benchmark/receipt.json`.
Three disposable offline calls and 273 independent metric/timing/context checks
pass. Lexical recall@5 is 0.666667, MRR 0.666667 and false-positive rate zero;
synthetic threshold 0.7 yields recall@5 and MRR 1.0, identifier nonregression
and zero false positives. Threshold 0.1 correctly fails negative calibration
with false-positive rate 1.0 despite CLI exit zero. Nine-query offline ranking
p95 is 0.048750/0.053708/0.056250 ms respectively. These timings measure only
fixture selection/render and do not establish real embedding or warm acceptance.

```sh
release_dir=$(cat scratch/oracle-improvements/final-validation/LATEST_RELEASE)
cat "$release_dir/eight-hour-soak.json"
CA_INSTALL_CANDIDATE="$release_dir/ca" bash tools/bug257_install.sh --check
```

`tools/orc258_manifest.py` verifies the oracle inventory and all bytes and produces
full source snapshots and content patches without Git. Milestone patches require
the shared integration partition; they are review partitions of one candidate.
Install only after the actual duration and selected real-model gates pass.

## 2026-10-04 BUG-257 long-session responsiveness and failure recovery

All commands below ran in the root application checkout with disposable
provider/session fixtures, not real runtime configuration. No production
provider request was used.

- PASS: `cargo test --locked --offline --bin ca -- tui:: turn_recovery:: terminal_awareness:: runtime_diagnostics:: idle_learning:: bug257_ pre_output_failure_footers partial_stream_failure --skip bug257_replacement_preserves_source_and_frozen_request_once --test-threads=1`:
  **566 passed, 0 failed, 1 ignored**, 2,151 filtered. Scheduling fairness,
  Unicode/style layout parity, cache eviction/worker generations, safety
  eligibility, frozen requests, persisted branch boundaries, cancellation,
  pricing latches and failure categories are covered. Log:
  `/private/tmp/ca-bug257-focused-final.log`.
- PASS: `cargo fmt --check` (job 96250,
  `/private/tmp/ca-bug257-fmt-final.log`),
  `cargo clippy --locked --offline --all-targets -- -D warnings` (job 28403,
  `/private/tmp/ca-bug257-clippy-verified.log`),
  `git diff --check` (`/private/tmp/ca-bug257-diff-check.log`).
- PASS: background `cargo build --locked --offline --release --workspace --all-features`
  (job 98631, `/private/tmp/ca-bug257-release.log`). Candidate ad-hoc signed and
  `codesign --verify --strict --verbose=3`, SHA/mode/owner/version/help verified.
- PASS: `python3 tools/bug257_pty.py --binary target/release/ca --soak-seconds 0`:
  4,096 messages, 32 samples per condition; initial p95 19.497 ms, post-idle
  19.565 ms (zero-hour smoke, not duration proof), narrow 19.676 ms. Ordered
  paste, safe `/theme sync`, resize and alternate-screen restoration pass,
  exit 0. Log `/private/tmp/ca-bug257-release-pty.log`; receipt
  `/private/tmp/ca-bug257-pty-bmki0xit/result.json`.
- PASS: optimized standalone `tools/bug257_render_probe.rs`: repeated prefix
  wrapping vs indexed layout at 512/1,024/2,048/4,096 source lines:
  20.09/66.57/264.91/1,077.73 ms vs 1.46/2.60/5.20/10.78 ms, identical viewport
  buffers. Structural probe, not a whole-application speedup claim. Log
  `/private/tmp/ca-bug257-render-probe.log`. Compile against the built release
  ratatui rlib using `rustc --edition=2021 -O`, `--extern ratatui=<rlib>` and
  `-L dependency=target/release/deps`.
- RUNNING: `python3 tools/bug257_pty.py --binary target/debug/ca --soak-seconds 28800`
  (job 89271, `/private/tmp/ca-bug257-pty-soak.log`). Actual untouched idle
  began 2026-10-04 21:22:54 UTC; earliest finish 2026-10-05 05:22:54 UTC.
  Initial p95 20.777 ms; `/private/tmp/ca-bug257-pty-uds9g6kg/result.json`.
  Debug input/render code is the same as the final release; later changes
  were recovery guards/tests. No eight-hour pass or RSS bound claimed yet.
- BLOCKED: `bug257_replacement_preserves_source_and_frozen_request_once`
  loopback HTTP test cannot bind in this sandbox (EPERM). It remains enabled
  for normal environments; the network-free branch/eligibility tests pass.
- NOT GREEN: `cargo test --locked --offline --workspace --no-fail-fast`
  (job 74837, `/private/tmp/ca-bug257-workspace-tests-final.log`): 12 failed
  targets. Main binary **2,521 passed, 157 failed, 40 ignored**; codex-team
  **560 passed, 16 failed, 1 ignored**; protocol **14 passed, 1 failed**;
  other target totals are retained in the log. Failures include EPERM on TCP
  listeners and nested sandbox-exec, Git commit creation denial, unavailable
  process probes, integration paths pointing at an absent ancestor target
  binary, missing checked-in schema fixtures and separate LSP/MCP assertions.
  These were not all independently established as baseline failures; do not
  label the entire set environment-only or weaken tests to make it green.
- INSTALL BLOCKED: destination-directory `mktemp` denied at
  `/Users/aji/.local/bin`. No installed file was replaced. Read-only installer
  preflight is available via `bash tools/bug257_install.sh --check`; normal
  terminal invocation without that flag performs recoverable installation of
  the pinned, signed candidate. See CONTINUITY.md for both hashes.
- PASS: installer syntax (`bash -n tools/bug257_install.sh`) and read-only
  `bash tools/bug257_install.sh --check` verify both signed artifacts, pinned
  hashes, candidate owner/mode/version/help and exact current executable path.
  No write or replacement occurs in this preflight; install/rollback execution
  remains untested because destination writes are denied.

The debug/test linker reported an existing large `__eh_frame` compact-unwind
warning; the optimized release completed without that warning.

## 2026-09-30 TUI-256 Grok-style usage-limit graph

- Focused: `cargo test --locked --bin ca -- usage_limit_graph meter_fills limit_tab_always usage_command_opens agent_dashboard_snapshots -- --test-threads=1` PASS.
- `cargo test --locked --bin ca --no-fail-fast`: **2,661 passed, 0 failed, 40 ignored**.
- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`: PASS.
- Reviewed wide/narrow snapshot `agent_dashboard_usage_limit` against the Grok weekly-limit bar screenshot.
- Release install: `/Users/aji/.local/bin/ca` post-codesign SHA-256
  `46d60ee15c7a4893c357e75eccd19b8f05eaaeacc1fc52c25cf122e913c07b41`;
  recovery `/Users/aji/.local/bin/ca.recovery-TUI256-20260930T155545Z-864eae83`.

## 2026-09-30 TUI-255 Grok-style agent dashboard

- Focused: `cargo test --locked --bin ca -- dashboard usage_command_opens dashboard_command service_cli_reserves --test-threads=1` PASS.
- `cargo test --locked --bin ca --no-fail-fast`: **2,653 passed, 0 failed, 40 ignored**.
- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`: PASS.
- Reviewed wide/narrow snapshots `agent_dashboard_{empty,roster,usage_limit}` against the attached Grok screenshot.

## 2026-09-05 PLN-244 Plan Card Review Lifetime

- Failure-first test reproduced a persistent card after approval:
  `/tmp/ca-pln244-before.log`.
- `cargo test --locked --bin ca plan_ -- --test-threads=1`: PASS, 40 tests.
  Approval/dismissal/revision/rejection remove the card; active turns and
  restored artifacts do not display it; the explicit reader retains the plan.
  Expanded task coverage remains. Log: `/tmp/ca-pln244-focused.log`.
- Reviewed paired 100x30/60x24 snapshots for hidden active-turn cards and the
  expanded review card (renamed from the prior always-visible approved fixture).
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `git diff --check`: PASS; `/tmp/ca-pln244-{fmt,clippy,diff-check}.log`.
- `cargo test --locked --no-fail-fast`: 3,232 passed, one failed, six ignored.
  All CA/render tests and MCP SDK checks passed. The unrelated codex-team
  `pm_commit_request_commits_selected_allowed_files_only` fixture reported
  commandFailed despite command exit zero. It passed unchanged in isolation
  with `cargo test --locked -p codex-team --lib
  app::app_tests::pm_commit_request_commits_selected_allowed_files_only -- --exact`.
  Preserve broad failure `/tmp/ca-pln244-tests.log` and isolated pass
  `/tmp/ca-pln244-isolated.log`; no test expectation or timeout was changed.
- The first release PTY found reader Esc restored the card. A keyboard-driven
  regression reproduced this (`/tmp/ca-pln244-reader-before.log`); reader Esc/q
  now calls `plan::dismiss`. All 40 focused tests pass again. Before-fix terminal
  log: `/tmp/ca-pln244-release-reader-before.log`; installation did not run.
- After the reader-close repair, `cargo test --locked -p ca --no-fail-fast`:
  PASS, 2,534 passed and five ignored; `/tmp/ca-pln244-ca-final-tests.log`.
  Final format, strict all-target Clippy and focused 40-test checks pass.
  Unchanged workspace package results above are retained.
- Final debug PTY: PASS at 100x30 and 60x24. Review displays all six tasks;
  Esc dismisses, `/plan show` reopens the reader, reader Esc clears it,
  approval and execution leave no card, and completion preserves that state.
  Three local fake-provider requests per fixture; Ctrl+C exits zero and restores
  the alternate screen. Log: `/tmp/ca-pln244-debug-pty.log`.
- Background all-feature workspace release/install job 32794: PASS;
  `/tmp/ca-pln244-release.log`. Release PTY checks pass at 100x30 and 60x24.
  Candidate/staged/installed hashes, strict signatures, executable mode/owner,
  version and help all match. Installed `/Users/aji/.local/bin/ca`, SHA-256
  `77a071bc3abb82dd9cd367b5c76e8aa4617e26a38df2826f3303e6091a76e5ad`.
  Recovery copy:
  `/Users/aji/.local/bin/.ca-recovery-20260906T005355Z-waec0o9e/ca.previous-9ed19d138e6f214722fe9209752a78bf507077f3a31341b426ae793a564180ac`.
  Independent installed verification also passed. Running sessions and user
  config/session files remain untouched. No oracle/reference edits; no new
  protected-tree verification claim. The known baseline discrepancy remains.

## 2026-09-05 PLN-243 Expanded Plan Default

- Failure-first regression reproduced the clipped long title and hidden tasks:
  `/tmp/ca-pln243-before.log`.
- `cargo test --locked --bin ca plan_ -- --test-threads=1`: PASS, 38 tests.
  Includes all six tasks, complete long titles/statuses, wide/narrow rendering,
  and scrolling back to the header. Log: `/tmp/ca-pln243-focused.log`.
- Reviewed three paired 100x30/60x24 snapshots: new six-task approved artifact,
  revised review card, and revised save-failure card. Full reader unchanged.
- `cargo test --locked --no-fail-fast`: PASS, 3,231 passed, zero failed, six
  ignored across 19 targets, including the full render suite and MCP SDK tests.
  Log: `/tmp/ca-pln243-tests.log`.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `git diff --check`: PASS. Logs: `/tmp/ca-pln243-{fmt,clippy,diff-check}.log`.
- Managed release/install job 85952: PASS; `/tmp/ca-pln243-release.log`.
  `cargo build --release --workspace --all-features` passed. Disposable-home
  local-provider PTY checks passed at 100x30 and 60x24: six tasks before and
  after `/plan approve`, complete wrapped first task, PageUp to the header,
  Ctrl+C exit code zero, and alternate-screen restoration. No Execute action
  ran; approval made no new provider request (two requests per fixture).
- Candidate/staged/installed hashes, strict signatures, executable mode/owner,
  version/help all passed. Installed `/Users/aji/.local/bin/ca`, SHA-256
  `9ed19d138e6f214722fe9209752a78bf507077f3a31341b426ae793a564180ac`.
  Recovery copy:
  `/Users/aji/.local/bin/.ca-recovery-20260906T002214Z-s4nfckkm/ca.previous-f6af39cd7c1eb0d1d6c7bafec884af414cd055e2f69207a0103c37f908283bb4`.
- Early PTY fixture attempts failed before installation: positional prompt
  selected headless mode; trust input raced startup; a phrase wrapped across
  rows; Ctrl+C exited before a second write. The fixture now launches the TUI,
  waits for trust/input readiness, compares wrapped text, and checks exit before
  sending again. Logs: `/tmp/ca-pln243-release-{first,trust,trust-timing,input-ready,wrapping,exit}.log`.
  These required no production changes or rerun of already-passed Rust checks.
- Existing debug linker unwind-section warning remains non-fatal. No live
  inference or oracle/reference edits; the separate known protected-tree
  baseline discrepancy is not repaired by this display change.

## 2026-09-05 BUG-242 Astra reasoning

- Failure-first status regression reproduced reasoning unavailable in
  `/tmp/ca-bug242-before.log`.
- Three Astra regressions pass: endpoint/profile boundaries, status and picker
  (100x30/60x24 deterministic renders), and exact Responses effort serialization.
  Log: `/tmp/ca-bug242-focused.log`.
- Request regression exposed x_high serialization; xhigh is now canonical and
  old x_high input is accepted. No test expectation was weakened.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `git diff --check`: PASS. Logs: `/tmp/ca-bug242-{fmt,clippy,diff-check}.log`.
- `cargo test --locked`: 2,475 CA tests passed, five ignored, one unrelated
  privilege fake-sudo failure (expected exit code 7). It repeated under
  `cargo test --locked --no-fail-fast`; the unchanged test passed in isolation
  (1/1), `/tmp/ca-bug242-privilege-isolated.log`. Preserve both broad failures.
- Full no-fail-fast totals: 3,228 passed, two failed, six ignored. The second
  failure was `codex-team`'s `real_runner_timeout_terminates_child_and_grandchild`
  waiting for a fixture PID. It also passed unchanged in isolation (1/1),
  `/tmp/ca-bug242-runner-isolated.log`. All other targets passed, including 8/8
  MCP SDK tests. The broad run remains a recorded failure.
Release/install job 44274 completed successfully. All-feature workspace release,
candidate/staged/installed SHA-256, strict signature, mode/ownership, version and
help checks passed. Installed `/Users/aji/.local/bin/ca`; SHA-256
`f6af39cd7c1eb0d1d6c7bafec884af414cd055e2f69207a0103c37f908283bb4`. Recovery:
`/Users/aji/.local/bin/.ca-recovery-20260905T222628Z-hd8mj6id/ca.previous-d9cbe4aac3947fb79eba4510ecafba0f53513f526045fba5c91ce41c6efa88e6`.
Log: `/tmp/ca-bug242-release.log`. Independent installed hash/signature/version
checks passed. Running sessions and user config/session data were left alone.

- No live inference or manual terminal walkthrough ran. Existing debug linker
  unwind-section warning is non-fatal. Oracle/reference status remains empty;
  the previously documented protected-tree baseline discrepancy is not repaired.


## 2026-09-04 PLN-241 Plan Review and Execution

- Failure-first: approval retained an active planning turn; the Markdown fixture
  rendered `testsApprove the plan.`. Both failures are recorded in
  `/tmp/ca-pln241-characterization.log` and `/tmp/ca-pln241-markdown-before.log`.
- `cargo test --locked --bin ca plan_ -- --test-threads=1`: PASS, 37 tests with
  ordinary snapshot assertions. Includes current-agent request/tool rebuilding,
  a successful temporary write through normal tool execution despite empty
  graph grants, approval-only, stale/duplicate actions, save rollback, revision,
  dismissal, legacy body reads, history waiting, and complete-plan copying.
- `cargo test --locked --bin ca markdown::tests -- --test-threads=1`: PASS, 20.
- Reviewed five new paired 100x30/60x24 plan snapshots. The first full render
  pass found three expected list/block spacing differences in existing learning
  and math fixtures; those changes were reviewed and accepted.
- Full render target: PASS, 224 tests and one unrelated manual check ignored.
- Disposable-home local fake-provider PTY walkthroughs: PASS at 100×30 and
  60×24. Both propose, read, revise, dismiss/reopen, then Execute with fresh
  Agent tools to write and read back a two-line proof file. Seven physical
  requests each, no duplicate start. Logs: `/tmp/ca-pln241-manual-wide-final.log`
  and `/tmp/ca-pln241-manual-narrow-final.log`. Early attempts used an incorrect
  ANSI-stripping assertion; screen-emulator assertions corrected that fixture.
- Final 37-test focused rerun also verifies cancellation clears the cached
  request and restoration clears an actual pending execution/review binding.
- Formatting, diff whitespace, and strict all-target/all-feature workspace
  Clippy: PASS. No pending snapshot files.
- The full workspace run reported one unrelated CA status-line test timeout:
  `ui_settings::tests::fake_required_sandbox_proves_profile_network_denial_and_output_bound`.
  Its unchanged compiled test passed immediately in isolation (1/1). Keep the
  original failed run as evidence; no timeout or assertion was weakened. Logs:
  `/tmp/ca-pln241-workspace-tests.log`, `/tmp/ca-pln241-status-sandbox-rerun.log`.
  Full totals: 3,373 passed, one failed, seven ignored; every other target
  passed (including 8/8 MCP SDK tests). The broad command exited 101 because
  of that timeout; its isolated rerun passed.
- Protected-tree verification still fails on the existing committed oracle
  baseline discrepancy. Direct `git status --short -- oracles reference` is
  empty. No protected-tree content or receipt changed.

- All-feature workspace release build and release 60×24 fake-provider PTY:
  PASS. Managed background job 42112 installed `/Users/aji/.local/bin/ca` after
  verifying candidate/staged/installed hashes, strict signatures, executable
  mode/ownership, `--version`, and identical `--help`. Installed SHA-256:
  `d9cbe4aac3947fb79eba4510ecafba0f53513f526045fba5c91ce41c6efa88e6`.
  Recovery: `/Users/aji/.local/bin/.ca-recovery-20260904T042329Z.jRN6nS/ca.previous-0336bd2050b525d71959f885af40ccc603873f534a0821b85e8bdd3c53fd0192`.
  Log/script/terminal evidence: `/tmp/ca-pln241-release.12xj4sgt/`.
  Independent installed hash, signature, mode/owner, and version checks pass.
- No live inference, user config/session mutation, oracle/reference edit, or
  commit occurred. The debug build's existing oversized unwind-section linker
  warning was non-fatal. Documentation-only completion does not repeat Rust gates.

## 2026-09-03 TUI-240 Stop and Interrupt

- Failure-first regressions reproduced busy Enter not interrupting, transient
  status hiding the stop controls, and foreground commands outliving an aborted
  tool waiter. The final interrupt-focused suite passes 14 tests, including
  controls in Vim normal mode and blocked prompt-hook handling.
- Focused all-feature input suite: 116 passed; queue: 46 passed; job lifecycle:
  31 passed. New coverage includes all active phases, old-generation rejection,
  preserved draft/images, held queues, queued edit/send/requeue, stopped-save
  failure recovery, popup precedence, process-group cancellation, promotion and
  ownership boundaries, and fail-closed prompt-hook submission.
- All-feature render suite: 221 passed/1 manual check ignored. Reviewed 13
  existing snapshot updates and 3 new paired 100x30/60x24 snapshots: persistent
  busy controls, stopped/held queues, and sending an edited held draft. Updated
  existing diffs contain only controls, queue labels, and keymap help copy.
- Disposable-home, loopback-only real PTY walkthrough: PASS. Exactly three
  fixture requests (`initial work`, `replacement`, `tool work`); Alt+Enter queued
  without another request, Enter interrupted/sent, old delayed output and queued
  work stayed inert, Esc cancelled the foreground command, `/jobs` confirmed
  cancellation, and the terminal exited cleanly. No credentials or inference
  service were used; no actual user config/session was touched. Script/logs:
  `/tmp/ca-tui240.oawiMS/`.
- Formatting and diff hygiene pass. Strict Clippy found one duplicated hint
  branch; it was merged without changing UI output. Final strict all-target/
  all-feature workspace Clippy passes. Final CA suite: 2,486 passed/5 manual
  checks ignored after the Vim-mode refinement. The full all-feature workspace
  suite passes 3,357 tests across 20 targets, with 7 manual/integration checks
  ignored. Experiment 118/1 ignored, codex-team 652/1 ignored, CLI 38, role-run
  17, MCP stdio 8, and all remaining integration/doc-test targets pass. The final
  CA rerun includes the additional Vim test after the workspace's CA phase.
- The all-feature optimized release builds and passes version/help, strict
  macOS signature, regular/single-link owner-501/mode-0755, and arm64 checks.
  The same loopback PTY walkthrough also passes against this release at 60
  columns, with exactly three requests and clean exit. SHA-256:
  `925756efc7abeda78d6ede26000ade8cc788cfdf5e3215ac2e711eb9f97ee69d`.
  No installation ran. The current executable remains unchanged at
  `0336bd2050b525d71959f885af40ccc603873f534a0821b85e8bdd3c53fd0192`.
- Protected-tree verification still reports only the pre-existing `oracles/
  (committed content baseline does not match)` discrepancy. Direct oracle/
  reference status is empty; no protected content or receipt was changed.

## 2026-09-03 BUG-239 Native Codex Catalog

- Failure-first native-format test reproduced `provider returned an invalid
  model catalog`; the minimal native decoder/request fixed it.
- `cargo test --bin ca native_codex -- --test-threads=1`: PASS, 11 tests and
  one explicitly ignored live check. Coverage includes seven visible IDs,
  non-API Spark, hidden/unknown visibility, version/account/CA identity headers,
  malformed/oversized/streamed responses, timeout, HTTP failures, empty success,
  repeated failure/recovery, configured-only rows and selecting Spark.
- `cargo test --bin ca catalog -- --test-threads=1`: PASS, 73 tests/2 ignored,
  including other providers, exact route and custom-endpoint isolation, both
  catalog refresh paths, selection, and prior fallback behavior.
- Five new paired 100x30/60x24 snapshots reviewed: recent, middle, configured-only,
  cached failure, and provider-specific full list. All seven fixture IDs appear
  across both widths; narrow cached status visibly says request failed. The
  previous documented-fallback snapshot remains unchanged.
- Manual `tui::render_tests::native_codex_live_catalog_matches_picker` ran
  explicitly with `--ignored --nocapture`: PASS, seven account-returned IDs and
  exact picker/live equality. Wide/narrow real-response renders were reviewed
  in `/tmp/ca-bug239-live.log`. Uses CA auth resolution only, no inference,
  config/session writes, or credential output. The seven IDs are in CONTINUITY.
- Formatting, diff hygiene, strict all-target/all-feature workspace Clippy:
  PASS. Initial Clippy identified an unchecked byte count in the new chunked
  test server; it now asserts a nonempty read. The final all-feature catalog
  suite passes all 18 tests after that correction.
- `cargo test --workspace --all-features --no-fail-fast -- --test-threads=1`:
  PASS, exit 0. CA 2,473 passed/5 ignored, experiment 118/1 ignored, CLI 38,
  role-run 17, codex-team 652/1 ignored, MCP stdio 8, and all remaining
  integration/protocol/doc-test targets pass. The ignored live catalog test
  ran and passed separately; other manual/integration skips remain unchanged.
- `cargo build --release --workspace --all-features`: PASS. Candidate
  `--version` (`ca 0.1.0`), `--help`, strict macOS signature, regular single-link
  owner-501/mode-0755 and arm64 artifact checks pass. Candidate SHA-256:
  `1ab5d08e0b5075f353828c613e24deeee2cc4849627c7109657f9b1ca5f292a4`.
- The user's latest instruction cancels installation for this task. No install
  job was launched. `/Users/aji/.local/bin/ca` is verified unchanged at
  `0336bd2050b525d71959f885af40ccc603873f534a0821b85e8bdd3c53fd0192`.
- `target/release/codex-team verify protected-trees --repo . --json`: still
  BLOCKED by `oracles/ (committed content baseline does not match)`. Direct
  oracle/reference Git status remains empty. No protected tree/receipt changes.


## 2026-09-03 REL-238 Background Release Installation

- Shell syntax and documentation diff checks: PASS. The standing rule and both
  older installation clauses now agree on the user's authorization.
- Tool-managed background job 57224 / PID 45741: PASS, exit 0. It ran
  `cargo build --release --workspace --all-features` (0.20s cached build), then
  required candidate SHA-256 to equal the previously tested BUG-237 artifact.
- Candidate, staged, and installed SHA-256 are identical:
  `0336bd2050b525d71959f885af40ccc603873f534a0821b85e8bdd3c53fd0192`.
  Installed PATH resolves `/Users/aji/.local/bin/ca`; strict code signature,
  regular/single-link ownership/mode checks, `ca 0.1.0`, and help smokes pass.
- Prior SHA-256 `1e91f11413c0d48cdfe6bbc9af2e1f9bee7c98e4d739e87d4865ea2fb2418f72`
  is verified in the timestamped recovery copy recorded in `CONTINUITY.md`.
- The first managed run stopped before replacement because the temporary
  executable's basename changed its help usage line. Staging under `candidate/ca`
  corrected that fixture assumption; strict help equality and the final install
  then passed. Raw detached dispatch had exited before the build, so the job
  uses a tool-managed asynchronous session. Attempt logs are retained under
  `/tmp/ca-rel238.kXCjlb/`.
- No Rust source changed; preceding BUG-237 focused/full tests, formatting, and
  strict Clippy results apply to this exact artifact. No full-suite rerun or
  live provider call was needed. The known protected-tree baseline mismatch
  remains separately recorded, not repaired or reported as passing.

## 2026-09-03 BUG-237 OpenAI Codex Picker Fallback

- Failure-first: the exact Codex route returned an empty documented catalog
  instead of Sol/Terra/Luna. `cargo test model_catalog --no-fail-fast` now
  passes 12/12, including exact-route and custom-endpoint isolation.
- `cargo test failed_codex_catalog_keeps_all_supported_openai_models_visible
  --no-fail-fast`: PASS. It derives the real documented fallback, injects a
  catalog failure, and checks all three model IDs plus truthful fallback status.
- `cargo test model_picker_documented_codex_fallback_snapshots --no-fail-fast`:
  PASS. The new paired 100x30/60x24 snapshot was reviewed with all three models,
  current selection, and the discovery failure visible.
- `cargo test --workspace --all-features --no-fail-fast -- --test-threads=1`:
  PASS. CA 2,462 passed/4 ignored, experiment 118/1 ignored, CLI 38/38,
  role-run 17/17, ca-watch 18/18, codex-team 652/1 ignored, MCP stdio 8/8,
  and every remaining integration/protocol/doc-test target passed.
- Formatting, strict all-target/all-feature workspace Clippy, `git diff
  --check`, and zero `.snap.new`: PASS.
- `cargo build --release --workspace --all-features`: PASS. Release
  `--version` (`ca 0.1.0`) and `--help` smokes pass; no installation ran.
  The focused test rebuild emitted the existing non-fatal macOS oversized
  `__eh_frame` linker warning.
- `target/release/codex-team verify protected-trees --repo . --json`: BLOCKED
  by the previously recorded `oracles/` committed-baseline mismatch. Direct
  Git status for `oracles/` and `reference/` is empty; neither tree nor receipt
  was changed for this task.
- No live account catalog request or manual running-TUI walkthrough was run;
  deterministic fallback and render coverage do not prove model entitlement.
  Runtime config, sessions, credentials, and the installed binary are unchanged.

## 2026-09-03 SES-236 Fresh Session Command

- Failure-first: the `/new` registry characterization failed because no command
  existed. `cargo test --locked -p ca new_session -- --test-threads=1` now
  passes 10/10. Coverage includes source preservation, independent identity,
  first-prompt title generation, queues, save failure with no provider events,
  paged history/generation validation, stale completion, active work, empty
  starts, and bare/`--no-session` behavior.
- `cargo test --locked -p ca tui::render_tests -- --test-threads=1`: PASS,
  218/218. The new paired 100x30/60x24 snapshots show `/new` discovery and the
  empty new-session confirmation. The existing command discovery/palette
  snapshots were reviewed and updated for the extra command; the first two
  full runs stopped at those expected snapshot differences.
- `cargo test --locked -- --test-threads=1`: PASS. CA 2,431 passed/4 ignored,
  CLI 38/38, role-run 17/17, ca-watch 18/18, codex-team 652 passed/1 ignored,
  MCP stdio 8/8, and every remaining integration/protocol/doc-test target pass.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo build --release`, and release `--version`/`--help`: PASS. The known non-fatal macOS oversized
  `__eh_frame` linker diagnostic remains. Snapshot hygiene and
  `git diff --check` pass.
- Disposable-home terminal walkthrough: PASS. A local missing-credential
  fixture created an old prompt without HTTP traffic. `/new` saved that source
  and started an independent root containing only the new prompt; both saved
  identities have no parent. `/sessions` successfully reopened the old root.
  The terminal exited cleanly. No real provider or user state was used.
  Logs and the exact disposable home were moved recoverably to
  `/Users/aji/.Trash/ca-ses236.bwusk8`.
- Protected-tree verification remains BLOCKED by the pre-existing ignored
  `oracles/` committed-baseline mismatch; direct Git status for `oracles/` and
  `reference/` is empty. Neither protected trees nor receipts were changed.
- Release SHA-256: `1e91f11413c0d48cdfe6bbc9af2e1f9bee7c98e4d739e87d4865ea2fb2418f72`.
  The installed binary remains unchanged at
  `682c45492e8e7060148f7d853cc783d394af1a2f8768195871042cd19aa1df12`.
  Installation was not requested or performed.

## 2026-09-03 BUG-235 Bounded Pre-Output Recovery

- Failure-first loopback coverage reproduced the old terminal body-stream
  transport error instead of replaying, and the old submission path retained
  the same source session ID instead of branching.
- `cargo test --locked -p ca agent::client::tests -- --test-threads=1`: PASS,
  58/58. Coverage includes identical frozen requests, a single request-store
  record, shared header/body exhaustion, bounded 429/5xx delays, HTTP 400 and
  malformed-stream suppression, metadata discard, text/reasoning/tool commit
  boundaries, and cancellation during delay, headers, and body work.
- `cargo test --locked -p ca replacement -- --test-threads=1`: PASS, 20/20
  matched tests. Ten BUG-235 cases cover persistent and in-memory branches,
  immutable failed sources, repeated failures, queues, branch failure with no
  provider I/O, hooks, attachments, paged history, partial output, and resolved
  tools.
- `cargo test --locked -p ca pre_output_recovery_snapshots -- --test-threads=1`:
  PASS, 1/1. The two combined snapshot artifacts were
  reviewed at 100x30 and 60x24; each shows one prompt and complete actionable
  copy without the generic HTTP 400 footer.
- `cargo test --locked -- --test-threads=1`, `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, and `cargo build --release`:
  PASS. The test/release link retains the known non-fatal macOS oversized
  `__eh_frame` diagnostic.
- Snapshot hygiene and `git diff --check`: PASS. Direct Git status/diff for
  `oracles/` and `reference/` is empty.
- Disposable-home loopback TUI walkthrough: PASS. Six physical requests proved
  two byte-identical request pairs, exactly one HTTP 400 request, a source that
  retained `fail twice`, a child that replaced it with `replacement`, and a
  grandchild that replaced the rejected prompt with `replacement fixed`.
  Recovery and rejection footers rendered fully and the terminal exited
  cleanly. The exact walkthrough root was moved recoverably to
  `/Users/aji/.Trash/ca-bug235.hGzW2A`.
- `target/release/codex-team verify protected-trees --repo . --json`: BLOCKED.
  The ignored `oracles/` tree has a stable digest mismatch
  (`bd0de373…189e` expected, `8dca8724…e258` observed) with identical counts;
  `reference/` matches. BUG-235 did not change protected-tree or receipt bytes,
  and neither was altered to make the gate pass.
- No real provider, credential, user configuration/session, oracle/reference
  mutation, or binary installation was used.

## 2026-09-02 BUG-234 DeepSeek DSML and Dream Queue Recovery

- Focused DSML/stream/client/config/provider coverage passes: normalizer 10/10,
  client 48/48, stream 17/17, config store 103/103, and provider contract 8/8.
  The typed-plan loopback proves screenshot-shaped `propose_plan` markup never
  enters transcript content, executes once through the structured tool path,
  and returns its result to the continuation.
- Focused learning/codex-learning coverage passes 108/108 and 69/69. It covers
  exact private legacy backup, same-ID corrected digest/totals, new-digest
  completion, idempotence, and rejection of tampered, unsafe, malformed,
  symlinked, colliding, and non-legacy records. Startup exposes only a bounded
  recovery count.
- At the user's explicit follow-up request, `cargo fmt` corrected the paused
  AUT-231 differences in `src/automation.rs`, `src/schedule.rs`,
  `src/session.rs`, and `src/main.rs`. The existing due-claim path now runs from
  schedule tick/daemon execution, the redundant public successful-tool field is
  removed, and focused automation 8/8 plus schedule 11/11 pass.
- The default serial workspace matrix passes: CA 2,395/4 ignored, CLI 38/38,
  role-run 17/17, ca-watch 18/18, codex-team 652/1 ignored, MCP stdio 8/8,
  and every other target/doc test passes.
- The first all-feature matrix exposed a one-in-sixteen test-only lifecycle
  flake: replacing a capability's first hex digit with `0` sometimes made no
  change. The test now selects the opposite digit; its focused rerun passes,
  and the clean replacement matrix passes with CA 2,424/4 ignored, experiment
  118/1 ignored, codex-team 652/1 ignored, MCP stdio 8/8, and all other targets.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo build --workspace --release
  --all-features`: PASS. The only test-build diagnostic is the known non-fatal
  macOS oversized `__eh_frame` compact-unwind warning.
- `target/release/ca` is a regular single-link owner-matching mode-0755 arm64
  executable. It passes strict code-signature verification, has no quarantine
  attribute, reports `ca 0.1.0`, and passes help, repository JSON, and
  disposable-profile schedule-tick smokes. SHA-256 is
  `60727a1c249dbef2984f32b07a0a4c53dfc59321b5be64eb5067b2b5087ad566`.
- `git diff --check`, zero `.snap.new`, direct protected status/diff, the
  HEAD-trusted protected-tree verifier, and exact-name orphan checks pass. The
  verifier reports `oracles/ and reference/ are unchanged`.
- No live provider request, credential access, real profile/session/learning
  mutation, protected-tree edit, or LaunchAgent operation ran.

## 2026-09-02 TUI-233 Distinct Sub-Agent Worker Icons

- Failure-first: `cargo test -p ca inline_agent_activity -- --nocapture`
  failed because the existing rows still rendered generic `·` bullets.
- Focused repair: the same command passes 1/1 after adding deterministic icon
  selection. Exact buffer assertions cover distinct glyphs and RGB colors, and
  six reviewed wide/narrow snapshots cover collapsed, focused/detail, and
  restored-summary presentation.
- `cargo test -p ca tui::render_tests -- --test-threads=1`: PASS, 215/215.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS. CA passes
  2,380 tests with four documented ignores; CA CLI passes 38/38, role-run
  17/17, ca-watch 18/18, codex-team 652/1 ignored, MCP stdio 8/8, and all other
  targets and doc tests pass.
- `rustfmt --edition 2021 --check src/tui/render.rs
  src/tui/render_tests.rs`: PASS.
- `cargo fmt --all -- --check`: BLOCKED by pre-existing paused AUT-231
  formatting differences in `src/automation.rs`, `src/schedule.rs`,
  `src/session.rs`, and the existing module order/one test line in
  `src/main.rs`; no broad formatter was run over that user-owned work.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  BLOCKED only by pre-existing unused AUT-231 items
  `AutomationStore::claim_due` and `AutomationExecution::successful_tools`.
- `cargo build --workspace --release --all-features`: PASS with only those two
  known AUT-231 dead-code warnings.
- `git diff --check`, zero `.snap.new` files, and direct `oracles/`/`reference/`
  status: PASS.
- No provider request, runtime state mutation, protected-tree edit, binary
  installation, or credential access ran.

## 2026-09-01 BUG-232 Unavailable-Tool Recovery

- Failure-first focused compilation failed because the new unavailable-tool
  classifier did not exist; after the repair,
  `cargo test -p ca unavailable_tool -- --nocapture` passes 4/4 and
  `cargo test -p ca mixed_tool_batch_leaves_only_advertised_calls_executable
  -- --nocapture` passes 1/1.
- Coverage proves a first hidden call becomes a resolved error without
  authority widening, mixed batches execute only advertised calls, a repeat
  fails closed, the marker is scoped to the active logical user turn, and a
  rejected mutating call never reaches execution or creates its target.
- `cargo test -p ca tool_selection::tests -- --nocapture`: PASS, 17/17,
  including registry-order selection, profile upper bounds, restrictive
  removal-only behavior, and explicit toolless-model rejection.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS. The CA
  binary passes its complete 2,384-test target with four documented ignores;
  codex-team passes 652/1 ignored, MCP stdio passes 8/8, and all other targets
  and documentation tests pass.
- `cargo build -p ca`: PASS. It reports the same two pre-existing AUT-231
  dead-code warnings plus the known non-fatal macOS compact-unwind linker
  warning; `target/debug/ca` is directly runnable but was not installed.
- `cargo fmt --all -- --check`: BLOCKED by pre-existing paused AUT-231
  formatting differences in `src/automation.rs`, `src/schedule.rs`,
  `src/session.rs`, and the existing `src/main.rs` module order. No broad
  formatter was run over those user-owned changes.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  BLOCKED only by pre-existing unused AUT-231 items
  `AutomationStore::claim_due` and `AutomationExecution::successful_tools`.
- `git diff --check`, zero `.snap.new`, and direct `oracles/`/`reference/`
  status: PASS.
- Initial parallel and serial all-feature release attempts were blocked by an
  imported generated cache: macOS stalled in `fcntl` while loading cached
  proc-macro dylibs. After explicit follow-up authority,
  `cargo clean --release` removed 11,222 generated files/5.7 GiB and the fresh
  normal parallel `cargo build --workspace --release --all-features` passed in
  3m03s. `target/release/ca` is an arm64 Mach-O, reports `ca 0.1.0`, passes
  help and strict code-signature checks, has no quarantine attribute, and has SHA-256
  `6e97b2388c413a649bada43952ffe2791c545a8d0d8621553a698876f5bd1eb8`.
- No real provider request, runtime configuration/session mutation,
  oracle/reference edit, binary installation, or credential access ran. The
  removed release cache was generated and fully recreated by Cargo.

## 2026-08-29 PRT-230 Finder Metadata Exclusion and REL-160 Repair

- Failure-first protected-tree coverage failed with four oracle entries versus
  the expected two because root and nested `.DS_Store` files were hashed.
- Algorithm `sha256-merkle-v2-ds-store-excluded` filters exact basename
  components before traversal accounting. Protected-tree unit tests pass 6/6;
  protected-baseline CLI tests pass 3/3. Mutable root/nested Finder metadata is
  ignored, while `.DS_Store.keep` still produces drift.
- The exact double-scanned apply receipt uses revision
  `1530d660916ecb8d6f5befa68d1a850e9912c674` and manifest digest
  `eeb36e1d625884a375ffa48fe244491902d198b08a9054f7ce572a16ebceb4dc`.
  Four Finder files are excluded; the oracle remains 42,161 entries and
  1,401,108,346 protected bytes. Neither protected tree was modified.
- Full codex-team package: 652 passed/1 ignored; MCP stdio 8/8 and all
  integrations pass. The first workspace attempt reproduced REL-160's known
  test-only exact-count race. The corrected invariant passed ten consecutive
  focused runs.
- Replacement default workspace matrix: PASS, CA 2,371/4 ignored, codex-team
  652/1 ignored, MCP stdio 8/8, every other target pass.
- Replacement all-target/all-feature matrix: PASS, CA 2,400/4 ignored,
  experiment 118/1 ignored, codex-team 652/1 ignored, MCP stdio 8/8, every
  other target pass.
- Formatting, strict workspace all-target/all-feature Clippy, workspace
  all-feature release, candidate strict signature, `git diff --check`, zero
  `.snap.new`, and direct oracle/reference status/diff: PASS.
- After explicit authority, the manifest-only commit contains exactly
  `.codex-team-protected-trees.json`; the HEAD-trusted release verifier passes.
  The installed binary matches the candidate SHA-256, strict signature,
  ownership/mode, path lookup, help/version, repository JSON, and representative
  new-command smokes.

## 2026-08-29 REL-229 Integrated ORC-218 Gates

- `cargo test --workspace --no-fail-fast -- --test-threads=1` passes on the
  clean rerun. CA passes 2,371 tests with four documented ignores; all other
  workspace libraries, binaries, integrations, examples, documentation tests,
  codex-team's 652/1 ignored tests, and MCP stdio 8/8 pass.
- The first default-matrix attempt exposed
  `learning_journey::tests::concurrent_writers_preserve_every_metadata_event`:
  same-path ledger handles owned different process mutexes, and a contender
  could see the lock disappear between collision and metadata inspection. A
  shared weak per-path lane plus retry-on-disappearance repairs the race. The
  new lane-identity test, the original concurrency test, and the complete
  matrix rerun pass.
- `cargo test --workspace --all-targets --all-features --no-fail-fast --
  --test-threads=1` passes. CA passes 2,400/4 ignored, the feature-gated
  reasoning experiment passes 118/1 ignored, codex-team passes 652/1 ignored,
  MCP stdio passes 8/8, and every other target passes.
- `cargo fmt --all -- --check`, strict workspace all-target/all-feature Clippy,
  and `cargo build --workspace --release --all-features` pass. Strict Clippy
  initially identified two test-only pane construction styles; after the
  mechanical cleanup, format and Clippy pass and both affected pane tests pass
  individually.
- Release help/version and the support-bundle, configuration provenance, ACP,
  Claude import, and repository command help smokes pass. Repository info and
  status JSON smokes emit `ca.repo-info.v1` and `ca.repo-status.v1`; worktree
  preview emits `ca.worktree-setup-preview.v1`. No provider request ran.
- `target/release/ca` is a regular owner-matching mode-0755 arm64 executable.
  Strict code-signature verification passes and its SHA-256 is
  `3ed7b2347f22671d2d45e25fd3d640309ecf6a734378e14fbdbff610409d0392`.
- `git diff --check` passes, no `.snap.new` files exist, direct staged and
  unstaged inspection of `oracles/` and `reference/` is empty, and exact-name
  orphan checks for CA, codex-team, keep-awake, and watcher helpers are zero.
  The final reconciled status has 908 records versus the frozen 868-record
  baseline (sorted SHA-256
  `5b4b82652500f967716197626567f8ab3096d2053012cc5942f0d603db208f3a`): the
  sole removed record is AGT-157's expected active-to-completed plan move, and
  all 41 additions are attributable to ORC-218/PRT-230.
- PRT-230 replaced the obsolete v1 comparison with an exact-basename
  `.DS_Store` exclusion and a digest-bound v2 manifest. After the separately
  authorized exact manifest commit, the HEAD-only verifier passes; neither
  protected tree has status or diff.
- A separate read-only Merkle reconstruction produced the exact current
  `oracles/` digest
  `1f609167983f81f6330227e59ac42e8e696608d303e64ca4f17904118304fe81`,
  validating the diagnostic method. Virtual substitution of every ordinary
  10,244-byte `.DS_Store` candidate found under local project paths produced a
  different root each time. A pre-drift Time Machine snapshot exists, but its
  protected backup volume was not mounted or restored.

## 2026-08-29 PRV-228 Responses WebSocket Deferral

- Official OpenAI stable and Beta Responses references were searched and
  fetched read-only. Stable creation documents `POST /responses`; WebSocket
  client/server event types and input injection are currently under the Beta
  Responses resource with `beta_*` identities.
- The dated readiness record lists the complete stable evidence and
  failure-first fixture contract required before any future implementation.
  Source/code/dependency/config/CLI/session scans show no PRV-228 provider
  surface was added. No API request, credential access, runtime-state mutation,
  protected/oracle edit, build requirement, or installation applies to this
  documentation-only child.

## 2026-08-29 IMP-227 Safe Claude Setup Import

- Focused setup coverage passes 10/10 across the versioned JSON preview,
  project/user mapping, sensitive exclusions without retained values, native
  rule/command/skill conversion, malformed skill metadata, symlink/hardlink and
  destination-symlink refusal, file/byte caps, collision/drift, bare refusal,
  no-overwrite idempotence, private staging, injected rollback, CLI shape,
  Cancel-first input, scrolling, and reviewed wide/narrow snapshots. The slash
  orchestration and command-registry regressions also pass.
- The complete serial CA binary suite passes 2,370 tests with four documented
  ignores. `cargo fmt --all -- --check`, strict CA all-feature Clippy, and
  `cargo build -p ca --release --all-features` pass. Release help for `ca import`
  and `ca import setup claude` exposes the required command shape.
- `git diff --check`, zero `.snap.new`, and direct `oracles/`/`reference/`
  inspection pass. Final workspace matrices remain REL-229's integrated gate.
  No provider request, real setup import, runtime-state mutation, or
  installation ran.

## 2026-08-29 EDT-226 ACP Context and Hash-Anchored Editing

- Stable ACP resource coverage passes 12/12, including official SDK
  initialize/new/load/prompt/mode behavior, embedded text, contained `file:`
  links and line ranges, 32-item/64-KiB/128-KiB bounds, binary/unsupported
  media, remote URI, traversal, symlink, and malformed range rejection.
- Anchored-read/edit coverage passes for Unicode and CRLF, optional default
  compatibility, exact receipts, stale/missing/duplicate/reversed/malformed
  anchors, schema, approval-preserving execution, undo, and artifact capture.
- The complete serial CA binary suite passes 2,359 tests with four documented
  ignores. Formatting, strict CA all-feature Clippy, and all-feature release
  pass; the existing macOS oversized `__eh_frame` warning remains non-fatal.
  Final workspace matrices remain REL-229's integrated gate. No provider
  request or installation ran.

## 2026-08-29 PAN-225 Bounded Split-Pane Sessions

- Pane-focused coverage passes 11/11 for nested bounded layouts, the four-pane
  cap, dimension refusal, geometry hit testing, complete state isolation,
  session retention on close, pane-tagged streams, simultaneous generation
  isolation, command visibility, and configurable two-stroke controls.
- The focused background-approval regression proves that an unfocused pane
  cannot automatically execute mutating, network, or unknown work. Existing
  session loading for `/split ... SESSION` uses a blocking worker and returns
  through the originating pane router.
- Reviewed `split_panes_vertical_wide`, `split_panes_horizontal_narrow`, and
  `split_panes_four_nested_wide` snapshots show stable borders, focus badges,
  independent transcripts/composers, and bounded narrow layouts. No dedicated
  PTY harness exists in this repository; key, mouse, resize geometry, focus,
  concurrent stream, and terminal restoration are covered at the deterministic
  input/event/render boundaries instead. Zero `.snap.new` files remain.
- The complete serial CA binary suite passes 2,353 tests with four documented
  ignores. `cargo fmt --all -- --check`, strict CA all-feature Clippy, and
  `cargo build -p ca --release --all-features` pass. The existing macOS
  oversized `__eh_frame` warning remains non-fatal. Final workspace matrices
  remain REL-229's integrated gate; no provider request or installation ran.

## 2026-08-29 ART-224 Session Artifact Ledger

- Artifact-focused coverage passes 13/13 across schema validation, cap and
  coalescing, mutation/read capture, containment/symlink/drift handling,
  historical branch boundaries, portable-export omission, and `/artifacts`
  behavior. The explicit read-tracking configuration regression and existing
  mutation receipt test also pass.
- Reviewed `session_artifact_browser_wide` and
  `session_artifact_browser_narrow` snapshots show bounded list/search guidance.
  Updated command-palette snapshots include `/artifacts`; zero `.snap.new`
  files remain.
- The first serial workspace run passed every non-CA target and found two CA
  fixture changes: fuzzy search for `audit` also found `/artifacts`, and the
  palette count/content predated REV-221/ART-224. After making the assertion
  command-specific and reviewing the snapshot deltas, the complete CA binary
  suite passes 2,341 tests with four documented ignores.
- `cargo fmt --all -- --check`, strict workspace all-target/all-feature Clippy,
  and `cargo build --workspace --release --all-features` pass. The existing
  macOS oversized `__eh_frame` linker warning remains non-fatal. No provider
  request or installation ran; the two integrated workspace matrices remain
  REL-229's final gate.

## 2026-08-29 SYS-223 Active-Work Lifecycle

- Fake-helper tests pass 3/3 for reference counting, duplicate release,
  debounce generation, idempotent shutdown, and the exact macOS idle-only
  absolute helper command. Linux code uses an exact `systemd-inhibit`/fixed
  sleep shape and a parent-death signal.
- All 29 job-manager tests pass, including graceful TERM output, grace-bound
  escalation, owner rejection, process-group cleanup, output bounds,
  promotion, timeout, shutdown, and completion claims.
- All 30 job-tool tests pass. Focused cases cover stop/timeout conflict,
  grace-without-stop, the 300-second cap, final cancelled output, and no later
  automatic duplicate. All 101 configuration tests, CLI override conflicts,
  schema validation, and configuration provenance tests pass.
- Formatting, strict workspace all-target/all-feature Clippy, all-feature
  release, `git diff --check`, zero `.snap.new`, and direct oracle/reference
  inspection pass. Complete matrices remain REL-229's integrated gate; no
  provider request or installation ran.

## 2026-08-29 WFL-222 Audited Built-In Workflows

- The workflow-path suite passes 40/40, including immutable identity,
  definition/runtime validation, route/tool/network restrictions, scheduler,
  pause/resume/cancel, session fork/restore, and command behavior. The 16 core
  workflow tests pass independently.
- Focused regressions prove three configured pairwise-distinct fact-check
  routes, request-level juror isolation from ambient/prior-verdict messages, a
  clean CA-owned worktree requirement, 1–10 iteration bounds, digest/network
  binding, and rejection of a widened evaluation command before approval.
- Reviewed wide/narrow definition and live-run tree snapshots expose only safe
  structure/state metadata; zero `.snap.new` files remain.
- `cargo fmt --all -- --check`, strict workspace all-target/all-feature Clippy,
  `cargo build --workspace --release --all-features`, `git diff --check`, and
  direct `oracles/`/`reference/` inspection pass. The existing macOS oversized
  `__eh_frame` linker warning is non-fatal. Complete matrices remain the
  integrated REL-229 gate; no provider request or installation ran.

## 2026-08-29 REV-221 Convergent Review and Acceptance

- Convergence tests pass 3/3 for missing/duplicate/incompatible/unpriced
  routes, concurrent isolated starts, byte-identical objectives, pass/pass and
  disagreement, and shared request/output/exact-cost ceilings.
- Acceptance tests pass 4/4 for host-only verified labels, secret-sentinel
  exclusion, private immutable publication/export, revision staleness, packet
  digest validation, and symlink/hardlink refusal. Existing review tests pass
  5/5.
- Deterministic headless fixtures pass for a successful two-reviewer packet and
  for explicit repair exhaustion at three fresh pairs/six reviewer
  invocations. Configuration route/budget validation and CLI flag dependency
  coverage pass.
- `cargo fmt --all -- --check`, strict workspace all-target/all-feature Clippy,
  `cargo build --workspace --release --all-features`, `git diff --check`, zero
  `.snap.new`, and direct `oracles/`/`reference/` inspection pass. Complete
  workspace matrices remain the integrated REL-229 gate; no provider request
  or installation ran.

## 2026-08-29 RES-220 Streaming Anomaly Protection

- Failure-first anomaly cases now pass 10/10: Unicode/chunk-safe exact spans,
  near-identical paragraphs, reasoning, bounded state, advertised-tool caps,
  JSON/XML envelopes, and fenced/code/short/malformed/valid-tool controls.
- A byte-at-a-time Chat SSE loopback preserves two safe Unicode spans, rejects
  the third, drops the response, and proves no second request under a
  two-attempt policy. A pseudo-tool loopback preserves assistant text, emits no
  tool event, and returns the typed suspected-envelope failure.
- Existing suites pass: stream assembly 17/17, provider decoders 34/34, client
  transports and continuations 47/47, fallback 4/4, provider probe 8/8,
  service/headless 25/25, and the shared interactive partial-output regression.
- `cargo fmt --all -- --check`, strict workspace all-target/all-feature Clippy,
  and `cargo build --workspace --release --all-features` pass. `git diff
  --check`, zero `.snap.new`, and direct `oracles/`/`reference/` inspection
  pass. Complete workspace matrices remain the integrated REL-229 gate; no
  network provider, runtime profile/session, or installed binary was touched.

## 2026-08-29 DIA-219 Support Bundles and Configuration Provenance

- Focused suites pass: configuration provenance 3/3, support bundle 9/9,
  runtime support-snapshot 1/1, shell integration 4/4, slash-command behavior,
  and manually reviewed wide/narrow support-preview snapshots.
- The default serial workspace matrix passed every non-CA target. Its only CA
  mismatch was the expected command-palette count increase from 100 to 101;
  after reviewing and accepting those two one-line snapshot changes, the
  complete CA target rerun passed 2,289 tests with four documented ignores.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo build --workspace --release
  --all-features` pass. Release help/version and provider-free provenance JSON
  smokes pass. The existing macOS oversized `__eh_frame` linker warning remains
  non-fatal.
- `git diff --check`, zero `.snap.new`, and direct `oracles/`/`reference/`
  inspection pass. Bundle writes were exercised only in disposable fixtures;
  no user profile, provider, session, installed binary, or protected subtree
  was mutated.

## 2026-08-29 AGT-157 Host-Owned Agent Verification

- Focused coverage passes: agent graph 14/14, work state 16/16, graph tick
  3/3, task graph 5/5, explicit human review 1/1, provider one-attempt
  accounting, and exact decimal ordering.
- The default serial workspace matrix passes with CA 2,279 tests and four
  documented ignores; every workspace integration and protected target passes.
- The all-target/all-feature serial matrix passes with CA 2,304/4 ignored,
  the feature-gated experiment 118/1 ignored, codex-team 651/1 ignored, and
  every CLI, integration, protocol, example, and documentation target passing.
- Formatting, strict workspace all-target/all-feature Clippy, all-feature
  release build, `git diff --check`, zero `.snap.new`, and direct
  oracle/reference inspection pass. Real provider and worktree operations were
  deliberately not run, and no installation occurred.

## 2026-08-29 TST-217 Broker Cache-Publication Collision Repair

- Four deterministic fixed-clock tests pass for concurrent distinct
  reservations and complete same-target payloads, preserved foreign
  collisions, fail-closed 16-candidate exhaustion, and writer-owned cleanup.
  The full 11-test broker module passes, retaining private-mode, hardlink,
  tamper, pruning, and cache-size coverage.
- The original shared-query stdio regression passed three consecutive runs.
  The MCP unit suite passed 36 tests and the complete stdio SDK suite passed
  8/8.
- Both serial workspace matrices passed. The default matrix passed CA 2,267/4
  ignored and codex-team 651/1 ignored. The all-target/all-feature matrix passed
  CA 2,296/4 ignored, experiment 118/1 ignored, and codex-team 651/1 ignored;
  every integration target passed in both runs.
- Formatting, strict workspace all-target/all-feature Clippy, and the workspace
  all-feature release build passed. Diff/snapshot/protected-tree and exact
  initial-inventory checks are recorded at completion. No installation or live
  provider check was performed.

## 2026-08-29 TST-216 Deterministic Scheduler Test Repair

- Replaced the protected codex-team fixture's fixed eight-second build delay
  with Git-ignored marker barriers inside its disposable repository. The broker
  must report five consumers before release; the cancellation phase remains
  active until its final consumer cancels.
- The repaired case passed four consecutive focused runs and all three later
  broad invocations. `cargo test -p codex-team mcp:: -- --test-threads=1`
  passed 36 tests. The clean `cargo test -p codex-team --test mcp_sdk --
  --test-threads=1` rerun passed 8/8.
- The package run passed 647 active library tests with one documented ignore
  and every non-MCP integration target. Its sole failed case was the separate,
  previously recorded shared-query cache-publication `EEXIST` race; that case
  passed immediately alone, while the repaired cancellation case passed.
  TST-217 subsequently repaired the publication race and passed both complete
  workspace matrices.
- Formatting and strict all-target/all-feature codex-team Clippy passed. No
  production scheduler code, runtime state, installed binary, oracle, or
  reference content changed.

## 2026-08-29 CUR-204 Integrated Release

- Clean complete observations exist for both exact matrices. The default run
  passed 2,267 CA tests with four documented ignores and every workspace target.
  The all-target/all-feature run passed CA 2,296/4 ignored, experiment 118/1
  ignored, CLI 38, role-run 17, ca-watch 15 plus three CLI tests, codex-team
  647/1 ignored plus all integrations, protocol 12, examples, and doc tests.
- A later repeated default run passed every CA and protected library test but
  hit the known five-client cancellation timing race in the unchanged protected
  integration target; its exact immediate rerun passed. This does not replace
  or obscure the clean full default and all-feature observations.
- `cargo fmt --all -- --check`, strict all-feature Clippy, all-feature release,
  `git diff --check`, zero `.snap.new`, strict signature, release help/version,
  ACP help, repository JSON schemas, worktree preview, and zsh generator smokes
  pass. Direct protected status/diffs exactly match the 810-record baseline, and
  `codex-team verify protected-trees --json` passes.
- Exact inventory audit found zero missing or status-changed baseline records
  and exactly 58 additive CUR-204 paths: one lint configuration, 13 plan/evidence
  files, 18 source modules, and 26 reviewed snapshots.
- Candidate, staged, and installed SHA-256 are
  `30660c4e21598363feb3c99d11f271716f84e6f3e95907d07f633faa0af83111`.
  `/Users/aji/.local/bin/ca` is regular, single-link, `aji`-owned, mode `0755`,
  strictly signed, selected by path lookup, and passes representative smokes.
  The prior `82c72a53…03ea` binary remains in its timestamped full-SHA recovery
  copy. Two earlier smoke-harness failures triggered and verified exact rollback
  before the corrected final installation passed.

No provider/hosted request, configuration/session mutation, or protected-tree
source change occurred.

## 2026-08-29 CLI-214 Local Automation Flags

Focused validation:

- Parser and launch tests pass for strict pre-init `--min-version`, all new
  option conflicts, structured-only `--printenv`, strict UUIDs, and session
  collision refusal.
- Bounded `ca.session-export.v1` history validation, tool-call consistency,
  ephemeral no-session behavior, workspace-context exclusion, exact explicit
  skill retention, host Git capture/containment/binary/truncation behavior,
  key-only redacted environment events, and `ca.repo-info.v1`/
  `ca.repo-status.v1` schema tests pass.
- Release help/version and `ca repo info|status|worktree-setup` JSON smokes pass.

Child-gate status:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS. CA passed
  2,267 tests with four documented ignores; every CLI, role-run, workspace,
  integration, protocol, and documentation target passed.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo build --workspace --release
  --all-features`: PASS.
- Two exact all-target/all-feature workspace attempts passed every CA and
  feature-gated experiment target but encountered different timing races in
  unchanged protected `codex-team` tests. The exact persistent-receipt and
  stateful-filter cases passed on isolated rerun; the last-consumer cancellation
  case also passed on its next isolated rerun. The third complete exact matrix
  passed: CA 2,296 with four documented ignores; the feature-gated experiment
  target 118 with one documented ignore; CLI 38; role-run 17; ca-watch 15 plus
  three CLI tests; codex-team 647 with one documented ignore plus all integration
  targets; protocol 12; and every remaining workspace target. No protected
  source was changed, and no installation occurred.

## 2026-08-28 ACP-213 Stable ACP Interoperability

Focused validation:

- `cargo test acp::tests --bin ca -- --test-threads=1`: PASS (nine tests).
  Official SDK clients cover stable initialization, new/load/prompt/cancel and
  mode changes, permission allow/deny/unavailable paths, streamed updates,
  absolute contained one-based locations, and disconnect cleanup. Raw newline
  fixtures cover malformed JSON-RPC and clean EOF.
- `cargo test service::tests --bin ca -- --test-threads=1`: PASS (24 tests).
  ACP uses a crate-private orchestration bridge without changing the released
  `ca.serve.v1` wire.
- The dependency-unification regression suite passes. Explicit sorted JSON
  compatibility serialization retains the prior request, mechanistic range,
  learned-command, runtime-log, and portable-session byte contracts despite
  the official SDK enabling `serde_json/preserve_order`.

Child completion validation:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS. The CA
  default target passed 2,255 tests with four documented ignores; every
  workspace and integration target passed.
- `cargo test --workspace --all-targets --all-features --no-fail-fast --
  --test-threads=1`: PASS. The CA all-feature target passed 2,284 tests with
  four documented ignores; every workspace and integration target passed.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo build --workspace --release
  --all-features`: PASS. Root `clippy.toml` keeps `result_large_err` active with
  a 256-byte threshold above the unified dependency's 200-byte result type;
  protected codex-team source is unchanged.
- Release help/version/ACP-help and one credentialless stable-v1 initialize/EOF
  stdio smoke pass.
- `git diff --check`, zero `.snap.new`, exact protected-status comparison to
  the CUR-204 baseline, and `target/release/codex-team verify protected-trees
  --repo . --json`: PASS.

No user configuration, session, provider, installed binary, oracle, reference,
or protected-tree source changed. No network or hosted provider check ran.

## 2026-08-28 OBS-211 Runtime Logs and Watchdogs

Focused validation:

- Runtime diagnostics tests pass for the content-free schema, private bounded
  rotation, symlink/hardlink rejection, typed filtering, malformed-record
  containment, process-local path hashing, fake-clock heartbeat/render/flicker
  detection, restoration failures, and corroborated unclean-exit memory
  classification.
- Command, lifecycle, and render tests pass for `/logs`, hidden `/debug render`
  controls, cleanup-after-restoration-failure, and four manually reviewed
  100x30/60x24 log-viewer/debug-overlay snapshots.

Child completion validation:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS. CA passed
  2,230 tests with four documented ignores; CLI 38, role-run 17, ca-watch 15
  library plus three CLI, codex-team 647 with one documented ignore plus 16
  integration tests, protocol 12, and doc tests all pass.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo build --workspace --release
  --all-features`: PASS. Release help/version smokes pass.
- `git diff --check`, zero `.snap.new`, exact protected-status comparison to
  the CUR-204 baseline, and `target/release/codex-team verify protected-trees
  --repo . --json`: PASS.

The first complete run found the expected command-palette count change and an
unrelated protected codex-team five-client file-creation race. The reviewed
snapshot changed only 97 to 98 commands; the broker case passed immediately in
isolation and the next full serial workspace run passed. No runtime profile,
provider, installed binary, oracle, reference, or protected-tree path changed.

## 2026-08-28 HED-210 Rich Launch and Worktree Controls

Focused validation:

- `cargo test worktree -- --test-threads=1`: PASS (36 tests). This includes
  registered creation/status/cleanup, launch primary-root replacement,
  supplemental-root preservation, setup parsing/execution, session/workflow
  attachment, input behavior, and reviewed render states.
- `cargo test worktree_setup::tests -- --test-threads=1`: PASS (eight tests).
  Native precedence, compatibility defaults, no-config behavior, malformed,
  traversal, count, symlink, size and drift rejection, successful execution,
  and failed-tree retention pass.
- Focused mode, sandbox/trust, repository parser, structured warning/tool
  profile, headless digest, and TUI input tests pass. Six 100x30/60x24
  setup-preview, busy, and failure snapshots were manually reviewed and
  accepted.

Child completion validation:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS. CA passed
  2,216 tests with four documented ignores; CLI 38, role-run 17, ca-watch 15
  library plus three CLI, codex-team 647 with one documented ignore plus 16
  integration tests, protocol 12, and doc tests all pass.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo build --workspace --release
  --all-features`: PASS. Release version/help and `repo worktree-setup`
  help/JSON preview smokes pass.
- `git diff --check`, zero `.snap.new`, exact protected-status comparison to
  the CUR-204 baseline, and `target/release/codex-team verify protected-trees
  --repo . --json`: PASS.

The setup TUI is asynchronous and Cancel-first; interactive exit cancels only
the setup task and keeps the registered worktree. No user worktree, runtime
profile, trust/config/session state, provider, installed binary, oracle, or
reference file was changed.

## 2026-08-28 ENV-209 Direnv and Shell Integration

Focused validation:

- `cargo test run_environment::tests -- --test-threads=1`: PASS (eight tests).
  Coverage includes redacted Debug output, child propagation/removal, bounded
  parsing, null values, re-evaluation merge behavior, primary-workspace binding,
  digest drift, and symlink rejection.
- `cargo test shell_integration::tests -- --test-threads=1`: PASS (four tests).
  Exact managed-block generation, idempotent installation, mode-0600 recovery
  copies, exact-block uninstall, malformed markers, and symlink refusal pass.
- `cargo test direnv_ -- --test-threads=1`: PASS (three focused CLI/TUI tests).
  All 203 TUI render tests pass, including reviewed 100x30 and 60x24 direnv
  preview states. No `.snap.new` remains.

Child completion validation:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS. CA passed
  2,198 tests with four documented ignores; CLI 38, role-run 17, ca-watch 15
  library plus three CLI, codex-team 647 with one documented ignore plus 16
  integration tests, protocol 12, and doc tests all pass.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo build --workspace --release
  --all-features`: PASS. Release help/version, shell generator/help, and bare
  `--direnv` rejection smokes pass.
- `git diff --check`, zero `.snap.new`, exact protected-status comparison to
  the CUR-204 baseline, and `target/release/codex-team verify protected-trees
  --repo . --json`: PASS.

No real `.zshrc`, runtime profile, provider, session, installed binary, oracle,
or reference state was changed.

## 2026-08-28 PLG-208 Plugin UX and Ephemeral Loading

Focused validation:

- `cargo test plugin -- --test-threads=1`: PASS. Twenty-one plugin/catalog,
  CLI, parser, command, and render tests cover process-only non-persistence,
  manifest ambiguity, canonical-root duplication, symlink/bare rejection,
  contribution collisions, drift, signed staging/update/rollback, manager
  preview/confirmation, and the existing end-to-end local plugin lifecycle.
- The 100x30 and 60x24 plugin manager snapshots plus the command-palette count
  delta were manually reviewed. The complete 202-snapshot render suite passes
  after accepting only those expected deltas; no `.snap.new` remains.

Child completion validation:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS on the
  clean completion run. CA passed 2,182 tests with four documented ignores;
  CLI 38, role-run 17, ca-watch 15 library plus three CLI, codex-team 647 with
  one documented ignore plus 16 integration tests, and protocol 12 all pass.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo build --workspace --release
  --all-features`: PASS. Release `--help` exposes repeatable `--plugin-dir` and
  `ca plugin --help` preserves the verified local lifecycle.
- `git diff --check`, zero `.snap.new`, exact all-path protected-status
  comparison to the CUR-204 baseline, and `target/release/codex-team verify
  protected-trees --repo . --json`: PASS.

The first full run had one unchanged active agent-lifecycle capability-handle
timing failure. Its exact isolated rerun passed immediately, and the subsequent
full serial workspace run passed. No live plugin store, profile, installed
binary, oracle, or reference state was changed.

## 2026-08-28 JOB-207 Background-Job Lifecycle

Focused validation:

- 27 job tests pass for origin separation, owner scoping, foreground promotion,
  timeout-boundary races, output preservation, direct-job exclusion, automatic
  one-time claim, explicit-await reservation, cancellation, and reaping.
- Headless completion, batching, `max_turns`, `--single-turn`, parser, and TUI
  two-press timing tests pass. Model-originated jobs re-enter once; direct
  `!&` work does not spend a model turn.

Child completion validation:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS on the
  final clean run. CA passed 2,175 tests with four documented ignores; CLI 38,
  role-run 17, ca-watch 15 library plus three CLI, codex-team 647 with one
  documented ignore plus 16 integration tests, and protocol 12 all pass.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo build --workspace --release
  --all-features`: PASS. Release `--help` exposes `--single-turn`.
- `git diff --check`, zero `.snap.new` files, exact protected-status comparison
  with the CUR-204 baseline, and `target/release/codex-team verify
  protected-trees --repo . --json`: PASS.

The first two full runs exposed different nondeterministic failures in the
pre-existing dirty `codex-team` subtree: the stateful clean-filter test and a
five-client broker cancellation test. Each passed on its exact isolated rerun;
neither protected file was edited, and the third full serial run passed both.
No live profile, provider, session, installed binary, oracle, or reference state
was changed.

## 2026-08-28 KNL-206 Conditional Rules and Command Management

Focused validation:

- Rule tests pass for legacy/always/glob/manual activation, explicit-path-only
  matching, owning-root isolation, stable user/all-workspace discovery,
  malformed metadata/globs, bounds, symlinks, and manual selection.
- Mutation and manager tests pass for atomic create/edit/delete, exact preview
  confirmation, source drift, hardlinks/symlinks, native-root confinement,
  built-in precedence, read-only compatibility sources, saved-session manual
  IDs, legacy session defaults, and prompt inclusion.
- Prompt-command, project-instruction, and command-palette suites pass. The new
  100x30 and 60x24 rule/command manager snapshots and palette delta were
  manually reviewed; no `.snap.new` remains.

Child completion validation:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS on the clean
  completion run. CA passed 2,164 tests with four documented ignores; CLI
  passed 38, role-run 17, ca-watch 15 library plus three CLI tests, codex-team
  647 with one documented ignore plus 16 integration tests, and protocol 12.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo build --workspace --release
  --all-features`: PASS. Debug linking emitted the known macOS oversized
  `__eh_frame` compact-unwind warning; the optimized build passed.
- `git diff --check`, zero `.snap.new` files, direct protected-tree status, and
  `target/release/codex-team verify protected-trees --repo . --json`: PASS. The
  verifier reports `oracles/ and reference/ are unchanged`.

An initial full run exposed the legacy `Rule: name` heading expectation; the
renderer was corrected and its focused regression passed. A later intermediate
full run had one unchanged five-client `codex-team` MCP timing failure; that
exact test passed immediately in isolation and the subsequent full workspace
run passed. No live profile, session, provider, installed binary, oracle, or
reference state was changed.

## 2026-08-28 WSP-205 Named Multi-Root Workspaces

Focused validation:

- Catalog tests pass for exact schema and limits, atomic owner-only storage,
  root ordering/deduplication, name/path ambiguity, corrupt or insecure files,
  missing/non-directory roots, and symlink aliases.
- Workspace, launch-parser, resume-conflict, bare-profile, named-command,
  prompt-command, instruction/rule, skill, and registry propagation tests pass.
- The TUI command suite passes 48 tests. New deterministic wide/narrow named
  workspace catalog snapshots and the two-command palette delta were manually
  reviewed and pass without pending `.snap.new` files.

Child completion validation:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS. CA passed
  2,152 tests with four documented ignores; CLI passed 38, role-run 17,
  ca-watch 15 library plus three CLI tests, codex-team 647 with one documented
  ignore plus 16 integration tests, and protocol 12.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo build --workspace --release
  --all-features`: PASS. Debug linking emitted the known macOS oversized
  `__eh_frame` compact-unwind warning; the optimized build passed.
- `git diff --check`, zero `.snap.new` files, direct protected-tree status, and
  `target/release/codex-team verify protected-trees --repo . --json`: PASS. The
  verifier reports `oracles/ and reference/ are unchanged`.

No live profile, session, provider, credential, installed binary, oracle, or
reference state was read or mutated.

## 2026-08-28 HRS-203 Hermes Delegation and Learning Completion

Focused validation:

- `cargo test agent::subagent::manager_tests -- --nocapture`: PASS, 28 child
  contract, repair, batch, pause, worktree, spill, timeout, job-attribution,
  steering, authority, and lifecycle tests.
- `cargo test agent::lifecycle -- --nocapture`: PASS, four capability-handle,
  correlation-idempotency, retention, origin, and spawn-pause tests.
- `cargo test agent::registry::tests -- --nocapture`: PASS, 11 registry/schema
  tests covering the four additive manager tools and their closed arguments.
- `cargo test skill_curator -- --nocapture`: PASS, five private-state,
  transaction-recovery, policy, adoption, consolidation, and foreign-source
  tests.
- `cargo test learning_journey::tests -- --nocapture`: PASS, seven bounded
  ledger/graph tests, including scoped memory nodes whose rendered form omits
  memory bodies.
- The detached-origin re-entry regression and the captured main/continuation
  request-schema regression both pass. The latter was updated from 41 to 45
  profile tools after the four new manager controls were intentionally added.

Completion validation:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS on the
  clean rerun. CA passed 2,138 tests with four documented ignores; CLI passed
  38, role-run 17, ca-watch 15 library plus three CLI tests, codex-team 647
  with one documented ignore plus 16 integration tests, and protocol 12.
- `cargo test --workspace --all-targets --all-features --no-fail-fast --
  --test-threads=1` ran twice. Every HRS-203 test and target passed in both.
  The first run had one unchanged voice descendant-cleanup timing assertion;
  its exact test passed immediately, and the second run's complete CA target
  passed 2,167 tests with four documented ignores. The second run then hit one
  unchanged five-client broker cache-pruning race; that full eight-test target
  passed in the first run and the exact case passed immediately in 51.80s.
  The reasoning experiment passed 118 tests with one documented ignore and all
  other workspace targets passed in both runs.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo build --workspace --release
  --all-features`: PASS. Debug test linking emitted the known macOS oversized
  `__eh_frame` compact-unwind warning; optimized linking completed cleanly.
- Release help/version (`ca 0.1.0`), strict code-signature verification,
  `git diff --check`, zero `.snap.new` files, direct protected-tree status and
  diff, and `codex-team verify protected-trees --repo . --json`: PASS. The
  protected receipt reports `oracles/ and reference/ are unchanged`; release
  SHA-256 is
  `7e041539d00f57504645e3b911daeeb0509286f31a167fd054c8fb2377a71a1d`.

No live provider, credential, profile, session, user skill store, installed
binary, or public network service was read or mutated.

## 2026-08-28 HRS-202 Hermes-Informed Learning and Agent Lifecycle

Characterization and focused validation:

- The initial `cargo test --bin ca --no-run` characterization failed as
  intended because relationship metadata, `result_sha256`, and
  `missed_steer` did not exist. The same target compiled after implementation.
- `cargo test --bin ca skills::tests:: --no-fail-fast -- --test-threads=1`:
  PASS, 26 portable-skill parsing, validation, catalog, and authority tests.
- `cargo test --bin ca learning_journey::tests:: --no-fail-fast --
  --test-threads=1`: PASS, seven metadata-only ledger, suggestion, and graph
  tests.
- `cargo test --bin ca agent::subagent:: --no-fail-fast --
  --test-threads=1`: PASS, 20 lifecycle tests including approval-time steering,
  missed steering, bounded waits, and stable completed/failed digests.
- `cargo test --bin ca agent::registry::tests:: --no-fail-fast --
  --test-threads=1` and the TUI command suite: PASS, 11 registry and 48 command
  tests.

Completion validation:

- `cargo test --workspace --all-features --no-fail-fast --
  --test-threads=1`: PASS. CA passed 2,149 tests with four documented ignores;
  the reasoning experiment passed 118 with one documented ignore; CLI passed
  38, role-run 17, ca-watch 15 library plus three CLI tests, codex-team 647
  with one documented ignore plus 16 integration tests, and protocol 12.
- `cargo test --workspace --no-default-features --no-fail-fast --
  --test-threads=1`: PASS on the clean completion rerun. CA passed 2,120 tests
  with four documented ignores and all remaining targets repeated the passing
  counts above. The first matrix attempt had one timing-sensitive failure in
  codex-team's unchanged five-client stdio cache/cancellation stress test; the
  exact case passed alone in 64.74 seconds, and the full clean rerun passed it
  in 133.54 seconds without a code change.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo build --workspace --release
  --all-features`: PASS. Debug test linking emitted the known macOS oversized
  `__eh_frame` compact-unwind warning; optimized linking completed cleanly.
- Release help/version (`ca 0.1.0`), strict code-signature verification,
  `git diff --check`, direct protected-tree status/diff, zero `.snap.new`
  files, and Rust-owned snapshot/protected-tree verifiers: PASS. The protected
  receipt reports `oracles/ and reference/ are unchanged`.

No live provider, credential, profile, session, ledger, oracle, installed
binary, or public network service was read or mutated.

## 2026-08-28 PRV-201 DeepSeek Cache-Usage Compatibility

Characterization and focused validation:

- The new byte-split Chat SSE regression failed against the old decoder with
  `unexpected OpenAI cache details for hit_miss contract`; the resolved
  DeepSeek Responses fixture and provider-contract case also failed because
  Responses received `HitMiss`. All three pass after the protocol split.
- `cargo test -p ca deepseek -- --nocapture`: PASS, 16 DeepSeek decoder,
  continuation, contract, doctor, render, routing, and exact-pricing tests.
- `cargo test -p ca provider_contract_verifier::tests:: -- --nocapture`: PASS,
  four provider-contract verifier tests.
- `cargo test -p ca cache_usage -- --nocapture`: PASS, three canonical usage
  and cache-pricing tests.
- The explicit Chat regression covers null, numeric, malformed, and
  contradictory supplemental OpenAI details while retaining reported hit/miss
  accounting and unknown writes. Existing incomplete-pair, inconsistent-total,
  malformed-required-field, and oversized-counter failures remain covered.

Completion validation:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS. CA passed
  2,115 tests with four documented ignores, CLI passed 38, role-run passed 17,
  ca-watch passed 15 library plus three CLI tests, codex-team passed 647 with
  one documented ignore plus 16 integration tests, and protocol passed 12.
  The PTY used to launch this matrix inherited an open stdin; closing only that
  test process's stdin allowed its deliberate `read_line` fixture to proceed.
- `cargo test --workspace --all-targets --all-features --no-fail-fast --
  --test-threads=1`: PASS. CA passed 2,144 tests with four documented ignores,
  the reasoning experiment passed 118 with one documented ignore, and all
  remaining workspace targets repeated the passing counts above.
- `cargo fmt --all -- --check`, strict workspace all-target/all-feature
  Clippy, and `cargo build --workspace --release --all-features`: PASS. Debug
  linking emitted the known macOS oversized `__eh_frame` compact-unwind
  warning; the optimized build completed cleanly.
- Release help/version, strict signature verification, `git diff --check`,
  zero pending `.snap.new` files, direct protected-tree status/diff, and
  `codex-team verify protected-trees --json`: PASS. The protected receipt says
  `oracles/ and reference/ are unchanged`.

Installation and live validation:

- Candidate, staged, and installed SHA-256 values match at
  `82c72a53a0092c1a274a92b550d5790d530fef39515247bd525144ec1fd603ea`.
  Installed metadata remains a single regular executable owned by `aji:staff`;
  strict signature, lookup, help, and version checks pass.
- The pre-install SHA-256 was
  `e68c2bfb434c3c9ad936b12f3e523666cd1000e35a7bca9d2dabf418197df870`.
  Its verified recovery copy is
  `/Users/aji/.local/bin/ca.backup-prv201-20260828-005217-e68c2bfb`.
- The authorized command
  `ca --doctor --connect --provider-contract deepseek` ran exactly once and
  passed `deepseek/deepseek-v4-pro` with exactly two requests, then reported
  `ready`. It produced no credential or response body and no configuration,
  session, trace, ledger, or protected-tree mutation.

## 2026-08-27 INT-194 Dependency-Aware Verification and Diagnosis

Focused implementation validation:

- `cargo test -p codex-team-protocol -- --test-threads=1`: PASS, six
  protocol/schema compatibility tests after regenerating the checked-in action
  schema for the optional full-escalation policy bit.
- `cargo test -p codex-team mechanistic:: --lib -- --test-threads=1`: PASS,
  17 dependency graph, transitive closure, ecosystem, policy escalation,
  diagnosis, cache binding, tracked-file safety, and non-signalling observation
  tests.
- `cargo test -p ca mechanistic_ -- --test-threads=1`: PASS, 24 facade,
  configuration, session checkpoint, event coordinator, evidence ledger,
  quality-history, schedule, and exclusion tests.
- `cargo test -p ca undo:: -- --test-threads=1`: PASS, six tests including
  multi-file prevalidation, atomic undo, and ordered redo.
- `cargo test -p codex-team --test mcp_sdk -- --test-threads=1`: PASS, six real
  stdio tests preserving exact observe/delegate/operate catalogs and zero
  resources.
- Focused CA MCP compatibility and Main-only tool-selection tests: PASS. The
  debug link emitted the known macOS compact-unwind size warning; no focused
  test failed.

Completion validation:

- The single serial workspace run reached every target. Its only failure was
  the command-discovery snapshot's stale command count after adding
  `/evidence`: CA reported 2,091 passing tests, one snapshot failure, and two
  ignores; codex-team passed 595 tests with one external-binary ignore, all 13
  integration tests passed, and all six protocol tests passed. Both affected
  wide/narrow snapshots were updated from 90 to 91 commands and the exact
  failed snapshot test then passed. Unaffected targets were not rerun.
- `cargo fmt --all -- --check`, strict workspace all-target/all-feature
  Clippy, and `cargo build --workspace --release --all-features`: PASS.
  Clippy's first completion pass found three local style-only findings in the
  new graph code; those were corrected before the clean strict pass.
- Release help/version, `git diff --check`, snapshot hygiene, committed
  protected-tree verification, and direct protected-tree Git status/diff:
  PASS. Neither protected root was modified.
- Production verification isolation: PASS as
  `macos-seatbelt-source-projection-v1` without a debug override. Overall
  doctor remains unhealthy only because the monorepo root is intentionally not
  initialized as a governed codex-team repository.
- Real release CA MCP compatibility: PASS for exact 11/15/28 catalogs, zero
  resources, repository-pinned operation, and clean shutdown. No live provider
  traffic occurred.

No installation, profile mutation, commit, reset, stash, or protected-tree
edit is part of INT-194.

## 2026-08-27 INT-193 Automatic Preflight and Governed Verification

Focused validation completed during implementation:

- `cargo test -p codex-team-protocol`: PASS, six protocol/schema tests.
- `cargo test -p codex-team mechanistic:: --lib -- --test-threads=1`: PASS,
  twelve composite query, changed-file selection, cache-binding, typed recipe,
  tracked-file safety, and non-signalling observation tests.
- Codex-team MCP server tests: PASS, 27 tests including exact 11/15/28
  catalogs, revision-bound queries, durable jobs, cancellation, and shutdown.
- Real stdio MCP SDK: PASS, six tests with zero advertised resources.
- Deterministic CA intent, bounded/hash-only event coordination, exact mutation
  path capture, configuration defaults, model-free Schedule V2, Main-only
  facade selection, and 42+17 role-run exclusion tests: PASS.
- Learned manifest lifecycle: PASS, four focused tests for inert executable
  members, shadow thresholds, exact approval, quarantine/retirement, and human
  negative evidence.

Completion validation:

- The one broad serial workspace run reached every target. CA passed 2,085
  tests with two ignores, plus 38 CLI and 17 role-run CLI tests; ca-watch passed
  15 library and three CLI tests; all 13 codex-team integration tests and all
  six protocol tests passed. The codex-team library initially passed 588 tests
  with one ignore and reported two failures. One was a real strict-schema
  embedding regression and was fixed; the clean-filter fixture passed on its
  immediate rerun. The complete failed library target then passed 590 tests
  with one external-binary ignore. Passing workspace targets were not rerun.
- Focused post-matrix checks passed for the 14-test release-readiness policy,
  self-contained strict/public schema embedding, MCP SDK negotiation, and the
  real 11/15/28 CA compatibility fixture. Dirty/untracked state and plan
  bookkeeping are informational; contract drift, protected/snapshot failure,
  source mutation during gates, and failed gates remain blocking.
- `cargo fmt --all -- --check`, strict workspace all-target/all-feature Clippy,
  `cargo build --workspace --release --all-features`, `git diff --check`,
  snapshot hygiene, committed protected-tree verification, release
  help/version, and direct protected-tree Git status/diff: PASS. Final
  codex-team-only edits received a second strict package Clippy and release
  rebuild.
- Production verification isolation: PASS as
  `macos-seatbelt-source-projection-v1` without a debug override. Overall
  doctor remains unhealthy only because the monorepo root is intentionally not
  initialized as a governed codex-team repository.
- Real release and installed-binary CA MCP compatibility: PASS for exact
  11/15/28 catalogs, zero resources, repository-relative regex query,
  structured rejection, and clean shutdown. Reverse release-CA role-run
  compatibility passed against a loopback fake provider; no live provider was
  contacted.

The validated binaries were installed atomically. Candidate and installed
SHA-256 values match:
`ca=e68c2bfb434c3c9ad936b12f3e523666cd1000e35a7bca9d2dabf418197df870`
and
`codex-team=841eff1f53dd98372d014e0f42416ec9f00503f30427d3bf856a2a5432cb881a`.
Prior binaries are recoverable at
`/Users/aji/.local/bin/ca.backup-int193-20260827-153936` and
`/Users/aji/.local/bin/codex-team.backup-int193-20260827-153936`, whose hashes
match the pre-install artifacts. Installed help/version passed. The named
profile remains byte-identical at SHA-256
`2bc81df31bf284208e417fa34240016c6d0e240a5759e9b4f80f90cdb8b74a96`
and mode 0600.

Installed TUI `/delegate` reported enabled, delegate catalog, 15 tools, zero
resources, and the pinned revision. `/jobs` reported no shell jobs, watchers,
or delegations. Both controls and shutdown completed without a provider
request, session write, or token accounting.

## 2026-08-27 INT-192 Live Mechanistic Delegation Hardening

Focused coverage proves legacy literal search round trips, explicit regex,
tracked exact-file and component-prefix scopes, deduplication, traversal and
missing-scope rejection, symlink/hardlink/untracked/secret/binary/size bounds,
output caps, and revision drift. CA coverage distinguishes argument, remote
tool, protocol, and transport failures; preserves bounded redacted structured
details; and renders non-success receipts as failed without authorizing
fallback.

Retry coverage preserves one canonical fully resolved batch containing
successful delegation, successful mutation, and denied/failed tool results;
excludes the subsequent partial assistant text; retains provider/model choice;
leaves the source session immutable; and works in memory under `--no-session`.
Pre-tool provider failure retains whole-turn replay. The existing central tool
selection test reconfirms absence from Plan, review, checkpoint, and subagent
profiles, while scheduled, service, bare, and role-run retain MCP-disabled or
explicitly disabled capability construction.

Focused validation completed before the one-time release matrix:

- `cargo test -p codex-team-protocol`: PASS, four tests.
- `cargo test -p codex-team mechanistic::query::tests --lib`: PASS, five tests.
- Codex-team MCP structured/stale/invalid regression: PASS.
- CA invalid-arguments, typed MCP error, facade-truthfulness, tool-selection,
  and retry suites: PASS.
- Explicit real stdio CA fixture: PASS for exact 11/15/28 catalogs, zero
  resources, separate CA/pinned-repository roots, relative prefix regex search,
  structured invalid-regex failure, and clean shutdown.

Completion and release validation:

- The serial workspace command passed 2,080 CA tests with two ignores, 38 CLI,
  17 role-run, 15 ca-watch plus three watcher CLI, all 13 codex-team integration
  tests, and four protocol tests. One codex-team library assertion still
  expected generic `REPO_QUERY_FAILED` for revision drift; after changing only
  that expectation to `STALE_REVISION`, the failed target passed all 586
  runnable tests with one external-binary ignore. Unaffected passing targets
  were not needlessly rerun.
- `cargo fmt --all -- --check`: PASS after applying its local formatting
  suggestions.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  PASS after replacing one needless double-ended `.last()` with `.next_back()`.
- `cargo build --workspace --release --all-features`: PASS.
- Release help/version, `git diff --check`, `.snap.new` hygiene, committed
  protected-tree verification, and direct protected-tree Git status/diff:
  PASS.
- Production verification isolation: PASS as
  `macos-seatbelt-source-projection-v1`; overall codex-team doctor remains
  unhealthy only because this repository is intentionally uninitialized.
- Exact release CA MCP fixture: PASS for 11/15/28 catalogs, zero resources,
  separate CA/pinned roots, regex query, structured rejection, and clean
  shutdown. Reverse release-CA role-run compatibility: PASS.

Validated releases were installed atomically. Installed and candidate hashes
match at
`ca=c37240f55d320e02e1bf66d8968134ac2b61914d910f3ab0ce20d7f7bf588061`
and
`codex-team=aba99855744d39d838e77a897457df7e1560f69912c8c8b23d4035b129efc479`.
The prior binaries are recoverable at
`/Users/aji/.local/bin/ca.backup-int192-20260827-001414-b966d7fd` and
`/Users/aji/.local/bin/codex-team.backup-int192-20260827-001414-770a000f`.
Strict signature verification and installed help/version pass. The named
profile remains byte-identical with SHA-256
`2bc81df31bf284208e417fa34240016c6d0e240a5759e9b4f80f90cdb8b74a96`
and mode 0600.

Installed TUI `/delegate` reports enabled, delegate catalog, 15 tools, zero
resources, and no recent jobs; `/jobs` reports no shell, watcher, or delegation
activity; exit leaves no MCP child. Those two controls made no provider call.
A separate intended offline doctor smoke was mistyped as positional `doctor`
instead of `--doctor`; CA interpreted it as a headless prompt and made one
unintended live provider turn, completing two read-only mechanistic calls
before interruption with exit 130. It created no session file and changed no
repository/profile/config bytes. The exact `--doctor` command then passed
offline and started no MCP process. This procedural incident means the overall
manual validation session did not remain provider-free, even though the
specified model-free cockpit behavior passed.

## 2026-08-26 INT-191 Protected Baseline and Delegate Activation

The initial characterization proved the ignored protected roots could never
pass without a trusted content baseline. Focused tests now cover HEAD-only
manifest trust, malformed and stale manifests, exact preview/apply binding,
content/add/delete/rename/mode/symlink drift, traversal, hardlinks, special
files, large/binary streaming, race and size bounds, and zero protected-root
mutation. The real committed receipt passes with `oracles/` and `reference/`
unchanged.

The arrow regression was reproduced under both the slash palette and `@file`
picker. Left/Right plus native Option/Command motion now pass through those
inline overlays to the composer; path completion and Unicode boundaries remain
covered.

Validation:

- Focused protected-tree unit and CLI suites: PASS.
- Focused overlay/path cursor regressions: PASS, three tests; full
  `tui::input_tests`: PASS, 110 tests.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS — 2,072 CA
  unit tests with two documented ignores, 38 CLI tests, 17 role-run tests,
  15 ca-watch unit tests plus three CLI tests, 585 codex-team unit tests with
  one external-binary ignore, 13 codex-team integration tests, and three
  protocol tests.
- Formatting, strict workspace all-target/all-feature Clippy, optimized
  all-feature workspace build, `git diff --check`, help/version, and snapshot
  hygiene: PASS.
- `codex-team verify protected-trees --json`: PASS against committed baseline
  `1530d660916ecb8d6f5befa68d1a850e9912c674`.
- Production isolation: PASS as
  `macos-seatbelt-source-projection-v1`; overall doctor remains unhealthy only
  because this root intentionally has no `.orchestrator` initialization.
- Real release CA MCP fixture: PASS for exact 11/15/28 catalogs and zero
  resources. Reverse release-CA role-run fixture: PASS.
- Publication release readiness still reports the pre-existing dirty/untracked
  source as audit findings. Local activation is no longer gated on clean Git;
  exact candidate hashes, protected-tree verification, backups, and installed
  compatibility are authoritative for that operation.
- Installed CA and codex-team hashes match their validated release artifacts:
  `b966d7fd02a5f97a58f0a42ec99b08d98ec8dbd1a4d3054bdf7786810b1a98f9`
  and `770a000fb296be68732278cc06fa1e3157491de7bf24e5396bd6bf07f6a502d9`.
- Named-profile doctor: PASS without starting MCP. Real TUI `/delegate`: PASS,
  exact delegate catalog 15 and resources 0. `/jobs`: PASS, unified empty
  delegation state. Exit reaped the stdio server. These controls made no
  provider request.
- Installed-executable sandbox and uninitialized repository-status focused
  regressions: PASS; strict package Clippy passes for both changed packages.
- Automatic-selection regression: PASS. The enabled Main-profile facade tells
  the model to prefer codex-team for eligible mechanical work without an
  explicit user request, before ordinary exploration or agent spawning, and
  permits fallback only when the receipt does.
- Host-owned facade version regression: PASS. The model-visible schema requires
  only `actions`, hides the version discriminator, accepts the exact action-only
  `search_text` shape observed in the failed live attempt, preserves exact
  legacy-version input, and rejects an incorrect legacy version. The real
  installed-codex-team 11/15/28 fixture passes with an action-only facade call.

Original installed binaries remain recoverable at
`/Users/aji/.local/bin/ca.backup-int191-20260826-da46aaeb` and
`/Users/aji/.local/bin/codex-team.backup-int191-20260826-62578fd4`. The named
profile is mode `0600`; the default profile, credentials, services, and both
protected trees remain unchanged.

## 2026-08-26 INT-190 CA ↔ Codex-Team Mechanistic Cooperation

Focused coverage proves protocol/schema round trips, config default-off and
bounds, local/network/catalog refusal, lazy server startup, raw-tool search
hiding, first-turn Main selection, excluded surfaces, query/job batch
classification, bounded current-session activity, schema-only durable-ID
restore, wide/narrow `/jobs` rendering, model-free cancel/feedback/correction,
all typed recipe/query/observation safety, exact MCP tiers, and combined
codex-team query-context/Rust-skip/exactly-one-CA edit routing.

Validation:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS — 2,070 CA
  library tests with two documented ignores, 38 CLI tests, 17 role-run tests,
  15 ca-watch library tests, three ca-watch CLI tests, 580 codex-team library
  tests with one documented external-binary ignore, all eleven codex-team
  integration tests, and three protocol tests.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- The explicitly invoked real release-binary CA fixture: PASS for exact
  observe/delegate/operate catalogs of 11/15/28 tools, zero resources, and the
  facade query flow.
- The explicitly invoked release `ca role-run` compatibility fixture: PASS for
  offline preflight, fixed read tools, exact allowed Worker writes, and denied
  outside-scope writes.
- Release `--help`/`--version`: PASS for `ca 0.1.0`, `ca-watch 0.1.0`, and
  `codex-team 0.1.0`.
- Production release doctor isolation subcheck: PASS,
  `macos-seatbelt-source-projection-v1`, without the debug override. Doctor's
  overall exit one is expected because `.orchestrator` is intentionally absent
  from this root; no initialization was performed.
- `git diff --check`, direct protected-tree Git status/diff, and built-in
  snapshot hygiene: PASS. The built-in protected-tree verifier fails closed
  only because ignored `oracles/` and `reference/` have no trusted tracked
  content baseline; direct Git inspection reports no changes there.

The command-palette count snapshots were reviewed from 89 to 90 for the new
`/delegate` command. Four new delegation list/detail snapshots cover 60- and
100-column layouts, and two existing narrow `/jobs` snapshots reflect the new
refresh control. No live provider/network operation, runtime-state mutation,
binary installation, oracle edit, reference edit, or implementation commit
occurred.

## 2026-08-26 TUI-189 Truthful Model Catalog Status

The initial configured-Codex, provider-scoped Codex, documented-fallback, and
model-less-provider regressions failed against the old classification. A
four-provider regression now proves two live catalogs plus direct OpenAI and
OpenAI Codex fallbacks produce `4 models · 2 live · 2 fallback`, with sanitized
detail on each provider row and no double-counted unavailable category. A
failed provider without configured or documented models remains named as
unavailable.

Validation:

- Focused new and existing catalog/fallback tests: PASS, including the exact
  mixed-provider and provider-scoped cases.
- `cargo test -p ca model_catalog -- --test-threads=1`: PASS, 11 matching tests.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,061 CA
  tests, 38 CLI tests, 17 role-run tests, 15 watcher tests, three watcher CLI
  tests, 556 embedded scheduler tests, and all integration/doc targets; three
  documented manual/external checks ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- Release list-models/help/version, strict signature, hash, `git diff --check`,
  `.snap.new` hygiene, and protected `oracles/`/`reference/` status: PASS.

No snapshot update was required. Validation made no provider request and
changed no runtime configuration, credential, session, model, protocol, oracle,
or reference state. The verified release was installed at
`/Users/aji/.local/bin/ca` with SHA-256
`da46aaeb4715debabe3e271c361be3a9d12c81c1ad751098bfc632bab3a03c51`;
the previous binary is recoverable at
`/Users/aji/.local/bin/ca.backup-tui189-20260826` with SHA-256
`0f457421a0fd1ca1bf5a2d4bcc8bfb091f02f97fe606f00b9f3ec8c7bd886b22`.

## 2026-08-26 TUI-188 Collapsible Edit and Command Activity

The initial focused characterization failed because activity presentation and
grouping did not exist. It now covers registry classification, two-call and
boundary rules, recorded turn separation, completed and partial calls, exact
live mutation receipts, conservative restored labels, Unicode paths, complete
sanitized command wrapping, error/running state, Ctrl+O, current/older/scrolled
mouse targets, clicks outside headers, stale-target clearing, pager isolation,
drag-selection precedence, copy-all preservation, and expanded-to-collapsed
redraw cleanup.

Validation:

- `cargo test -p ca activity_ -- --test-threads=1`: PASS, 19 matching tests.
- `cargo test -p ca tui::render_tests -- --test-threads=1`: PASS, 197 tests.
- `cargo test -p ca tui::input_tests -- --test-threads=1`: PASS, 107 tests.
- `cargo test -p ca tui::transcript::tests -- --test-threads=1`: PASS, eight tests.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,055 CA
  tests, 38 CA CLI tests, 17 role-run tests, 15 watcher tests, three watcher CLI
  tests, 556 embedded scheduler tests, and all integration/doc targets; three
  documented manual/external checks ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- Release `--help`/`--version` for `ca 0.1.0` and `ca-watch 0.1.0`: PASS.
- `git diff --check`, `.snap.new` hygiene, and protected `oracles/`/`reference/`
  status: PASS.

Nine TUI-188 snapshots at 60, 100, and 184 columns were reviewed individually
against the supplied visual guidance without a bulk snapshot refresh. No live
network/provider request, runtime profile/session mutation, binary
installation, oracle write, or reference write occurred during validation.
After a separate explicit user request, the validated release was installed at
`/Users/aji/.local/bin/ca`; its SHA-256 matches `target/release/ca` at
`0f457421a0fd1ca1bf5a2d4bcc8bfb091f02f97fe606f00b9f3ec8c7bd886b22`.
The prior binary is recoverable at
`/Users/aji/.local/bin/ca.backup-tui188-20260826` with SHA-256
`b1e029ff2a87d691f1eabd55a6a5960480ce0969096508d5571a58f0c7545591`;
strict macOS signature verification, absolute/lookup version, and help pass.

## 2026-08-26 ASY-187 Dedicated `ca-watch` and Token-Free Monitoring

The crate suite covers local liveness, marker-first completion, incomplete-copy
failure, identity drift, bounded log tails, cancellation ordering, transport
degradation/recovery, capped backoff, versioned JSON, validation, fixed SSH
arguments/quoted values, safe errors, text/JSONL CLI output, invalid exit two,
terminal failure exit one, and Ctrl+C exit 130 without target termination. Fake
transport tests perform zero live network operations.

CA coverage verifies Main-profile-only typed schemas, stable owner-scoped IDs,
local/remote workspace capability preparation, exact approval identity across
destination/PID/kind/path/cadence changes, schedule/service/headless exclusion,
background versus foreground timeout behavior, redraw-only watcher delivery,
and unchanged provider-call/message/token/usage counters across 100 events.
Snapshots cover wide/narrow activity strips, managed jobs plus local/remote PID
watches, watcher details, bounded progress, and completion/failure toasts;
input coverage proves watcher rows cannot open stdin and stop observation only.

Validation:

- `cargo test -p ca-watch --no-fail-fast -- --test-threads=1`: PASS, 15 library
  tests and three CLI integration tests.
- `cargo test -p ca tui::render_tests -- --test-threads=1`: PASS, 185 tests.
- `cargo test -p ca tui::input_tests -- --test-threads=1`: PASS, 107 tests.
- Focused registry, manager, tool, approval, workspace, schedule, timeout, and
  provider/token-isolation regressions: PASS.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,040 CA
  tests, 38 CA CLI tests, 17 role-run tests, 15 watcher library tests, three
  watcher CLI tests, 556 embedded scheduler tests, and all integration/doc
  targets; three documented manual/external tests ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- Release `--help` and `--version` for `ca 0.1.0` and `ca-watch 0.1.0`: PASS.
- `git diff --check`, `.snap.new` hygiene, and protected `oracles/`/`reference/`
  status: PASS.

The new and affected activity/job snapshots were reviewed individually without
a bulk snapshot refresh. Live SSH/BSD execution remains intentionally blocked
by scope; fixed fake-transport tests establish the remote invocation contract.
No runtime profile/session, installed binary, oracle, or reference state was
changed.

## 2026-08-26 TUI-186 Adaptive Tool-Output Layout

The initial four pure-layout regressions failed with an empty characterization
stub. They now cover stable nine-column TSV detection, Unicode/display-cell
bounds, numeric alignment, empty cells, eight-cell tab fallback, inconsistent
rows, narrow fallback, collapsed omission accounting, and control sanitization.
Render coverage adds 60-, 100-, and 184-column cards, collapsed and Ctrl+O
expanded snapshots, complete right borders, and expanded-to-collapsed cleanup
beside later Markdown.

Validation:

- `cargo test -p ca tui::output_layout::tests -- --test-threads=1`: PASS, five
  tests.
- `cargo test -p ca tui::render_tests -- --test-threads=1`: PASS, 182 tests.
- `cargo test -p ca command_output -- --test-threads=1`: PASS, six tests.
- `cargo test -p ca terminal_text -- --test-threads=1`: PASS, two tests.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,030 CA
  tests, 38 CLI tests, 17 role-run tests, 556 embedded scheduler tests, and all
  integration/doc targets; three documented manual/external tests ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- Focused `git diff --check` and `.snap.new` hygiene: PASS.

The new and directly affected output-card snapshots were reviewed without a
bulk snapshot refresh. Validation changed no canonical tool output, generated
TSV, provider/session data, runtime configuration, installed binary, oracle,
or reference state and made no network request.

## 2026-08-26 REL-185 Hermes/MiMo-Style Interactive Approval

The initially failing regressions cover the complete reported workflow:
`interactive_yolo_includes_network_in_the_command_decision` and
`interactive_network_command_respects_one_explicit_ask_and_deny` cover one
combined interactive decision; `approval_prompt_shows_the_complete_command_at_every_width`
covers the unclipped command tail; `remote_absolute_path_in_prose_never_becomes_a_slash_command_or_blocks_submit`
covers the exact destination sentence; and
`terminal_job_statuses_map_to_typed_tool_outcomes` covers nonterminal polling.

Validation:

- `cargo test approval --no-fail-fast -- --test-threads=1`: PASS, 84 tests
  across the workspace, including approval snapshots and queued reclassification.
- `cargo test tui::input_tests --no-fail-fast -- --test-threads=1`: PASS, 106
  composer tests, including native Option/Command editing and path completion.
- Exact network, read-only SSH/deferred-order, complete-command, and job-status
  regressions: PASS.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,022 CA
  tests, 38 CLI tests, 17 role-run tests, 556 embedded scheduler tests, and all
  integration/doc targets; three documented manual/external tests ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- `git diff --check`, `.snap.new` hygiene, and protected-tree status: PASS.

The completion gates made no live network/provider request, runtime
profile/session mutation, installed-binary replacement, or oracle/reference
write. Hermes and MiMo were used only as read-only behavioral evidence; no
Cursor parity claim is made.

After a separate explicit user request, the validated release was installed at
`/Users/aji/.local/bin/ca`. Its SHA-256 equals the repository release at
`b1e029ff2a87d691f1eabd55a6a5960480ce0969096508d5571a58f0c7545591`;
strict macOS signature verification, absolute and normal-path `--version`, and
`--help` pass. The previous binary (SHA-256
`3aa2d455caf2ec1e22302a154964dd80a4d750469b94e29e1c579162d852c56a`)
is recoverable at `/Users/aji/.local/bin/ca.backup-rel185-20260826`.

## 2026-08-26 TUI-184 Native macOS Composer Editing

The initially failing `option_backspace_deletes_the_previous_unicode_word_atomically`
and `command_backspace_deletes_to_the_start_of_the_current_hard_line`
regressions cover Unicode boundaries, kill-ring text, hard-line locality, and
atomic large-paste deletion. `option_and_command_horizontal_arrows_use_native_mac_boundaries`
and `native_mac_editing_keys_keep_working_while_path_completion_is_open` cover
word/line motion and the prior completion-popup interception path. Keymap and
terminal lifecycle tests cover shortcut reservation, Command modifier
preservation, balanced enhanced-keyboard setup/cleanup, and unsupported-mode
fallback.

Validation:

- Initial `cargo test -p ca backspace_deletes -- --test-threads=1`: expected
  FAIL before implementation; Option+Backspace was intercepted by Remove Last
  Attachment and Command+Backspace was unhandled.
- `cargo test -p ca tui::input_tests -- --test-threads=1`: PASS, 105 tests.
- `cargo test -p ca keymap::tests -- --test-threads=1`: PASS, six tests.
- `cargo test -p ca tui::event::lifecycle_tests -- --test-threads=1`: PASS,
  ten tests.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,023 CA
  tests, 38 CLI tests, 17 role-run tests, 556 embedded scheduler tests, and all
  integration/doc targets; three documented manual/external tests ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.

Validation changed no runtime profile/session, installed binary, oracle, or
reference state. Command shortcuts require a terminal that reports the
Command modifier; enhanced-keyboard setup is deliberately best-effort.

## 2026-08-26 REL-183 Request-Scoped Read-Only SSH Authority

The user-reported two-call `root@bsd` approval queue is covered by
`reported_read_only_ssh_batch_skips_redundant_network_queue`; its local
pre-execution hook proves both calls bypass approval without launching SSH.
`deferred_read_only_ssh_keeps_request_scoped_network_authority` proves an
earlier interaction cannot strip the later ordered call's sandbox network
authority. Pure classifier and broker tests cover exact host binding, direct
request extraction, attachment exclusion, AutoReview/RunEverything versus
Allowlist/no-live-turn behavior, explicit Ask/Deny precedence, and rejection of
transfer, forwarding, alternate config, redirects, substitution, sequencing,
backgrounding, output files, mutation-capable `find`/`sort`, `tee`, unknown
programs, and missing remote commands.

Validation:

- Initial `cargo test -p ca request_scoped_ssh -- --test-threads=1`: expected
  FAIL before implementation because the request classifier and
  authority-aware broker entry point did not exist.
- `cargo test -p ca permission::tests -- --test-threads=1`: PASS, 16 tests.
- `cargo test -p ca approval::tests -- --test-threads=1`: PASS, 49 tests.
- `cargo test -p ca workspace::tests -- --test-threads=1`: PASS, seven tests.
- `cargo test -p ca ssh -- --test-threads=1`: PASS, seven tests.
- Focused reported/deferred batch, broker, no-authority network, and positive
  classifier tests: PASS.
- `cargo test -p ca auto_review -- --test-threads=1`: PASS, nine CA tests and
  one CLI test.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,019 CA
  tests, 38 CLI tests, 17 role-run tests, 556 embedded scheduler tests, and all
  integration/doc targets; three documented manual/external tests ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- `git diff --check`, `.snap.new` hygiene, and protected-tree status: PASS.

No live SSH/provider request, runtime profile/session mutation, binary
installation, or oracle/reference write occurred. The supplied screenshot is
the behavior report; no Cursor oracle parity claim is made.

## 2026-08-26 TUI-182 Prose-Safe Path Completion Editing

The user-reported `red/ops` failure is covered by
`prose_slashes_do_not_open_path_completion_or_block_cursor_motion`.
`inline_path_completion_requires_explicit_path_syntax` separates explicit
paths from slash-separated prose, and
`horizontal_arrows_move_the_cursor_while_path_completion_is_open` preserves
editing without redundant discovery work. All three failed before the source
correction and pass afterward.

Validation:

- `cargo test --bin ca path_completion -- --test-threads=1`: PASS, seven tests.
- `cargo test --bin ca tui::input_tests -- --test-threads=1`: PASS, 101 tests.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,631
  tests across CA, CLI, role-run, embedded `codex-team`, integration, and doc
  targets; three documented manual/external tests ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- Changed-file `git diff --check`, `.snap.new` hygiene, and protected-tree
  status: PASS.

No runtime profile, provider, installed binary, oracle, or reference state was
changed. The screenshot supplied by the user is the behavior report; no Cursor
oracle parity claim is made.

## 2026-08-25 SKL-181 Session-Automatic Skill Activation

Focused validation:

- `cargo test automatic --no-fail-fast`: PASS after implementation; covers
  interactive, RequireApproval headless, and service activation without a
  generic prompt. The three new contract tests failed before implementation.
- `cargo test skill --no-fail-fast`: PASS, 54 CA unit tests, two CA CLI tests,
  and one embedded `codex-team` test.
- `cargo test headless --no-fail-fast`: PASS, 39 CA unit tests and six CA CLI
  tests.
- `cargo test service --no-fail-fast`: PASS, 30 CA unit tests.
- `cargo test session --no-fail-fast`: PASS when run serially, 185 CA unit
  tests and seven embedded `codex-team` tests. A concurrent first run produced
  five unrelated process/socket timing failures; each exact test and the full
  serial session-filter rerun passed.
- `cargo fmt --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --release`: PASS.

The first serial all-feature workspace run passed 2,034 CA tests, all CLI,
role-run, experiment, embedded `codex-team`, integration, and doc-test targets,
with the expected three ignored manual tests. Its only failure was an older
AutoReview assertion that still expected generic approval for
`activate_skill`; the assertion was reconciled with SKL-181 and its exact
rerun passed. Final `cargo test --workspace --no-fail-fast --
--test-threads=1`: PASS with 2,006 CA unit tests, 38 CLI tests, 17 role-run
tests, 556 embedded `codex-team` tests, and all integration/doc-test targets;
the expected three manual tests were ignored. No provider traffic,
runtime-profile/session mutation, binary installation, plugin installation,
or oracle/reference write occurred.

## 2026-08-25 TUI-180 Quiet Skill Discovery

Focused validation:

- `cargo test skill_suggestions_never_replace_the_idle_status_row
  --no-fail-fast`: PASS.
- `INSTA_UPDATE=always cargo test
  skill_suggestion_composer_and_ranked_pager_snapshots --no-fail-fast`: PASS;
  only the two composer snapshots changed, while ranked `/skills` snapshots
  remained unchanged.
- `cargo test skill --no-fail-fast`: PASS, 53 CA unit tests, two CA CLI tests,
  and one embedded `codex-team` test.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.

The full `cargo test --workspace --no-fail-fast -- --test-threads=1` run passed
2,004 CA tests, all 38 CLI tests, all 17 role-run tests, 556 embedded
`codex-team` tests, and all integration/doc-test targets. One unrelated CA test,
`voice::tests::cancelling_local_transcription_kills_descendants`, failed its
timing assertion; an immediate exact serial rerun passed. No provider traffic,
runtime-profile mutation, binary installation, or oracle/reference write
occurred.

## 2026-08-25 LRN-179 Cross-Session Recall and Skill Operations

Focused coverage proves lazy all-history CA/Codex/Hermes/MiMo recall,
deterministic redaction and paging, session deduplication with retained source
provenance, isolated malformed sources, and query-only foreign SQLite access.
The Hermes fixture exceeds the old per-session learning cap and remains fully
searchable without WAL/SHM creation. Independent response bounds cover search
excerpts, safe action summaries, message chunks, and aggregate tool output.

Skill/config coverage exercises every platform, tool, toolset, fallback, and
required-environment condition; workspace-over-profile-over-default settings;
credential-like key/value rejection; unavailable/setup-required discovery; and
owner-only workspace settings. Bundle coverage exercises two-to-five-member
limits, collision/ambiguity/drift refusal, atomic exact-digest activation,
shared token budgeting, read-only foreign bundles, and prepared-journal
recovery. Ledger coverage proves idempotency, concurrent writers, stale-lock
recovery, exact digest attribution, metadata-only storage, lifecycle mutation
links, and all three deterministic suggestion thresholds. Eight wide/narrow
snapshots cover `/recall`, unavailable/config-required skills, bundle
diagnostics, and `/learning`.

Completion gates:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS. CA passed
  2,005 active unit tests with two documented manual tests ignored; 38 CLI and
  17 role-run tests passed. Embedded `codex-team` passed 556 active tests with
  one documented external-binary test ignored, plus its integration suites.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- `git diff --check`, zero pending `.snap.new` files, and no `oracles/` or
  `reference/` modifications: PASS.

Validation used temporary fixtures and local build artifacts only. It made no
provider call, read no credential, ran no live `/learn`, mutated no real
profile, installed no binary, and wrote no oracle/reference content.

## 2026-08-25 LRN-177 Resilient Learning Finalization

The exact live failure is covered by
`adaptive_learning_drops_extraneous_unknown_non_authorizing_cluster`: a valid
accumulated submission plus all missing cluster decisions and one extraneous
unknown skip now succeeds, and the bogus ID enters neither the final batch nor
coverage. Separate regressions prove unknown IDs remain invalid on authorizing
created/extended decisions, correction schemas expose only the missing stable
IDs, and catalogs above 512 retain the bounded fallback schema. The final live
receipt also added `coverage_source_summary_includes_hermes_and_omits_zero_counts`.

Focused results:

- `cargo test -p ca learning::tests -- --test-threads=1`: PASS, 98 tests.
- `cargo test -p ca skills::tests`: PASS, 21 tests.
- `cargo test -p ca prompt_commands::tests`: PASS, 5 tests.
- `cargo test -p ca foreign_knowledge::tests`: PASS, 3 tests.
- Exact direct-receipt source-summary regression: PASS.

Live acceptance used the repository-built binary and bare `/learn` exactly once.
Run `020c3875-ef34-4a0e-b034-51708fe22b0a` completed in four adaptive turns and
20 bounded retrieval steps with 120,418 input plus 5,503 output tokens. It made
all 189 sessions / 1,591 messages searchable (CA 115, Codex 872, Hermes 4,
MiMo 600), retrieved 207 messages / 66 sessions, cited 26 support sessions,
delivered both catalog pages, explicitly decided 118/118 clusters, and retired
1,591 messages only after commit. It published no duplicate asset because the
three recurring BSD workflows already exist as discoverable MiMo commands. Its
undo command is `/learn undo 020c3875-ef34-4a0e-b034-51708fe22b0a`.

Completion gates:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS on the final
  clean run, 2,599 passed and 3 documented manual/external tests ignored. The
  preceding attempt had one unrelated timing-sensitive `codex-team` clean-filter
  failure; the exact test and the subsequent complete serial rerun passed.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- Release `--version`, `--help`, and `--doctor`: PASS (`ca 0.1.0`, Ready).
- `git diff --check` and `.snap.new` hygiene: PASS.

The completion run itself performed no provider retry, learning retry,
oracle/reference write, release installation, or dependency/plugin/service
installation. After explicit user authorization, the same validated release was
installed at `/Users/aji/.local/bin/ca`; ordinary `ca` lookup, `--version`,
`--help`, offline `--doctor` (`Ready`), SHA-256
`5474464afa6dadf5c2e6497481ff31c72a62c2895d334bc35ec3a5d841c9d415`, and
strict macOS code-signature verification all pass. The prior binary remains at
`/Users/aji/.local/bin/ca.backup-lrn177-20260825T135550Z`. Installation
verification made no learning or provider call.

## 2026-08-25 LRN-176 Foreign Knowledge Interoperability

Codex, Hermes, and MiMo reusable knowledge now has direct read-only product
coverage. Focused results:

- `cargo test foreign_knowledge::tests --no-fail-fast`: PASS, 3 tests.
- `cargo test prompt_commands::tests --no-fail-fast`: PASS, 5 tests.
- `cargo test memory::tests --no-fail-fast`: PASS, 13 tests.
- `cargo test skills::tests --no-fail-fast`: PASS, 21 tests.
- `cargo test codex_learning::tests --no-fail-fast`: PASS, 65 tests.

The Hermes SQLite fixture proves completed main work retains bounded requests,
final conclusions, safe action shapes, permission errors, successful recovery,
and stopping conditions while excluding raw tool output, reasoning, email
activity, secrets, subagent sessions, and incomplete turns. It also proves no
WAL/SHM creation and byte-for-byte database stability. Foreign-memory fixtures
cover hostile instruction and secret removal; skill/command fixtures cover
read-only ownership, full-tree validation, literal arguments, registry
availability, and built-in collision refusal.

Read-only checks against the existing machine discovered 81 structurally
compatible Hermes skills, 22 bundled MiMo skills, and one Codex skill with no
duplicate instance. The one skipped Hermes tree contains binary PDF templates,
which CA deliberately rejects; an equivalent MiMo-bundled
`research-paper-writing` skill remains available. A local-only preview of the
real Hermes database found one completed main session and four candidate
messages with zero provider calls and zero profile writes.

Completion gates:

- `cargo test`: PASS, 2,593 tests passed and 3 ignored across the workspace.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.

No live provider or `/learn` run was invoked, no release binary was installed,
and no foreign/runtime/profile state was modified.

## 2026-08-24 LRN-174 MiMo End-to-End Learning and Dream Repair

- The complete MiMo Distill operator and prompt sources were reviewed read-only.
  CA now gives the adaptive agent ephemeral sanitized session titles, ranked
  broad-term retrieval across titles and safe message projections, bounded
  session inspection, and the existing strict final submission tool. Titles and
  transcript text are never added to persistent learning state.
- `direct_first_learn_discovers_publishes_memory_and_command_then_undoes_both`
  proves a first bare `/learn` automatically discovers two native sessions,
  searches them, finishes in two provider turns, publishes both workspace memory
  and a native `.ca/commands/check-rsync.md`, renders the compact receipt, and
  removes both publications with one undo. No sync, review, digest approval, or
  second invocation is used.
- `learning_sanitizer_is_byte_bounded_and_idempotent_for_unicode` reproduces the
  supplied Dream failure shape. New sanitized evidence is bounded by UTF-8 bytes,
  is idempotent, and cannot produce the previous unchanged 8,234-byte record.
- `supervised_dream_queue_accepts_byte_bounded_multibyte_evidence_end_to_end`
  additionally builds the multibyte corpus, queues it through the enrolled
  background-maintenance bridge, reloads/validates it, and exports it intact.
- `cargo test --bin ca learning::tests -- --test-threads=1`: PASS, 79 tests.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,567
  tests with three documented manual/external ignores (`ca` 1,946, CLI 38,
  role CLI 17, `codex-team` 556, integrations 10).
- `cargo test --workspace --all-targets --all-features --no-fail-fast --
  --test-threads=1`: PASS, 2,714 tests with four documented manual/external
  ignores (`ca` 1,975, experiment 118, CLI 38, role CLI 17, `codex-team` 556,
  integrations 10).
- `cargo fmt --all -- --check`, strict workspace/all-target/all-feature Clippy,
  the all-feature release build, and release `ca` version/help plus `codex-team`
  help smokes: PASS. The recurring macOS debug-linker compact-unwind warning is
  unchanged.
- Validation used fake providers and temporary profiles/workspaces. It did not
  contact a live provider, install the release binary, mutate the user's runtime
  profile, or modify `oracles/` or `reference/`.

## 2026-08-24 LRN-173 First Direct Learning Run Repair

- `first_direct_run_keeps_review_created_by_standard_source_sync`: reproduced
  the supplied `evidence planning: no evidence is pending review` failure before
  the repair and passes afterward.
- `cargo test --bin ca codex_learning::tests -- --test-threads=1`: PASS, 50
  tests.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,562
  tests with three documented manual/external ignores (`ca` 1,941, CLI 38,
  role CLI 17, `codex-team` 556, integrations 10).
- `cargo fmt --all -- --check`, strict workspace/all-target/all-feature Clippy,
  the all-feature release build, and release help/version smokes: PASS.
- The installed user binary and real learning profile were not changed; no live
  provider or publication run was started.

## 2026-08-24 LRN-172 Full Command-Operator Examples

- Exact filtered `/learn` and `/distill` usage regression: PASS; complete
  operator strings are present at the supplied wide layout.
- Command-browser wide/narrow snapshots: PASS; long review, workspace, goal,
  trace, and description rows wrap without layout-generated ellipses.
- `cargo test --bin ca tui::render_tests -- --test-threads=1`: PASS, 177 tests.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,561
  tests with three documented manual/external ignores (`ca` 1,940, CLI 38,
  role CLI 17, `codex-team` 556, integrations 10).
- `cargo fmt --all -- --check`, strict workspace/all-target/all-feature Clippy,
  the all-feature release build, release help/version smokes,
  `git diff --check`, `.snap.new` hygiene, and protected-tree status: PASS.
- No runtime profile, installed binary, provider, oracle, or reference state
  was changed.

## 2026-08-24 LRN-171 Direct Adaptive MiMo-Style Learning

- Archive-learning focused suite: 49 passed, including native CA bundle
  ingestion, automatic native discovery, unsupported-sibling tolerance, a
  valid rollout above the legacy 64 MiB ceiling, and native email/incomplete
  exclusion. Existing adaptive/archive, prompt-command, direct-command, and
  reviewed wide/narrow transcript coverage remains passing.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,560
  tests with three documented manual/external ignores (`ca` 1,939, CLI 38,
  role CLI 17, `codex-team` 556, integrations 10).
- `cargo test --workspace --all-targets --all-features --no-fail-fast` with
  `--test-threads=1`: PASS, 2,707 tests with four documented manual/external
  ignores (`ca` 1,968, experiment 118, CLI 38, role CLI 17, `codex-team` 556,
  integrations 10).
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- `cargo build --workspace --release --all-features`: passed; release `ca`
  version/help and `codex-team` help smokes passed.
- `git diff --check`, `.snap.new` hygiene, and protected `oracles/`/`reference/`
  status: passed.
- Optimized read-only preview against the actual standard roots: PASS in 3.91
  seconds, 151 files / 1,063,142,977 bytes / 1,632 completed candidate
  messages, six secret-shaped skips, zero provider calls, and zero profile
  writes.
- Live provider quality and real-profile publication were intentionally not run;
  fake backends and temporary roots cover learning/publication behavior.

## 2026-08-24 LRN-170 MiMoCode-Style Learning and Distillation

Synthetic native-session and query-only SQLite fixtures prove empty/`main`
main-agent selection, completed-turn action-window extraction, recent-first
bounded selection with older corroboration, richer fork deduplication, and
safe command/path/query projection. Tool outputs, email activity, secret-shaped
inputs, source/content bodies, incomplete turns, subagents, private paths, and
raw transcripts remain absent from provider requests or persisted indexes.

- `cargo test --bin ca codex_learning::tests --no-fail-fast`: PASS, 38 tests.
- `cargo test --bin ca learning::tests --no-fail-fast`: PASS, 56 matching
  learning/archive tests.
- `cargo test --bin ca learning_review_ --no-fail-fast`: PASS, three tests.
- Reviewed archive-learning pager snapshot: PASS.
- `cargo test --workspace --no-fail-fast`: PASS, 2,536 tests with three
  documented ignores.
- `cargo test --workspace --all-targets --all-features --no-fail-fast`: PASS,
  2,684 tests with four documented ignores.
- `cargo fmt --all -- --check`, strict workspace/all-target/all-feature Clippy,
  `cargo build --workspace --release --all-features`, `git diff --check`,
  release `ca`/`codex-team` help and version smokes, snapshot hygiene, and
  protected-tree status: PASS.

Fake providers prove exactly one combined learning request, asset inventory,
full bounded workflow evidence, strict structured validation, zero-call resume
after a completed checkpoint, and both creation and zero-creation receipts.
Validation did not contact a live provider, read credentials, mutate the real
learning/session/config profile, activate a service, install a binary, or
modify `oracles/` or `reference/`. The recurring macOS debug-linker
compact-unwind warning is unchanged.

## 2026-08-24 LRN-169 Zero-Jitter TUI, Copyable Sessions, and Streamlined Learning

Focused fake-provider, state-machine, rendering, clipboard, and selection
coverage proves one style-only final-line streaming cursor; stable bottom and
scrolled-up anchors across blink, delta, wrap, Markdown, and resize frames;
exact `/learn approve` progress and missing-call-only checkpoint resume; strict
checkpoint drift refusal and cleanup; non-empty last-response and privacy-safe
whole-session copying; forward/reverse cross-scroll Unicode selection; and
successful byte-changing edit progress.

- `cargo test --bin ca loop_guard --no-fail-fast`: PASS, 16 tests.
- `cargo test --bin ca streaming_ --no-fail-fast`: PASS, six tests.
- `cargo test --bin ca codex_learning::tests --no-fail-fast`: PASS, 34 tests.
- `cargo test --bin ca copy_ --no-fail-fast`: PASS, eight tests.
- `cargo test --bin ca tui::transcript::tests --no-fail-fast`: PASS, seven
  tests; focused mouse-event, keymap, command, progress, and provider-fallback
  snapshot regressions also pass.
- `cargo test --workspace --no-fail-fast`: PASS, 2,532 tests with three
  documented ignores (`ca` 1,911 + two ignored, CLI 38, role-run CLI 17,
  `codex-team` 556 + one ignored, integrations 10).
- `cargo test --workspace --all-targets --all-features --no-fail-fast`: PASS,
  2,679 tests with four documented ignores (`ca` 1,940 + two ignored,
  experiment 118 + one ignored, CLI 38, role-run CLI 17, `codex-team` 556 +
  one ignored, integrations 10).
- `cargo fmt --all -- --check`, strict workspace/all-target/all-feature Clippy,
  all-feature release build, release `ca`/`codex-team` help and version smokes,
  `git diff --check`, snapshot hygiene, and protected-tree status: PASS.

A rebuilt, uninstalled 100x30 native PTY used a disposable home/workspace and
credentialless literal-loopback provider. The screenshot response wrapped to
four stable transcript rows. During a five-second delayed follow-up, repeated
thinking/blink ticks emitted no writes to those transcript rows; the later
streaming cursor updated only its one final-line cell. The TUI restored the
terminal and exited cleanly. The exact temporary root was moved recoverably to
`/Users/aji/.Trash/ca-lrn169-native.P39GcH`.

Validation did not contact a live provider, read credentials, mutate real
learning/session/config state, install a binary, or modify `oracles/` or
`reference/`. The recurring macOS debug-linker compact-unwind warning is
unchanged.

## 2026-08-23 LRN-168 Guided, Copyable Learning Review

Temporary archive sources and inert proposal fixtures cover exact evidence and
proposal bindings, automatic registered-source refresh, route/cost display,
fresh digests after source drift, revoked approval, memory-only, skills-only,
all-scope application, partial digest renewal, and reject-all. Provider-backed
generation remains covered by the existing fake backend; no live route ran.

- `cargo test --bin ca codex_learning::tests --no-fail-fast`: PASS, 32 tests.
- `cargo test --bin ca learning_review_ --no-fail-fast`: PASS, three
  orchestration tests covering Later-first exact actions, busy suppression,
  stale generations, and closed-pager completion.
- `cargo test --bin ca tui::render_tests --no-fail-fast`: PASS, 170 tests.
- `cargo test --bin ca tui::input --no-fail-fast`: PASS, 175 tests, including
  forward/reverse multiline Unicode pager selection, complete off-screen Copy
  All, retained selection on clipboard failure, and contextual Enter/R/close.
- Reviewed learning status/evidence/proposal snapshots pass at wide and narrow
  sizes; no `.snap.new` file remains.
- `cargo test --workspace --no-fail-fast -- --quiet`: PASS, 2,517 tests with
  three manual-only ignores (`ca` 1,896 + two ignored, CLI 38, role-run CLI 17,
  `codex-team` 556 + one ignored, integrations 10).
- `cargo fmt --all -- --check`, strict workspace/all-target/all-feature Clippy,
  all-feature release build, `git diff --check`, snapshot hygiene, and
  oracle/reference status: PASS.

Validation used temporary state and build artifacts only. It did not contact a
live provider, read credentials, mutate the real learning profile, register a
source, activate the LaunchAgent, install a binary, or modify protected trees.

## 2026-08-23 LRN-167 Manual MiMoCode Session Learning

Focused synthetic SQLite coverage passes for completed main-slice user/final
assistant selection, incomplete/error/tool-step/non-main rejection,
reasoning/tool/synthetic/ignored filtering, secret removal, owner-controlled
read-only access, transcript-free persisted state, fork deduplication, and
preservation of an existing Codex index when MiMo is registered.

- `cargo test --bin ca codex_learning::tests`: PASS, 32 tests.
- `cargo test --bin ca tui::render_tests::reviewed_archive_learning_pager_snapshots`:
  PASS with the reviewed generic “Session evidence learning” heading.
- Preview-only live-schema smoke against
  `~/.local/share/mimocode/mimocode.db`: PASS, 114 sessions, 567 completed
  candidate messages, two secret-shaped skips, zero provider calls, and zero
  profile writes. No transcript text was printed.
- `cargo test --workspace --no-fail-fast`: PASS, 2,510 tests with three
  manual-only ignores (`ca` 1,889 + two ignored, CLI 38, role-run CLI 17,
  `codex-team` 556 + one ignored, integrations 10).
- `cargo fmt --all -- --check`, strict workspace/all-target/all-feature Clippy,
  all-feature release build, release help/version, `git diff --check`, snapshot
  hygiene, and oracle/reference status: PASS.

No provider was contacted, no profile learning state or ordinary saved session
was changed, no service/binary was installed, and oracle/reference trees were
read only. `launchctl` confirms the learning LaunchAgent remains unloaded.

## 2026-08-23 SES-165 Understandable Session Names

Focused coverage proves bounded request-subject extraction, provider-output
cleanup, legacy-title detection, process-local stale-result rejection,
tool-free request recording/accounting, exact archived-session repair, projected
Ctrl+G targeting, poor-name guidance, and updated wide/narrow picker snapshots.
All provider behavior used a one-shot loopback fake; all persistence used
temporary active/archive stores.

- `cargo test title_ --no-fail-fast`: PASS, including isolated request kind,
  usage outcome, cleanup, and stale-result checks.
- `cargo test session_picker_tests --no-fail-fast`: PASS, 13 picker input tests.
- `cargo test tui::render_tests::session_ --no-fail-fast`: PASS, 14 session
  render tests with reviewed wide/narrow snapshots.
- Exact archived title generation/persistence: PASS against a fake provider and
  temporary UUID-checked archive store.
- `cargo test --workspace --no-fail-fast -- --quiet`: PASS, 2,492 tests with
  three ignored (`ca` 1,873 + two ignored, CLI 38, role-run CLI 17,
  `codex-team` 554 + one ignored, integrations 10).
- `cargo fmt --all -- --check`, strict workspace/all-target Clippy, release
  workspace build, release help/version smokes, diff/snapshot hygiene: PASS.

No live provider, credential, real saved session/configuration, installed
binary, oracle, or reference content was accessed or changed. The recurring
macOS debug-linker compact-unwind warning is unchanged.

## 2026-08-23 SES-164 Named Session Browser

Focused characterizations cover explicit `My request:` title extraction,
Unicode/control bounds, exact active/archive rename persistence, projected-row
rename input, empty-name refusal, orchestration result application, hidden
current-session behavior, hidden metadata, date grouping, local-offset times,
and wide/narrow browser/editor snapshots.

- `cargo test session_ --no-fail-fast`: PASS, 107 matching `ca` tests plus
  seven matching `codex-team` tests.
- `cargo test tui::render_tests --no-fail-fast`: the first run exposed one
  accidental timestamp edit in an unrelated agent-activity fixture; after
  restoring that byte, its exact regression passed and the final workspace run
  passed every render snapshot.
- `cargo test --workspace --no-fail-fast -- --quiet`: PASS, 2,484 tests with
  three ignored (`ca` 1,865 + two ignored, CLI 38, role-run CLI 17,
  `codex-team` 554 + one ignored, integrations 10).
- `cargo fmt --all -- --check`, `git diff --check`, strict workspace/all-target
  Clippy, release workspace build, release help/version smokes, and absence of
  `.snap.new`: PASS.

The updated wide/narrow picker and rename snapshots were reviewed against the
user-provided example. Validation used in-memory state, temporary session
stores, and build artifacts only; it did not contact a provider, access
credentials, rewrite real sessions/configuration, install a binary, or modify
oracle/reference trees.

## 2026-08-23 SES-163 Primary Sessions Browser

Failing characterizations first confirmed that `/sessions` was unknown, the
current saved session rendered and received actions, and only-current history
opened an empty picker. After implementation:

- Command registry and discovery: PASS for canonical `/sessions`, compatible
  `/resume` and `/continue`, separate `/session`, and common-command priority.
- Session orchestration and picker input: PASS for direct/no-argument routing,
  only-current and archive-only states, hidden-current filtering/navigation,
  and resume/pin/archive/delete/restore action mapping.
- Render coverage: PASS for all 12 session render tests and reviewed wide/narrow
  command-discovery, command-palette, and former-session picker snapshots.
- `cargo test session_ -- --quiet`: PASS, 110 matching workspace tests.
- `cargo test --workspace --no-fail-fast -- --quiet`: PASS, 2,478 tests with
  three ignored (`ca` 1,859 + 2 ignored, CLI 38, role-run CLI 17,
  `codex-team` 554 + 1 ignored, integrations 10).
- `cargo fmt --all -- --check`, `git diff --check`, strict workspace/all-target
  Clippy, release workspace build, release help/version smokes, and absence of
  `.snap.new`: PASS.

Validation used only in-memory state, temporary session stores, and build
artifacts. It did not access real sessions/configuration/credentials, contact
a provider, install a binary, or modify oracle/reference trees.

## 2026-08-23 REL-162 Low-Friction Bounded Autonomy

The screenshot regressions were first confirmed as failures for external typed
reads, AutoReview local execution and job control, child default scopes,
agent-name guidance, and provider timeout defaults. After implementation, the
focused permission, workspace, registry, provider, child, main/headless,
role-run, PDF/image external-read, and job sandbox tests pass. The native macOS
job test proves external reads, workspace writes, and conventional `/tmp`
writes succeed while a sibling external write and private privilege-prefix
access remain denied.

Completion results on the final source:

- `cargo test --workspace --no-fail-fast -- --quiet`: PASS. `ca` main passed
  1,853 tests with two ignored, CLI passed 38, role-run CLI passed 17,
  `codex-team` passed 554 with one ignored, and its ten integration tests
  passed: 2,472 passed and three ignored overall.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets -- -D warnings`: PASS.
- `cargo build --workspace --release`: PASS.
- Release `ca` and `codex-team` help/version smokes: PASS at 0.1.0.
- `git diff --check`: PASS.
- Snapshot hygiene and protected `oracles/`/`reference/` status: PASS.

The recurring macOS debug-linker `__eh_frame` compact-unwind warning remains
unchanged. Validation used disposable local fixtures only; it did not contact a
provider, access credentials, modify runtime configuration or saved sessions,
install a binary, or write protected oracle/reference trees.

## 2026-08-23 Status Review and SEC-158 M1

All sudo/password execution coverage was hermetic. It used temporary fake
helpers and sentinel secrets only; validation did not execute real sudo, ask
for or accept a real password, contact a provider, mutate user runtime state,
install a binary, or write `oracles/`/`reference/`.

Focused results:

- `cargo test --bin ca privilege::tests -- --nocapture`: PASS, 20 tests,
  including exact direct askpass order, minimal environment, separate target
  stdin, NOPASSWD behavior, FIFO modes, cwd identity drift, password
  rejection, timeouts, cleanup latch, leader descendants, cancellation, and a
  sudo-like PTY monitor whose target enters a separate session.
- `cargo test --bin ca sandbox::tests -- --nocapture`: PASS, 10 tests. The
  private FIFO directory and its `/private/tmp` alias are denied after the
  system-wide root allow; target process inspection and network are denied.
- `cargo test --bin ca privilege_ -- --nocapture`: PASS, 10 tests across
  main-Agent eligibility, negative modes, headless/service/schedule closure,
  exact approval/order, hooks, and deterministic TUI snapshots.
- Masked host input 2/2, Markdown 17/17, agent graph 7/7, work receipts 15/15,
  graph tick 3/3, task graph 5/5, and release-readiness 14/14: PASS.

Completion results on the final source:

- `cargo test --workspace --no-fail-fast -- --quiet`: PASS. `ca` main 1,850
  passed/2 ignored, CLI 38, role-run CLI 17, `codex-team` library 554 passed/1
  ignored, and ten `codex-team` integration tests: 2,469 passed, 3 ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  PASS.
- `cargo build --workspace --release --all-features`: PASS. Released `ca` and
  `codex-team` help/version smokes passed at 0.1.0.
- The final `cargo test --workspace --all-targets --all-features
  --no-fail-fast -- --quiet` run passed `ca` main 1,879/2 ignored, experiment
  118/1 ignored, CLI 38, role-run 17, all ten integrations, and every
  privilege test. One unrelated `codex-team` library test failed because a
  stateful clean-filter counter observed one invocation instead of an exact
  expected two; its unchanged focused rerun passed 1/1, and two earlier full
  matrices passed. REL-160 tracks removal of that nondeterministic exact-count
  assumption. The complete matrix is therefore recorded as flaky, not green.
- The only recurring build diagnostic was the existing macOS linker warning
  that the debug `__eh_frame` section is too large for compact unwind offsets.

Two unchanged release-readiness audits completed deterministically with exit
1 and the same digest. They correctly reported not ready due active-plan
tracking consistency, dirty tracked/untracked release inputs, and untracked
workspace/release control files; static toolchain/workflow/snapshot/protected-
tree checks passed and expensive gates were skipped by design.

## SEC-155 Linux and FreeBSD Privilege Handoff

Historical milestone evidence. SEC-158 now requires the macOS enforcement
backend and fails closed on Linux/FreeBSD before approval or password input.

All execution coverage remains hermetic and uses the SEC-154 injectable
fake-sudo fixture. The only production-helper action was a later explicitly
authorized, non-interactive FreeBSD validation probe; no credential was used.

Results on macOS (2026-08-23):

- The new fixed-platform contract test first failed to compile because helper
  selection, native-format selection, and non-macOS sandbox gating did not yet
  exist; it passes after the portability implementation.
- `cargo fmt --all -- --check`: PASS.
- `cargo test privilege::tests --no-fail-fast`: PASS, 9 tests.
- Closed registry-schema and exact approval/order focused tests: PASS.
- `cargo test --workspace --no-fail-fast`: PASS; `ca` ran 1,817 unit tests
  (1 ignored), plus 38 CLI and 17 role-run tests, and all default
  `codex-team` suites passed.

FreeBSD validation was intentionally bounded. On the existing
15.1-RELEASE-p2 amd64 host, `/usr/local/bin/sudo` is root-owned mode 4755 ELF;
`/usr/bin/id` is root-owned ELF; and `/`, `/usr`, `/usr/local`, and
`/usr/local/bin` are root-owned mode 0755. With explicit user authorization,
`/usr/local/bin/sudo -n -v </dev/null` ran and was denied by sudo policy for
user `aji`; the subsequent `/usr/bin/id -u` target did not execute. No password
prompt, password input, or privileged mutation occurred. A locked workspace
compile stopped at a pre-existing `crates/codex-team` `setpriority` i32/u32
mismatch unrelated to SEC-155. The check was stopped rather than expanding
this task; exact disposable source directories were removed. Linux/FreeBSD
platform selection, mandatory-sandbox refusal, native-format recognition, and
all credential/execution behavior are covered by portable unit/fake-helper
tests.

## SEC-154 Safe Interactive macOS Privilege Handoff

Historical milestone evidence. SEC-158 replaces the `sudo -n` lifecycle below
with the direct askpass supervisor validated in the current section above.

All privilege execution tests use an injected fake-sudo executable and
temporary roots. Production validation only inspected the fixed helper's file
identity; it never executed real sudo or accepted a credential.

Completion results on macOS (2026-08-23):

- The initial registry characterization failed because `run_privileged` did
  not exist, as expected. The first complete run then exposed two fake-process
  PID-file observation races and one platform-conditional schema-count
  expectation; all three were corrected and reproduced cleanly.
- `cargo test privilege::tests --no-fail-fast`: PASS, 8 fake-helper,
  validation, nonce/revalidation, timeout/reaping, exact-argv, closed-stdin,
  and split-byte redaction tests.
- `cargo test privilege_ --no-fail-fast`: PASS, 7 approval/policy, hook,
  headless/service/schedule, and reviewed wide/narrow render tests.
- Focused masked-input, closed-schema, and prepared-request tests: PASS.
- `cargo test --workspace --no-fail-fast`: PASS; `ca` ran 1,816 unit tests
  plus 38 CLI and 17 role-run tests, and the embedded `codex-team` workspace
  suites passed. Three documented manual/pilot tests were ignored across the
  workspace.
- `cargo test --workspace --all-targets --all-features --no-fail-fast`: PASS;
  `ca` ran 1,845 main tests (1,843 passed, 2 ignored), 119 experiment tests
  (118 passed, 1 ignored), 38 CLI tests, and 17 role-run tests; all
  `codex-team` suites also passed.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features
  -- -D warnings`, and `cargo build --workspace --release --all-features`:
  PASS. The first lint run found one new needless `return`; it was removed and
  the strict rerun passed.
- Released `ca` and `codex-team` help/version smokes (`0.1.0`), `git diff
  --check`, absence of `.snap.new`, reviewed snapshot status, and protected
  `oracles/`/`reference/` worktree checks: PASS.

Coverage includes cached and password-required success, wrong password,
denial/cancellation, auth/command timeout, stale binding, nonce mismatch,
one-use replay, authorization expiry, command failure, process-group reaping,
secure executable/ancestry and interpreter rejection, exact argv with shell
metacharacters, closed target stdin, unchanged sandbox/network preparation,
secret echo on both streams across chunk boundaries, redacted hooks/results and
Debug surfaces, ordered mixed batches, and fail-closed unattended/delegated/
Plan/non-macOS surfaces. No provider traffic, runtime config/session mutation,
installed-binary replacement, credential access, or oracle/reference mutation
occurred.

## CON-153 One-Project Monorepo Consolidation

The canonical root is a Cargo workspace containing `ca` and
`crates/codex-team`. On 2026-08-22 the consolidated source passed:

- `cargo fmt --all --check`.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo test --workspace`: `ca` 1,797 unit tests plus 38 CLI and 17 role-run
  CLI tests; `codex-team` 540 library tests plus 1 CA-foundation, 1 manual
  commit, 5 MCP SDK, and 3 step-cycle tests. Three documented manual/pilot
  tests were ignored.
- `cargo build --workspace --release`.
- Release `ca`/`codex-team` help and version smokes, `git diff --check`, and
  absence of `.snap.new` files.

These gates used local/disposable fixtures only. No provider traffic,
credential access, runtime profile/session mutation, binary installation, or
oracle/reference mutation ran. Generated `target/` output was removed after
the gates passed; rebuild from the workspace root when new validation is
needed.

## OPT-152 Hosted API Context Efficiency

Deterministic coverage exercises the stable symbol-first/batched-read prefix,
canonical fact/hypothesis checkpoint contract, uncertainty preservation,
private cache-scope omission and no-hit disclaimer, exact-known versus unknown
cache math, latency sampling, tool-continuation counts, bounded byte stability,
eligible semantic model-result selection, raw interactive expansion, and
wide/narrow command-output, usage, provider-account, and session reports.

Completion results:

- Focused context, request, usage, and command-output module suites: PASS, 27,
  10, 27, and 6 tests; exact prefix and relevant render snapshots also PASS.
- `cargo test --no-fail-fast --quiet`: PASS, 1,731 main tests and 38 CLI tests;
  one ignored.
- `cargo test --all-targets --all-features --no-fail-fast --quiet`: PASS,
  1,760 main tests, 118 experiment tests, and 38 CLI tests; two ignored.
- `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D
  warnings`, `cargo build --release --all-features`, release help/version (`ca
  0.1.0`), `git diff --check`, snapshot hygiene, and oracle/reference worktree
  status: PASS.

No paid/live provider verification ran. Validation did not inspect credentials,
rewrite runtime configuration/sessions, install a binary, or read/modify
`oracles/`/`reference/` content.

## PRV-151 Model-Scoped Prompt Cache Contracts

Deterministic coverage exercises exact first-party/custom contract precedence,
proxy isolation, invalid declaration rejection before credentials, targeted
TOML preservation, rename/delete maintenance, explicit/provider-managed wire
fields, stable/rotating cache epochs, frozen tool continuations, fallback and
one-turn routes, side-question/subagent isolation, checkpoint serialization and
staging, legacy/tampered checkpoint digests, deterministic MCP catalogs, every
usage dialect/provenance case, all write-billing modes, catalog migration,
external JSON compatibility, and wide/narrow model-cache snapshots.

Completion results:

- Focused prompt-cache and pricing suites: PASS, 17 and 18 tests; checkpoint,
  MCP, usage, headless, service, configuration, lifecycle, and render suites
  also PASS.
- `cargo test --no-fail-fast`: PASS, 1,728 main tests and 38 CLI tests; one
  ignored.
- `cargo test --all-targets --all-features --no-fail-fast --quiet`: PASS,
  1,757 main tests, 118 experiment tests, and 38 CLI tests; two ignored.
- `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D
  warnings`, `cargo build --release --all-features`, release help/version (`ca
  0.1.0`), `git diff --check`, snapshot hygiene, and oracle/reference
  immutability: PASS.

No paid/live provider verification ran. Validation did not inspect credentials,
rewrite runtime configuration/sessions, install a binary, or modify
`oracles/`/`reference/`.

## LRN-166 Autonomous Learning Loop Completion

Validated on 2026-08-23 without installing the LaunchAgent or contacting a real
provider. Focused deterministic coverage proves:

- native default-profile active/archive sessions and Codex active/archive JSONL
  share bounded completed-turn harvesting, secret filtering, workspace
  partitioning, stable active/archive deduplication, queue backpressure, and the
  15-minute scan plus first-due 7/30-day schedules;
- imported evidence is not acknowledged on export/import and advances its exact
  cursor only after terminal replay rejection or successful publication;
- sorted-canonical completion digests interoperate across the process boundary,
  retry returns the same receipt, and exact Dream memory and inert Distill skill
  publication each retain their existing undo path;
- knowledge-only enrollment binds exact binaries, roots, default evidence
  profile, named execution routes, and unbounded metered policy without a clean
  repository or source/deployment authority;
- the generated execution profile retains only provider/model metadata and
  external credential references, forces required sandbox plus no approvals,
  and excludes literal secrets, MCP commands, hooks, and unrelated settings;
- only one RunAtLoad/KeepAlive supervisor plist is rendered and legacy
  `com.ca.learn.*` scanners are migrated without touching unrelated agents; and
- the process-level fake Codex/DeepSeek/MiMo pilot reaches one exact promoted
  skill and bridge completion without source or deployment mutation.

Completion results:

- `cargo test --workspace`: PASS, 2,500 passed and three manual-only ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- release `--help`, `--version`, `learn --help`, `learn service preview`, and
  `git diff --check`: PASS.
- read-only activation preview digest:
  `1aae1e531b984a353c8221a94bd14b8d8f7b87a6316dbd83e4fc35adde2e1bdd`.

The first confirmed install attempt changed nothing and exposed unordered
provider-map serialization across processes. After converting the isolated
profile document to ordered maps, the focused determinism regression, strict
lint, release rebuild, and two independent release previews all pass with the
same digest above.

The installed/loaded/nonzero-source/live-terminal-workflow smoke is pending the
one exact activation confirmation. This validation did not install a service or
profile, call a live provider, publish real memory/skills, read credential
values, or mutate session/oracle/reference data.

## LRN-150 Reviewed Codex Archive Learning

Focused deterministic coverage includes completed-turn/final-answer selection,
first-metadata identity, unknown/oversized records, terminal and secret
filtering, malformed/future records, symlink refusal, owner-only
state/lock, append-only incremental sync, same-size/same-mtime revision checks,
active/archive and parent/fork deduplication, source drift,
workspace partitioning plus cross-workspace user reduction, exact route/cost
digest binding, path-free provider prompts, inert proposals, exact second
review, individual proposal apply/reject, and application through the existing
learning journal. macOS tests cover deterministic no-shell LaunchAgent content,
fake-`launchctl` lifecycle,
and a required background sandbox with scrubbed credentials, denied network,
and no config/session access.

The real supplied roots were validated only with `ca learn source preview`.
An intermediate preview observed 89 files / 469,585,296 bytes; the final
preview observed 93 files / 504,570,420 bytes and 1,145 completed candidates
after four new live Codex session files appeared. It made zero provider calls
and profile writes, and profile state remained absent.

Completion results:

- Focused archive-learning suite: PASS, 22 tests.
- Reviewed TUI coverage: PASS, six new wide/narrow archive-learning snapshots
  plus the updated wide/narrow 83-command discovery count.
- `cargo test --no-fail-fast --quiet`: PASS, 1,695 main tests and 38 CLI tests;
  one ignored.
- `cargo test --all-targets --all-features --no-fail-fast --quiet`: PASS,
  1,724 main tests, 118 experiment tests, and 38 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy,
  `cargo build --release --all-features`, release help/version (`ca 0.1.0`),
  `git diff --check`, snapshot hygiene, and oracle/reference immutability: PASS.

Real source/service installation, provider processing, proposal application,
and installed-binary replacement remain blocked on separate exact approvals.

## REL-149 Reliable Tool Execution and Scoped Consent

Deterministic coverage reproduces all six reported paths: empty skill
registries omit activation/view tools and invalid selectors fail before
approval; 1 MiB-bounded contextual/multi-hunk patches apply atomically while
malformed patches preserve bytes and emit no receipt; no-LSP/extension-mismatch
edits are quiet; exact current-run grants cover later same-file calls but not another
file; required-sandbox Git traverses `/etc`; and the exact
`cargo fmt --all --check && cargo build` command resolves the installed Cargo
and builds a cached registry dependency offline.

Focused results:

- Unified patch, mutation receipt, and loop-guard suites: PASS.
- Permission/approval and wide/narrow approval render suites: PASS (74 approval tests).
- Sandbox/jobs native Git/Cargo, trusted executable, offline cache, literal
  file rule, inferred SSH network, and managed `read_lints` suites: PASS.
- Skills/tool-selection, LSP, service, headless-adjacent, and subagent suites:
  PASS; automatic read-only children do not receive `read_lints`.

Completion results:

- `cargo test --no-fail-fast`: PASS, 1,671 main tests and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast --quiet`: PASS,
  1,700 main tests, 118 experiment tests, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy,
  all-feature release build, release help/version (`ca 0.1.0`),
  `git diff --check`, snapshot hygiene, and oracle/reference immutability: PASS.

Validation used disposable workspaces, loopback fixtures, and the local Cargo
registry cache. It did not inspect credentials, create provider traffic, mutate
runtime configuration/session state, install a binary, or read/modify
oracle/reference content.

## TUI-148 fx-Inspired Quiet Default

Characterization first failed on the old five-row idle chrome and registry-order
empty slash palette. Focused assertions now require exactly three non-empty idle
rows, no duplicated labels or full-row title fill, one occurrence of configured
title/status facts, the locked ten-command discovery prefix, and complete unique
registry coverage. Reviewed wide/narrow snapshots cover startup, discovery,
streaming, queue, approval, failure, completion, and status-line states.

Local completion results:

- `cargo test tui:: --no-fail-fast`: PASS, 391 tests.
- `cargo test --no-fail-fast --quiet`: PASS, 1,652 main and 37 CLI tests;
  one ignored.
- `cargo test --all-targets --all-features --no-fail-fast --quiet`: PASS,
  1,681 main, 118 experiment-target, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy, all-feature
  release build, local help/version (`ca 0.1.0`), diff/snapshot hygiene, and
  oracle/reference diff checks: PASS.

Validation used deterministic TestBackend rendering. It did not run a native
PTY, provider traffic, credential access, runtime configuration/session writes,
installation, or oracle/reference access.

## REL-147 Route-Aware Token Estimates and Relevant Memory Recall

Characterization first proved that punctuation-heavy input could remain below
the old bytes/4 checkpoint threshold and that a relevant record appended past
the insertion-order cutoff was omitted. Exact-route tests cover official
OpenAI/Codex, MiMo, and DeepSeek family identity, custom-proxy isolation, an
explicit conservative quality label, risky code/JSON/identifier/Unicode input,
and no regression below the legacy heuristic. Memory tests cover late
relevance, broad-record reserve, unmatched external-fact exclusion, independent
scope/storage behavior, exact bounds, and Debug redaction.

Focused local results:

- `cargo test context::tests --no-fail-fast`: PASS, 24 tests.
- `cargo test memory::tests --no-fail-fast`: PASS, 13 tests.
- `cargo test service::tests --no-fail-fast`: PASS, 22 tests, including
  per-user-request recall reselection without prompt accumulation.
- Focused enabled-memory, mutation-refresh, side-question, and fallback suites:
  PASS.
- `cargo test context_dashboard_snapshots --no-fail-fast`: PASS after reviewing
  the intended conservative estimate/label changes at wide and narrow widths;
  no `.snap.new` remains.

Completion results:

- `cargo test --no-fail-fast`: PASS, 1,651 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast --quiet`: PASS,
  1,680 main, 118 experiment-target, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D
  warnings`, `cargo build --release --all-features`, release help/version (`ca
  0.1.0`), `git diff --check`, snapshot hygiene, and oracle/reference diff
  checks: PASS.

Validation performed no provider/tokenizer request and did not inspect
credentials or mutate configuration, sessions, memory stores, installed
binaries, or oracle/reference trees. Deterministic unit, loopback/service, and
render coverage is the acceptance evidence.

## PRV-146 First-Party OpenAI, MiMo, and DeepSeek Support

Exact-route capability tests prove official endpoint/protocol/model matching,
proxy isolation, user-profile/legacy-context precedence, current modalities and
reasoning values, and newly generated MiMo Responses defaults. Hermetic request
and two-turn tool fixtures cover direct OpenAI, fake Codex runtime resolution,
MiMo Responses/Chat, and DeepSeek Chat/Responses, including replay,
unsupported-field exclusion, keep-alives, usage/cache math, and secret
redaction. Pricing tests cover catalog-v1 migration, endpoint/billing scoping,
Terra, MiMo PAYG/Token Plan, DeepSeek cache rates, and stale-rate failure.

The provider-contract verifier is exercised through injectable authentication
and transport: family filtering, zero-request auth failure, the two-request
ceiling, no retry, partial failure, synthetic local-tool continuation,
sanitized versioned reports, and unchanged ordinary doctor behavior. Direct
OpenAI cache modes have request and provider-editor wide/narrow coverage.

Local completion results:

- Focused config/capability, provider request/decoder, client continuation,
  catalog/template, pricing/usage, trace-redaction, contract-verifier,
  doctor/CLI, orchestration, and provider-editor input/render suites: PASS.
- The first complete run found six expected provider-editor snapshot diffs
  caused by a cache hint appearing on unrelated providers. The hint was scoped
  to exact direct OpenAI routes; no snapshot update was accepted, and the
  corrected render and complete suites pass.
- `cargo test --no-fail-fast`: PASS, 1,645 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,674 main,
  118 experiment-target, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy, `cargo build
  --release --all-features`, local release help/version (`ca 0.1.0`), `git diff
  --check`, snapshot hygiene, and oracle/reference diff checks: PASS.

The live `--provider-contract` verifier was deliberately not invoked because
it requires user credentials and can incur paid traffic. No configuration,
session, ledger, credential store, installed binary, native TUI, or
oracle/reference tree was read or mutated by validation; provider exercises
used fake credentials and loopback transports only.

## TUI-145 Simultaneous Trackpad Scroll and Text Selection

Default Scroll mode now keeps wheel/trackpad navigation while forwarding
left-button down/drag/up into a bounded process-local selection over the
visible conversation cells. Focused coverage proves forward/reverse,
multiline, Unicode-wide-cell, padding, out-of-area, click-only, wheel-clear,
clipboard-success/failure, exact-text, visual-modifier, and lifecycle behavior.
Release copies through the existing in-process clipboard dependency; failure
is visible and retains the selection.

Local completion results:

- Pre-fix characterization: expected compile FAIL because typed mouse-selection
  events and the visible selection model did not exist.
- `cargo test tui::event::lifecycle_tests -- --nocapture`: PASS, 9 tests.
- `cargo test tui::transcript::tests -- --nocapture`: PASS, 5 tests.
- `cargo test tui::input -- --nocapture`: PASS, 163 tests.
- `cargo test tui::render_tests -- --nocapture`: PASS, 158 tests, including
  exact buffer text/style assertions for the selected range.
- `cargo test --no-fail-fast`: PASS, 1,624 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast --quiet`: PASS,
  1,653 main, 118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy, `cargo build
  --release --all-features`, local release help/version (`ca 0.1.0`), `git diff
  --check`, snapshot hygiene, and oracle/reference diff checks: PASS.

No native interactive terminal walkthrough, clipboard read, runtime
configuration/session mutation, installed-binary replacement, provider
traffic, or oracle/reference access ran. Deterministic event, clipboard-injection,
and TestBackend coverage is the acceptance evidence.

## AGT-144 Automatic Safe Agent Launches

Focused coverage proves safe asynchronous read-only child grants skip
AutoReview approval while empty, unknown, write, shell, web,
external-communication, nested-agent, command, persistent Ask/Deny, strict
Allowlist, and synchronous `task` cases retain their boundaries. Schema and
routing tests prove omitted complexity inherits the parent and explicit
unavailable complexity fails once without fallback. Injectable fake resolvers
and loopback transport prove Codex runtime authentication happens before
traffic, non-Codex routes bypass it, concurrent resolution is serialized,
failure records no request/usage, and credentials remain redacted. Wide and
narrow snapshots plus structural assertions prove exact contiguous failure
grouping without mutating underlying calls.

Local completion results:

- Pre-fix characterizations: expected FAIL for safe-spawn AutoReview and
  missing inherited/explicit complexity guidance.
- `cargo test permission::tests --no-fail-fast`: PASS, 11 tests.
- `cargo test agent::subagent::manager_tests --no-fail-fast`: PASS, 16 tests.
- Focused schema, inherited/fail-closed routing, runtime-credential,
  serialized-resolution, and grouping tests: PASS.
- `cargo test tui::render_tests --no-fail-fast`: PASS, 157 tests, including
  reviewed wide/narrow collapsed and expanded grouping snapshots.
- `cargo test --no-fail-fast`: PASS, 1,619 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast --quiet`: PASS,
  1,648 main, 118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy, `cargo build
  --release --all-features`, local release help/version (`ca 0.1.0`), `git diff
  --check`, snapshot hygiene, and oracle/reference diff checks: PASS.

No live authentication, paid provider traffic, credential inspection, native
TUI session, runtime configuration/session mutation, binary installation, or
oracle/reference access was used. Official OpenAI authentication documentation
was checked for the cached-login and automatic-refresh boundary; executable
acceptance used only fake credentials and loopback transport.

## REL-143 Low-Friction Reads and Credential Migration

Focused coverage proves exact/comment-preserving legacy approval migration for
default and named profiles, strict `ask`, non-persistent bare behavior, invalid
configuration/write failure safety, and persistent `/auto-review` toggling.
Approval coverage proves typed list/search/read tools and closed local read-only
pipelines skip AutoReview approval while mutation, network, explicit Ask/Deny,
unknown command, unsafe shell, and workspace boundaries remain enforced.
Fake primary/legacy credential stores prove one-time import, no later legacy
read, profile separation, primary-only save/delete, rollback retention,
permissions, atomic replacement, symlink and oversize refusal, actionable
migration failure, and secret-safe Debug/error paths.

Local completion results:

- Pre-fix characterization: expected compile FAIL (`E0599` for the missing
  approval update boundary and `E0433` for the missing migration store).
- Configuration store tests: PASS, 72 tests.
- Provider-secret tests: PASS, 10 tests; permission tests: PASS, 10 tests.
- `cargo test auto_review -- --nocapture`: PASS, six main tests and one CLI
  policy test; the exact `list_dir`, search, file-read, and safe-pipeline cases
  are included.
- SES-142 exact session-exit handoff regression: PASS.
- `cargo test --no-fail-fast --quiet`: PASS, 1,610 main and 37 CLI tests; one
  ignored.
- `cargo test --all-targets --all-features --no-fail-fast --quiet`: PASS,
  1,639 main, 118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy, `cargo build
  --release --all-features`, released help/version (`ca 0.1.0`), `git diff
  --check`, snapshot hygiene, and oracle/reference diff checks: PASS.

Live delivery used timestamped binary/config backups and an atomic install. The
default config now contains `approval_mode = "auto-review"`; the private
credential directory is 0700 and both configured provider files are regular
0600 files. The one-time import completed with user authorization. A second
secret-resolving `--list-models` load finished in 0.01 seconds without another
Keychain prompt, and offline `ca --doctor` reports Ready. No provider request
was sent and credential contents were never inspected.

## SES-142 Exit Session Handoff

Focused coverage proves a resumable interactive session produces the exact
two-line `Session`/`Continue` block with a public `ses_…` continuation command,
sanitizes and display-cell-bounds the title, and omits output without an
identity, user turn, or persistence capability. Existing `/session`, short
resume parsing, and public-ID behavior remain covered.

Local completion results:

- Pre-fix characterization: expected compile FAIL because the formatter did
  not exist.
- `cargo test session_exit_handoff_matches_mimocode_shape_and_omission_boundaries
  -- --nocapture`: PASS, one focused test.
- Focused `/session`, `-s`, and public-ID regression tests: PASS, three tests.
- `cargo test --no-fail-fast`: PASS, 1,602 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast --quiet`: PASS,
  1,631 main, 118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy, `cargo build
  --release --all-features`, released help/version (`ca 0.1.0`), `git diff
  --check`, snapshot hygiene, and oracle/reference diff checks: PASS.

Manual native TUI output was not exercised because isolating provider/session
runtime state would require authority outside this task. The pure formatter
test and the structural `run_tui` return/`run_standard` print boundary are the
recorded post-restoration evidence. No installed binary or runtime state was
changed.

## REL-141 Practical Read-Only AutoReview

Focused coverage proves the exact reported `find src -maxdepth 3 -type f |
sort` pipeline is recognized by interactive AutoReview and executes through the
headless tool path. Ordinary `find` predicates/printing and stdout-only `sort`
are accepted; `find` exec/execdir/ok/okdir/delete/fls/fprint/fprint0/fprintf and
`sort` output/compression-helper modes remain gated. Existing closed-shell,
background, unknown-program, explicit Ask/Deny, and network boundaries remain
covered.

Local completion results:

- Pre-fix characterization: expected FAIL on the exact reported pipeline.
- `cargo test auto_review -- --nocapture`: PASS, five focused main tests and
  one CLI policy test.
- `cargo test permission::tests -- --nocapture`: PASS, 10 tests.
- `cargo test --no-fail-fast`: PASS, 1,601 main tests and 37 CLI tests; one
  ignored manual browser test.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,630 main
  tests, 118 experiment tests, and 37 CLI tests; two documented ignores.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy, `cargo build
  --release --all-features`, released help/version (`ca 0.1.0`), `git diff
  --check`, snapshot hygiene, and oracle/reference diff checks: PASS.

No manual TUI check was needed because the approval decision and exact command
execution are deterministic and no layout changed. No runtime configuration,
session, credential, installed binary, oracle/reference file, or Git history
was changed.

## SES-140 Automatic Session Names and Public Identifiers

Focused coverage proves fixed-width `ses_…` round trips for nil, representative,
and maximum UUIDs; malformed/noncanonical/overflowing values fail; public IDs
resolve through full and paged store loads without changing UUID storage keys;
first-request titles normalize Markdown/whitespace, omit attachment bodies,
truncate at Unicode display-cell boundaries, and preserve explicit names; `-s`
parses the public form; identity exists before the first turn; `/session` emits
the exact title/identifier/continuation command; and no-session runs refuse to
advertise continuation.

Local completion results:

- five focused public-ID/title/store/CLI/identity tests and the focused session
  report test: PASS.
- `cargo test tui::render_tests --no-fail-fast`: PASS, 155 tests, including
  reviewed wide/narrow session report and stacked session-picker snapshots.
- `cargo test --no-fail-fast`: PASS, 1,600 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,629 main,
  118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy,
  `cargo build --release --all-features`, released help/version (`ca 0.1.0`),
  `git diff --check`, snapshot hygiene, and oracle/reference diff checks: PASS.

## TUI-139 White Composer Cursor Blink

Focused TestBackend coverage proves the visible phase applies black-on-white
cell styling at empty input, ASCII end-of-input, and before a wide Unicode
glyph; the alternate phase restores the underlying cell; modal focus suppresses
the cursor; and hard lines, soft wraps, and exact-width wraps retain their exact
insertion cells. A cadence test proves nine 50 ms ticks preserve the phase, the
tenth flips it, and user-input reset returns immediately to visible white.

Local completion results:

- `cargo test cursor_blink -- --nocapture`: PASS, 2 focused tests.
- `cargo test composer_white_cursor_tracks_hard_lines_soft_wraps_and_exact_width_wraps -- --nocapture`: PASS.
- `cargo test tui::render_tests --no-fail-fast`: PASS, 155 tests.
- `cargo test --no-fail-fast`: PASS, 1,595 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,624 main,
  118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy, and `cargo
  build --release --all-features`: PASS.
- released help/version (`ca 0.1.0`), `git diff --check`, snapshot hygiene, and
  oracle/reference diff checks: PASS.

## TUI-138 Composer-First Usability Overhaul

Focused coverage proves shared grapheme/display-cell wrapping and exact
cursor placement across hard lines, soft wraps, exact-width wraps, combining
sequences, and wide glyphs; seven-row overflow, preferred-column movement,
history boundaries/sidecar restoration, line-local and Vim motion, atomic
movement/deletion, range-bound paste expansion, image queue refusal, and
Finder-style folder registration without authority grant. Completion tests
cover slash Enter/Tab metadata, token-scoped Escape, alias ranking, attachment
replacement, generation-safe path work, and directory/file continuation.
Transcript/lifecycle coverage proves wrapped-row offsets, frozen scrolling,
unseen/follow behavior, lazy-history anchoring, focused mouse routing, ordered
capture/toggling, unsupported fallback, suspend/resume, failure, panic, and
Drop cleanup.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,594 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS on complete
  rerun, 1,623 main, 118 experiment, and 37 CLI tests; two ignored. The first
  run had one unrelated voice descendant-cleanup timing failure; its isolated
  rerun and the complete rerun passed.
- `cargo test tui:: --no-fail-fast`: PASS, 380 TUI tests.
- all 154 render tests and reviewed 100×30/60×24 compact-startup, multiline,
  inline-completion, narrow-row, picker, queue, chip, footer, and transcript
  states pass; no `.snap.new` files remain.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy,
  `cargo build --release --all-features`, released help/version (`ca 0.1.0`),
  `git diff --check`, and oracle/reference diff checks: PASS.

Manual result: BLOCKED. A native TUI needs an isolated `HOME` to avoid the
user's real profile paths, but repository safety rules explicitly prohibit
overriding `HOME`. A bare no-session attempt stopped at the existing credential
startup check and changed no configuration, session, provider, installed
binary, oracle/reference file, or Git history. Finder drag and physical
trackpad behavior are not claimed; deterministic bracketed-paste, mouse-event,
terminal-lifecycle, native-cursor, and render coverage is the recorded evidence.

## TUI-137 Native Composer Cursor

Historical note: TUI-139 supersedes terminal-owned cursor presentation while
retaining the exact insertion-cell and modal-focus contract tested here.

Focused TestBackend coverage asserts the runtime buffer contains no composer
`▌` glyph and the native backend cursor is at `(3,27)` for empty input,
`(7,27)` immediately after `over` with no trailing space, and `(6,27)` before
the wide `界` in `A🙂界`. The same test asserts modal focus hides the native
composer cursor. The complete 150-test render suite passes, and snapshot-only
cursor notation preserves deterministic existing visual coverage with no
`.snap.new` files.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,561 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,590 main,
  118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy, and `cargo
  build --release --all-features`: PASS.
- release help/version (`ca 0.1.0`), `git diff --check`, snapshot hygiene, and
  oracle/reference diff check: PASS.

Manual blink capture: not run. Ratatui's backend position/visibility contract
is deterministic, while blink cadence and full cursor shape belong to the
user's terminal. No runtime state, installed binary, oracle/reference file, or
Git history changed.

## TUI-136 Bracketed Paste for Folder Drops

Focused lifecycle coverage proves bracketed-paste acquisition, reverse-order
normal cleanup, partial-setup cleanup, suspend/resume reacquisition, failed-
resume cleanup, and panic cleanup. Focused composer coverage proves a pasted
absolute folder path remains literal input with the slash palette closed while
an ordinarily typed `/` still opens Commands.

Local completion results:

- focused lifecycle and composer tests: PASS, seven and one tests.
- `cargo test --no-fail-fast`: PASS, 1,560 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,589 main,
  118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy, and `cargo
  build --release --all-features`: PASS.
- release help/version (`ca 0.1.0`), `git diff --check`, snapshot hygiene, and
  oracle/reference diff check: PASS.

Manual result: not run. A native Finder drag requires an interactive terminal;
the deterministic terminal-mode and exact composer-event tests ran without
touching user runtime state. No binary was installed.

## REL-135 Sandboxed SSH and Read-Only Approval Friction

Focused coverage proves least-authority ordinary macOS OpenDirectory `libinfo`
lookup with the experiment profile remaining closed, nonnumeric user
resolution inside a required sandbox, exact SSH-family network inference,
unchanged one-use network approval under Yolo and broad allow rules, closed
read-pipeline parsing, executable/path bounds, background refusal, dangerous
flag/shell rejection, explicit Ask/Deny precedence, interactive and headless
AutoReview execution, the new AutoReview default, and explicit `allowlist`
compatibility without rewrite.

Manual result:

- A required macOS sandbox matching the ordinary generated profile completed
  `ssh bsd true` with BatchMode, strict known-host checking, disabled
  multiplexing, and a five-second connection timeout. Exit was 0 with no
  output. The check read existing SSH authority but did not print it or mutate
  local/remote files.

Local completion results:

- focused permission/sandbox/workspace/job/config and orchestration tests:
  PASS (10, 7, 7, 17, and 70 module tests plus focused interactive/headless
  cases).
- `cargo test --no-fail-fast`: PASS, 1,557 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS on complete
  rerun, 1,586 main, 118 experiment, and 37 CLI tests; two ignored. The first
  run had one unrelated voice descendant-cleanup timing failure; its isolated
  rerun and the complete rerun both passed.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy, and `cargo
  build --release --all-features`: PASS.
- release help/version (`ca 0.1.0`), `git diff --check`, snapshot hygiene, and
  oracle/reference diff check: PASS.

Physical FreeBSD: not run. The user regression and authority repair are macOS-
specific; platform-neutral parser/preparation changes pass all local targets.

No user config/session, SSH key/config/known-host file, remote file, installed
binary, provider, oracle/reference file, or Git history was changed.

## LRN-134 Autonomous Dream and Distill

Focused coverage proves absent-versus-explicit-false defaults, due intervals,
excluded-runner capability, private typed requests, distinct usage kinds,
session/message/time/byte corpus bounds, learning-ineligible import and private
email exclusion, Dream ownership and cross-workspace evidence, CAS drift
refusal, crash recovery, exact undo, Distill replacement/rollback, and strict
generated-skill authority. Reviewed wide/narrow snapshots cover Dream and
Distill status, and `/usage` snapshots include both maintenance categories.

Disposable manual result:

- A released native TUI with a disposable home/workspace showed due automatic
  Dream and Distill skips when no eligible evidence existed, rendered the full
  `/dream status` schedule/run/route/evidence/result view, and persisted
  `/dream off` without changing Distill. The state and config stores were mode
  `0600`; the state contained two distinct bounded skipped automatic run
  records, while the config preserved explicit Dream `false`, Distill `true`,
  and enabled absent-memory defaults. The fixture moved recoverably to
  `/Users/aji/.Trash/ca-lrn134-pty-20260817`.
- A credentialless custom provider cannot satisfy the existing official-price
  route gate, so the PTY correctly could not exercise a maintenance POST by
  weakening that boundary. Injected provider tests cover the strict structured
  one-request path. No real provider was contacted.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,548 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,577 main,
  118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --check`, strict all-target/all-feature Clippy,
  `cargo build --release --all-features`, release help/version (`ca 0.1.0`),
  `git diff --check`, snapshot hygiene, and oracle/reference diff check: PASS.

Physical FreeBSD completion results:

- Exact source copied to `/tmp/ca-mimo-roadmap.08YMBV` on FreeBSD
  15.1-RELEASE-p2 amd64 and tested as unprivileged `aji` with Rust/Cargo
  1.96.1.
- `cargo test --locked --no-fail-fast`: PASS, 1,535 applicable main and 37 CLI
  tests; the manual Chromium test is ignored.
- `cargo test --locked --all-targets --all-features --no-fail-fast`: PASS,
  1,564 applicable main, 116 experiment, and 37 CLI tests; the manual Chromium
  and complete external-fixture tests are ignored.
- Packaged native `cargo-fmt`, strict all-target/all-feature Clippy, locked
  all-feature release build, help/version, snapshot hygiene, FreeBSD 15.1
  x86-64 ELF identity, and linked-library inspection: PASS. The user-local
  rustup formatter/Clippy shims are stale non-native links, so the packaged
  `/usr/local/bin` tools were used directly.
- FreeBSD release SHA-256:
  `e5ed2c281efac7391d5dc4db793576871978996b3f517b22a307abe541c34c8c`.

No provider request, credential, installed binary, profile configuration,
session, oracle/reference tree, or project outside the disposable source tree
was changed.

## SES-133 Foreign Transcript Import

Focused coverage proves closed Codex and Claude JSONL adapters, immutable
read-only OpenCode SQLite access, explicit-file/directory traversal without
symlinks, 1,000-file and 64-MiB caps, partial-error reporting, message/text
bounds, hidden/system/reasoning/tool/media/credential filtering, safe timestamp
retention, path-free provenance, old-session compatibility, preview-before-
write, Cancel-first input, explicit learning opt-in, fresh inert session
publication, active-session isolation, and current-route resume. Reviewed
wide/narrow snapshots cover the foreign preview; command-palette snapshots
cover the four `/import` forms. No `.snap.new` remains.

Local completion results:

- `cargo test`: PASS, 1,532 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,561 main,
  118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --check`, strict all-target/all-feature Clippy,
  `cargo build --release --all-features`, release help/version, `git diff
  --check`, and snapshot hygiene: PASS.

Disposable manual result:

- A released native TUI with a disposable home/workspace and six-record Claude
  source showed asynchronous inspection and an accurate preview of one session,
  two visible messages, four skipped records, zero errors, destination profile,
  and default-off learning. Enter on the initial Cancel choice wrote nothing;
  a second preview confirmed Import and opened a picker containing exactly one
  fresh session. Persisted metadata used the configured fake route and retained
  only filename-plus-digest provenance; history contained the two safe source
  timestamps and no hidden-system, chain-of-thought, tool-output, or dangerous
  sentinels. The fixture moved recoverably to
  `/Users/aji/.Trash/ca-ses133-pty-20260816`.

Physical FreeBSD: not requested or run for this `ca` extension.

## CFG-132 Recent and Favorite Models

Focused coverage proves recency caps/deduplication, favorite identity bounds,
unique Favorites/Recent/provider projections, dormant unavailable models and
their reappearance, owner-only atomic storage, corruption recovery, symlink
refusal, bare process-only behavior, old partial-keymap compatibility, scoped
cycling, explicit favorite interaction, and every persistent selection entry
point. Reviewed wide/narrow snapshots cover preference sections and updated
keymap help. No `.snap.new` remains.

Local completion results:

- `cargo test`: PASS, 1,520 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,549 main,
  118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --check`, strict all-target/all-feature Clippy,
  `cargo build --release --all-features`, release help/version, `git diff
  --check`, and snapshot hygiene: PASS.

Disposable manual result:

- A released native TUI with a disposable profile added a favorite, selected a
  distinct recent model, rendered unique scoped sections, exercised both new
  cycle chords, and retained state across restart. A bare run loaded no stored
  preference rows, refused favorite mutation, kept selection process-only, and
  left the persisted preference SHA-256 unchanged. The fixture moved
  recoverably to `/Users/aji/.Trash/ca-cfg132-pty-20260816`.

Physical FreeBSD: not requested or run for this `ca` extension.

## INP-131 Explicit Prompt Stash

Focused coverage proves explicit-only persistence, restore/remove and atomic
swap semantics, entry/count/aggregate bounds, corruption quarantine, bare
refusal, profile-relative symlink rejection, owner-only storage, keymap chord
validation, overlay input isolation, repeated delete confirmation, pasted-image
refusal, and restored attachment drift through ordinary send-time validation.
Six reviewed wide/narrow snapshots cover populated, busy, and bare picker
states; existing keymap snapshots include the new configurable action. No
`.snap.new` remains.

Local completion results:

- `cargo test`: PASS, 1,507 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features`: PASS, 1,536 main, 118 experiment,
  and 37 CLI tests; two ignored.
- `cargo fmt --check`, strict all-target/all-feature Clippy,
  `cargo build --release --all-features`, release help/version, `git diff
  --check`, and snapshot hygiene: PASS.

Disposable manual result:

- A released native TUI with a disposable profile saved `keep this draft` via
  `Ctrl+X S`, displayed its preview/age/line count in `/stash`, restored it
  with Enter, and required two `d` presses to delete it. The initial run caught
  macOS `/tmp`'s system symlink ancestry; the corrected validation canonicalizes
  the home and rejects only profile-owned symlink components. The successful
  fixture moved recoverably to
  `/Users/aji/.Trash/ca-inp131-pty-20260816`.

Physical FreeBSD: not requested or run for this `ca` extension.

## TUI-130 Optional Turn Metadata

Focused coverage proves final-annotation lifecycle updates, completed/failed/
cancelled outcome mapping, deterministic duration formatting, absent-value
honesty, old/new session compatibility, default-off UI configuration, exactly
one final-assistant footer, and shared speed/cost classification for missing
timing, usage, pricing, incremental billing, subscription allocation, mixed,
and unknown states. Twelve new reviewed wide/narrow snapshots cover completed
and interrupted transcript metadata plus all four usage statusline cost states.
Existing statusline-manager snapshots intentionally add the two opt-in rows.
No `.snap.new` remains.

Local completion results:

- `cargo test`: PASS, 1,497 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features`: PASS, 1,526 main, 118 experiment,
  and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy,
  `cargo build --release --all-features`, release help/version, `git diff
  --check`, and snapshot hygiene: PASS.

Disposable manual result:

- A released 80x24 no-session TUI used a disposable home/workspace and one
  credentialless delayed loopback response. It displayed timestamps and one
  final `fake/fake-model · 705ms` footer. `/context` independently showed 12
  output tokens at 18.3 tokens/s and unknown cost. Only the injected config and
  disposable workspace-trust update existed; the fixture moved recoverably to
  `/Users/aji/.Trash/ca-tui130-pty.Tv20E5`.

Physical FreeBSD: not requested or run for this `ca` extension.

## TUI-129 Asynchronous `@` Attachments

Focused coverage proves ignore-aware multi-root discovery, explicit empty-index
readiness, cancellation/stale-generation rejection, deterministic fuzzy-first
and frecency-second ranking, relative owner-only persistence, corruption
recovery, record/result/query caps, strict line ranges, send-time drift,
traversal/oversize/directory/symlink refusal, typed picker insertion, MCP range
refusal, metadata non-activation, and bounded send-time MCP resolution. Eight
reviewed wide/narrow snapshots cover loading, multi-root/MCP results with a
range, no matches, and safe failure. No `.snap.new` remains.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,488 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,517 main,
  118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy,
  `cargo build --release --all-features`, release help/version, `git diff
  --check`, and snapshot hygiene: PASS.

Disposable manual result:

- A released 80x24 bare/no-session TUI used a disposable home/workspace.
  Opening `@` visibly entered indexing, then showed only `README.md` and
  `src/main.rs` while excluding `.gitignore`-ignored content. Typing
  `main#2-3` narrowed to one row and Enter inserted exact
  `@src/main.rs#2-3`. The home remained empty. The stopped fixture moved
  recoverably to `/Users/aji/.Trash/ca-tui129-pty.hqK18T`.

Physical FreeBSD: not requested or run for this `ca` extension.

## REL-128 Low-Yield Loop Guard

Focused coverage proves normalization and UTF-8-safe bounds, three-token
shingle similarity and the 12-edit window, unchanged failed-command detection,
four-action no-progress streaks, progress/reset/suppression boundaries,
process-only one-shot Replan, coordinator wake-up, structured service choices,
typed unattended failure, Cancel-first input, and reviewed wide/narrow warning
and confirmation snapshots. No `.snap.new` remains.

Local completion results:

- `cargo test loop_guard --no-fail-fast`: PASS, 13 focused tests.
- `cargo test low_yield --no-fail-fast`: PASS, three headless/service/
  orchestration tests.
- `cargo test --no-fail-fast`: PASS, 1,479 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,508 main,
  118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy,
  `cargo build --release --all-features`, release help/version, `git diff
  --check`, and snapshot hygiene: PASS.

Disposable manual result:

- A released 80x24 no-session TUI used a disposable home/workspace and
  credentialless loopback provider. Three completed `run_command false` calls
  opened the bounded low-yield dialog; `c` opened confirmation with Keep paused
  selected, Enter retained the pause, and `r` immediately resumed to the
  provider's distinct `Replanned safely.` response. The first walkthrough
  caught a missing coordinator wake-up; the added typed resume action and
  regression assertion fixed it before the successful rerun. The stopped
  fixture moved recoverably to `/Users/aji/.Trash/ca-rel128-pty.M7cKLW`.

Physical FreeBSD: not requested or run for this `ca` extension.

## TUI-127 Timed Thought and Inline Agent Activity

Focused timing, deterministic duration formatting, generation-speed/unknown
usage, structured activity ordering/bounds/sanitization/origin, live watch,
terminal anchoring, legacy/new session resume and branch filtering,
non-activating LSP status, key validation, activity input controls, terminal
read-only behavior, and context composition tests pass. Reviewed deterministic
wide/narrow snapshots cover collapsed/expanded thoughts, multiple live agents,
approval waiting, focused detail, resumed summaries, keymap registration, and
every `/context` section; no `.snap.new` remains.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,462 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,491 main,
  118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy,
  `cargo build --release --all-features`, release help/version, `git diff
  --check`, and snapshot hygiene: PASS.

Disposable manual result:

- A released 80x24 no-session TUI used a disposable home/workspace and one
  credentialless MiMo-dialect loopback request. The live collapsed thought
  timer froze at 652ms, `/thinking on` exposed the same frozen duration and
  body, and `/context` reported 18.3 tokens/s from 12 output tokens alongside
  unknown cost, workspace roots, inactive LSP, and task status. No session or
  credential was created. The stopped fixture contained only its injected
  config and moved recoverably to
  `/Users/aji/.Trash/ca-tui127-walk.s3mT9Z`.

Physical FreeBSD: not requested or run for this `ca` extension.

## PRJ-126 and combined local validation

Focused scratch storage/tool/render tests passed. Combined default: 1448 passed,
1 ignored; all-feature main: 1477 passed, 1 ignored; experiment: 118 passed,
1 ignored; CLI: 37 passed. Formatting, strict all-feature Clippy, release build,
help/version, diff, and snapshot hygiene passed. The user explicitly paused BSD
work, so PRJ-126/final FreeBSD checks were not run and are not claimed.

## OUT-125 Semantic Command Output

Local focused tests, wide/narrow snapshots, default (1444 passed, 1 ignored),
all-feature main (1473 passed, 1 ignored), experiment (118 passed, 1 ignored),
CLI (37 passed), formatting, strict Clippy, release, help/version, diff, and
snapshot hygiene passed. Physical FreeBSD exact-source focused/default/all-feature
tests passed (1431 main default; 1460 main all-feature; 116 experiment; 37 CLI;
expected ignores only). The native Clippy/release continuation was stopped at
the user's request and is not claimed.

## CFG-124 One-Turn Models and Route Presets

Characterization began with 21 expected missing state/config/command/input and
render symbols. Coverage now proves original-baseline retention across unused
override replacement, exact consumption by main turns, restoration after all
three terminal outcomes, parent-agent route updates, ordinary-model and preset
cancellation, route preparation, strict name/config validation,
comment-preserving atomic saves, failed-save stability, bare refusal, and
Cancel-first overwrite behavior.

Focused local results:

- One-turn lifecycle, routing, command-cancellation, preset config/store,
  command metadata, input, and render tests: PASS.
- Eight reviewed 100x30/60x24 picker empty/list/error and overwrite
  cancel/busy/error snapshots: PASS. Existing provider palette/navigation
  remains unchanged and no `.snap.new` remains.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,437 main and 37 CLI; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,466 main,
  118 experiment, and 37 CLI; two ignored.
- Formatting, strict all-feature Clippy, all-feature release, release
  help/version, `git diff --check`, and snapshot hygiene: PASS.

Disposable manual result:

- An 80x24 no-session TUI used a disposable home/workspace and credentialless
  loopback provider. The selected one-turn request used `once`; its successor
  restored to `base`. Named presets saved only the restored route, applying a
  preset cancelled an unused override, and default Enter kept an existing
  preset byte-identical. No session was created; the provider stopped and the
  fixture moved recoverably to Trash.

Physical FreeBSD results:

- Physical FreeBSD 15.1-RELEASE-p2 amd64, unprivileged `aji`: focused tests
  PASS; default PASS with 1,424 main and 37 CLI; all features PASS with 1,453
  main, 116 experiment, and 37 CLI. The two documented ignores remain and the
  official-price experiment took 490 seconds.
- Native formatter, strict Clippy, all-feature release, help/version, snapshot
  hygiene, x86-64 ELF identity, linkage, and key-file exact-source hashes:
  PASS. The disposable source was removed; the shared native build cache
  remains for later tasks.

## CTX-123 Retained-Boundary Context Controls

Characterization began with the expected missing request/boundary/grid API
failures. Coverage now proves default and bounded `here` parsing, focus-text
separation, complete exchange detection across tool continuations, incomplete
and all-suffix refusal, prefix-only provider preparation, byte-faithful suffix
reconstruction, deterministic 100-cell largest-remainder allocation, stable
category colors, automatic-compaction separation, and reviewed context pager
rendering.

Focused local results:

- `compact_here`, `retained_checkpoint`, `context_dashboard`, `context_grid`,
  `automatic_checkpoint`, and `compact_focus` suites: PASS.
- Reviewed 100x30 and 60x24 `/context` snapshots: PASS. No `.snap.new` remains.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,427 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,456 main,
  118 experiment, and 37 CLI tests; two ignored.
- Formatting, strict all-feature Clippy, all-feature release, release
  help/version, `git diff --check`, and snapshot hygiene: PASS.

Disposable manual result:

- A 100x30 no-session TUI used a disposable home/workspace and credentialless
  loopback provider for three completed exchanges. `/compact here 2` made one
  checkpoint request containing only the first exchange; neither retained
  exchange entered that prompt. `/context` displayed the 5x20 grid and 2 of 6
  message coverage. No session was created. The fixture and logs were moved
  recoverably to Trash.

Physical FreeBSD results:

- Physical FreeBSD 15.1-RELEASE-p2 amd64, unprivileged `aji`: all focused
  boundary/orchestration/grid/snapshot tests PASS.
- Default: PASS, 1,414 main and 37 CLI tests; one ignored.
- All features: PASS, 1,443 main, 116 experiment, and 37 CLI tests; two
  documented cases ignored. The existing official-price experiment test took
  494 seconds and passed.
- Native formatter, strict Clippy, all-feature release, help/version, snapshot
  hygiene, x86-64 ELF identity, and linkage: PASS. The exact disposable source
  tree was removed; the shared native build cache remains for later tasks.

## USG-122 Provider-Account Usage

Characterization began with two intentional failures for the absent cache and
scope-separated report. Coverage now proves exact recognized-host routing for
arbitrary configured provider names, refusal of canonical providers on
overridden hosts, concurrent OpenAI Usage/Costs and DeepSeek balance requests,
exact bearer placement, UTC-month query fields, no retry/ledger mutation,
admin denial, malformed and oversized bodies, timeouts, MiMo local estimates,
cache identity/expiry, sanitized diagnostics, and deterministic ordering.

Focused local results:

- Ten `provider_usage` adapter/cache/report tests: PASS.
- The `/usage` orchestration/cache test: PASS.
- Eight reviewed loading, empty, unavailable, and populated wide/narrow pager
  snapshots: PASS. No `.snap.new` remains.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,420 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,449 main,
  118 experiment, and 37 CLI tests; two ignored.
- Formatting, strict all-feature Clippy, all-feature release, release
  help/version, `git diff --check`, and snapshot hygiene: PASS.

Disposable manual result:

- A 100x30 no-session TUI used a disposable home/workspace, a synthetic
  credential, and an overridden loopback OpenAI route. `/usage` showed loading,
  then refused the non-official host without network traffic, and a second
  invocation showed the 60-second cached report. The TUI exited cleanly. Only
  the fixture config existed, and the exact fixture was removed.

Physical FreeBSD results:

- Physical FreeBSD 15.1-RELEASE-p2 amd64, unprivileged `aji`: all focused
  adapter, command, cache, and snapshot tests PASS.
- Default: PASS, 1,407 main and 37 CLI tests; one ignored.
- All features: PASS, 1,436 main, 116 experiment, and 37 CLI tests; two
  documented cases ignored. The existing official-price experiment test used
  sustained CPU for about eight minutes and passed.
- Native formatter, strict Clippy, all-feature release, help/version, snapshot
  hygiene, x86-64 ELF identity, and linkage: PASS. The exact disposable source
  tree was removed; the shared native build cache remains for later tasks.

## TUI-121 Native Raw Transcript

Characterization began with five intentional failures for the absent raw
projection and presenter. Coverage verifies canonical ordering, explicit
system/reasoning/provider/legacy-tool omissions, private-email omission,
configured and secret-shaped credential redaction, embedded-media omission,
terminal-control sanitization, Unicode-safe tool-name bounds, exact native
output, Enter acknowledgement, I/O failures, command metadata, and argument
handling. The existing terminal-lifecycle tests continue to prove ordered
suspend/resume and cleanup during setup failure or panic.

Focused local results:

- Five `raw_transcript` projection/presenter tests and two exact
  `raw_command` tests: PASS.
- The native surface deliberately has no ratatui modal: exact emitted bytes
  are asserted directly. Existing command-palette snapshots remain green and
  no `.snap.new` remains.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,409 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,438 main,
  118 experiment, and 37 CLI tests; two ignored.
- Formatting, strict all-feature Clippy, all-feature release, release
  help/version, `git diff --check`, and snapshot hygiene: PASS.

Disposable manual result:

- An 80x24 bare/no-session TUI used a disposable home/workspace and a
  synthetic-credential loopback provider. `/raw` was searchable, left the
  alternate screen, printed the user/assistant transcript with the configured
  credential replaced by `[REDACTED]`, waited for Enter, restored/redrew the
  TUI, and exited cleanly. No session or user runtime state was created; the
  fixture was stopped and moved to Trash.

Physical FreeBSD results:

- Physical FreeBSD 15.1-RELEASE-p2 amd64, unprivileged `aji`: focused raw
  projection, presenter, registry, and command tests PASS.
- Default: PASS, 1,396 main and 37 CLI tests; one ignored.
- All features: PASS, 1,425 main, 116 experiment, and 37 CLI tests; two
  documented cases ignored. The existing official-price experiment test used
  sustained CPU for about eight minutes but completed successfully.
- Native formatter, strict Clippy, all-feature release, help/version, snapshot
  hygiene, x86-64 ELF identity, and linkage: PASS. The exact disposable source
  tree was removed; the shared build cache remains for later roadmap gates.

## SEC-120 Agent-Harness Audit

Characterization began with three intentional failures for the absent scanner:
deterministic secret-safe findings, symlink escape without target disclosure,
and bounded partial reporting. Coverage then expanded to Unix permissions,
terminal controls, invalid configuration without mutation, command metadata,
argument handling, named-profile CLI parsing, exit status, and profile-state
absence.

Focused local results:

- Five `security_audit` scanner tests: PASS.
- Two `audit_command` tests, one audit-subcommand parser test, and one CLI
  process test: PASS.
- Eight reviewed wide/narrow loading, partial report, empty, and error pager
  snapshots: PASS. No `.snap.new` remains.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,402 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,431 main,
  118 experiment, and 37 CLI tests; two ignored.
- Formatting, strict all-feature Clippy, all-feature release, release
  help/version/audit-help, `git diff --check`, and snapshot hygiene: PASS.

Disposable manual result:

- A disposable-home TUI showed `/audit` search, scanning, complete report,
  pager close, and terminal restoration with no provider/external network.
  The fixture moved to Trash; user config, sessions, credentials, installed
  binaries, oracles, and references were untouched.

Physical FreeBSD results:

- Physical FreeBSD 15.1-RELEASE-p2 amd64, unprivileged `aji`: focused audit,
  command, parser, and CLI tests PASS.
- Default: PASS, 1,389 main and 37 CLI tests; one ignored.
- All features: PASS, 1,418 main, 116 experiment, and 37 CLI tests; two
  documented cases ignored.
- Native formatter, strict Clippy, all-feature release, help/version/audit
  help, snapshot hygiene, x86-64 ELF identity, and linkage: PASS.
- The first full invocation accidentally ran as root and made the existing
  permission-denial fixture succeed incorrectly. That result was discarded;
  the exact same source passed under `aji`. The disposable source tree was
  removed while the shared build cache remains for later roadmap gates.

## LSP-119 Post-Edit Diagnostics

Characterization began with version-race, stale/unversioned, deletion,
sanitization, item/byte-bound, mutation-result, undo, and deterministic pager
tests. The byte-bound test exposed and fixed a truncation marker that could
otherwise extend a report beyond 32 KiB.

Focused local results:

- `cargo test post_edit --no-fail-fast`: PASS, 5 protocol, mutation,
  bounds, and render tests.
- `cargo test lsp --no-fail-fast`: PASS, 11 LSP-matching tests; the targeted
  undo diagnostic integration test also passes.
- Reviewed wide/narrow pager snapshots contain sanitized versioned
  diagnostics. No `.snap.new` remains.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,393 main and 36 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,422 main,
  118 experiment, and 36 CLI tests; two ignored.
- Formatting, strict all-feature Clippy, all-feature release build,
  help/version, `git diff --check`, and snapshot hygiene: PASS.

Disposable manual result:

- A disposable loopback provider requested one approved `write_file` against
  a fake configured LSP. The TUI pager displayed the matching version-1
  warning with source/code/range, and the next provider request contained that
  report. The fixture and its process were removed; no user config, session,
  credential, installed binary, oracle, or reference changed.

Physical FreeBSD results:

- Physical FreeBSD 15.1-RELEASE-p2 amd64, Rust/Cargo 1.96.1, unprivileged
  `aji`: focused 5 `post_edit` and 13 broad `lsp` matches PASS.
- Locked default suite: PASS, 1,380 main and 36 CLI tests; one ignored.
- Locked all-target/all-feature main and CLI suites: PASS, 1,409 main and 36
  CLI tests; the corrected metadata-free exact-source experiment target also
  passes 116 tests with one ignored.
- Native formatter, strict all-feature Clippy, all-feature release,
  help/version, FreeBSD x86-64 ELF, linkage, diff, and snapshot hygiene: PASS.
- One intermediate archive admitted AppleDouble companion files and therefore
  failed 25 exact fixture-manifest tests. It was removed, the archive command
  was corrected, and the clean exact-source rerun passed. No LSP assertion
  failed in either run, and no disposable LSP-119 source tree remains.

## INI-118 Safe `/init`

Focused local results:

- `cargo test project_init --no-fail-fast`: PASS, 7 evidence, validation,
  fingerprint, symlink/existing-target, safe-create, and undo tests.
- `cargo test init_ --no-fail-fast`: PASS, 8 matching command, input,
  orchestration, render, and service tests.
- Reviewed wide/narrow snapshots cover preparing, complete preview, and error
  states. Intentional usage and command-palette changes are accepted; no
  `.snap.new` remains.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,387 main and 36 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,416 main,
  118 experiment, and 36 CLI tests; the two documented cases ignored.
- `cargo fmt --all -- --check`, strict all-feature Clippy, all-feature release
  build, release help/version, `git diff --check`, and snapshot hygiene: PASS.

Disposable manual result:

- A bare-profile 80x24 TUI used a disposable home/root and a loopback-only
  provider with a synthetic credential. Each `/init` invocation produced
  exactly one request and its JSON body contained no tool schema. Default
  confirmation canceled with the target absent; explicit Create wrote the
  complete preview; `/undo` restored the target to Missing.
- The fixture created no session or additional runtime file and was removed.

Physical FreeBSD results:

- Physical FreeBSD 15.1-RELEASE-p2 amd64, Rust/Cargo 1.96.1, unprivileged
  `aji`: focused 7 `project_init` and 8 `init_` tests PASS.
- Locked default suite: PASS, 1,374 main and 36 CLI tests; one ignored.
- Locked all-target/all-feature suite: PASS, 1,403 main, 116 experiment, and
  36 CLI tests; two documented cases ignored.
- Native formatter, strict all-feature Clippy, all-feature release build,
  help/version, ELF identity, linkage, and snapshot hygiene: PASS. The
  disposable exact-source tree was removed.

## OBS-117 Persistent Provider Exchange Tracing

Validation began with failing characterization tests: `cargo test trace_`
failed because `RuntimePaths::provider_trace_dir` and `CommandId::Trace` did
not yet exist. Oracle repositories were not executed or modified, and no
Cursor parity claim was made.

Focused local results:

- `cargo test provider_trace --no-fail-fast`: PASS, 16 storage, permissions,
  symlink, truncated-JSONL, retention, cap, diagnostic, activation-isolation,
  concurrent-ordering, all-protocol, retry, raw-frame, decoded-event, outcome,
  and adversarial-redaction tests, including secret-shaped JSON embedded in a
  provider-text string.
- `cargo test trace_ --no-fail-fast`: PASS, 14 matching runtime, command,
  orchestration, input, provider, and rendering tests.
- Reviewed deterministic wide/narrow snapshots cover status, list, open,
  Cancel-first deletion, and deletion failure. The provider command-palette
  snapshots intentionally include the new searchable command. No `.snap.new`
  file remains.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,379 main tests and 36 CLI tests; one
  documented manual-Chromium case ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,408 main
  tests, 118 experiment tests, and 36 CLI tests; the documented
  manual-Chromium and complete external-fixture cases ignored.
- `cargo fmt --all -- --check`,
  `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo build --release --all-features`, release help/version/service-help,
  `git diff --check`, and snapshot hygiene: PASS.
- Local arm64 release SHA-256:
  `bc363d34e5355e52c8dcc3534b848b1c7b0e0f641b79073a381010139f85929b`.

Disposable manual result:

- A synthetic-secret, credential-isolated loopback provider exercised
  headless `--trace` and TUI `/trace` status/on/off/list/open/delete. JSONL
  records contained request/attempt/raw-frame/decoded-event/outcome records,
  used owner-only permissions, omitted the synthetic credential and raw
  request ID, and survived the default-Cancel deletion path.
- No external network, user configuration/session/credential, installed
  binary, oracle/reference file, or repository content was changed.

Physical-FreeBSD results:

- Exact 863-file implementation-source archive SHA-256:
  `124e80c6a6a1865025daadfec787017cc8c2276155c46119c375036a774dfccb`.
  Git/build/oracle/reference trees and macOS extended attributes were excluded;
  the local and remote checksums match.
- `cargo test --locked provider_trace --no-fail-fast`: PASS, 16 tests.
- `cargo test --locked trace_ --no-fail-fast`: PASS, 14 tests.
- Locked default suite: PASS, 1,366 main tests and 36 CLI tests; one documented
  manual-Chromium case ignored.
- Locked all-target/all-feature suite: PASS, 1,395 main tests, 116 experiment
  tests, and 36 CLI tests; the documented manual-Chromium and complete
  external-fixture cases ignored.
- Native formatter check, strict all-feature Clippy, all-feature release build,
  help/version/service-help, snapshot hygiene, FreeBSD 15.1 x86-64 ELF
  identity, and linked-library inspection: PASS.
- FreeBSD release SHA-256:
  `794bc65e7958e88a8f95a3cf8a8d1fd9356a9ec7bcc176abd5c0b18a9dfe012a`.

The host run used unprivileged user `aji`. Its user-local `rustup` shim is a
stale Linux ELF, so native `/usr/local/bin/cargo-fmt` and `cargo-clippy` were
invoked directly; no host tool or configuration was changed.

## CTX-112–MD-116 Selected Oracle-Informed Enhancements

Validated on 2026-08-16 locally and on physical FreeBSD
15.1-RELEASE-p2 amd64. Oracle material was read-only design evidence; no
Cursor parity claim was made.

Focused results:

- `cargo test compact_`: PASS, 4 focus/interface/prompt/orchestration tests.
  Manual focus is trimmed, scalar-bounded, control-free, JSON-quoted, and
  unable to replace the six required checkpoint sections; automatic focus is
  absent.
- `cargo test notification_`: PASS, 5 config, lifecycle, exact-byte,
  picker/bare-mode, and reviewed-snapshot tests. BEL/OSC 9, tmux passthrough,
  inclusive 30-second timing, suppression, and once-only completion are
  covered.
- `cargo test session_`: PASS, 88 matching command, reset/resume,
  request/tool, usage/pricing, pager, and existing session tests.
- `cargo test markdown::tests`: PASS, 5 table-driven delimiter, conversion,
  fallback, currency/code/URL, list/table, control, and width tests.
- `cargo test exploration_`: PASS, 6 exact-registry, grouping-boundary,
  12-action-cap, stable-key, running/error, and collapsed/expanded snapshot
  tests.
- Reviewed deterministic coverage includes wide/narrow `/session`, terminal
  math, notification picker, and collapsed/expanded/running/error exploration
  states. Existing snapshots changed only where the new command or explicitly
  classified exploration tools alter presentation. No `.snap.new` remains.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,359 main tests and 36 CLI tests; one
  documented manual-Chromium case ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,388 main
  tests, 118 experiment tests, and 36 CLI tests; the documented
  manual-Chromium and complete external-fixture cases ignored.
- `cargo fmt --all -- --check`,
  `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo build --release --all-features`, release help/version,
  `git diff --check`, and snapshot hygiene: PASS.
- Local arm64 release SHA-256:
  `47a997081ae94906a2fff82420a5fe378cb526fdf3695bf96c87d4168724b86e`.

Disposable manual TUI result:

- The released binary ran with `--no-session --profile bare`, a disposable
  home/repository, and a credentialless loopback-only deterministic provider.
  It rendered `Explored` for adjacent `read_file`/`search_files` actions,
  rendered `x₂ + α` and an indented display sum, showed two tools/two model
  requests in `/session`, exposed the notification picker, refused its OSC 9
  save in bare mode, created `/compact focus on algebra`, and then showed 3/3
  checkpoint coverage and three model requests.
- The fixture received exactly three loopback requests. The config retained
  `notification = "off"`, no session file appeared, and the sample SHA-256
  remained `d9befc8e757c41072a826408a19eca45f115841f8a659c424179fc7bbb959bad`.

Physical-FreeBSD completion results:

- Exact implementation-source archive SHA-256 (created before final
  documentation reconciliation):
  `5e640069a28e45e0df7eaeec8eb75a07fe3b4ba1fb47e0bc953d7fdd515fd96e`.
  It excluded Git/build/oracle/reference/local-ignored trees and macOS xattrs.
- The first exact native focused run found one snapshot-only difference:
  `/session` estimated the platform-specific system prompt as 3404 tokens,
  versus the macOS fixture's 3403. The test fixture now normalizes only that
  occupancy number to 3400; production continues to report its native
  calculation. The final exact archive passes all five focused suites above.
- Locked default suite: PASS, 1,346 main tests and 36 CLI tests; one documented
  manual-Chromium case ignored.
- Locked all-target/all-feature suite: PASS, 1,375 main tests, 116 experiment
  tests, and 36 CLI tests; one main and one experiment case ignored as
  documented.
- Native formatter check, strict all-feature Clippy, all-feature release build,
  help/version, snapshot hygiene, FreeBSD 15.1 x86-64 ELF identity, and linked
  library inspection: PASS.
- FreeBSD release SHA-256:
  `8ff0208e7a7f2a9c9ecab60ff5792d6e41d98685e4c0bd99fda7f411f69ad1b0`.

The host run used unprivileged user `aji`, locked dependencies, and a
disposable Cargo home. The initial empty-cache offline probe stopped before
compilation; dependencies were then fetched only into that disposable cache,
and all subsequent commands were offline. No provider request, credential,
user config/session, installed binary, oracle/reference file, or permanent
FreeBSD state changed. Both partial and final host trees and the local transfer
bundle were removed.

## GIT-111 Approval-Safe `/commit` Workflow

Validated on 2026-08-15 locally and on physical FreeBSD
15.1-RELEASE-p2 amd64. No provider or network request, credential access,
runtime configuration/session mutation, installed-binary replacement, or
implementation commit was used.

Focused local results:

- Initial `/commit` visibility characterization: expected FAIL while the
  command was hidden; passed after implementation.
- `cargo test changes::tests`: PASS, 20 Git-service tests in disposable
  repositories. Coverage includes modified, added, deleted, renamed,
  untracked, mixed staged/unstaged, Unicode, spaces, leading-dash, and
  pathspec-metacharacter paths; unrelated-index and worktree preservation;
  initial commits; detached/conflict/special-state refusal; subject validation;
  shell-like text; empty and stale snapshots; exact trees; hook failure;
  timeout/process-group cleanup; and uncertain HEAD movement.
- `cargo test commit_`: PASS, 19 matching service, command, input,
  orchestration, and rendering tests.
- `cargo test working_tree_`: PASS, 3 existing `/diff` tests; its reviewed
  snapshots remain unchanged.
- Fourteen new reviewed snapshots cover wide/narrow list, diff, subject,
  confirmation, busy, error/conflict, and detached-HEAD states. No
  `.snap.new` file remains.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,332 main tests and 36 CLI tests; one
  documented manual-Chromium test ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,361 main
  tests, 118 experiment tests, and 36 CLI tests; the documented
  manual-Chromium and complete external-fixture cases ignored.
- `cargo fmt --all -- --check`,
  `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo build --release --all-features`, release help/version,
  `git diff --check`, and snapshot hygiene: PASS.
- Local release SHA-256:
  `c704629724be722749945968808248cb746324f1887d1e72e3c16560fd5060a2`.

Disposable manual TUI result:

- An 80x24 PTY used the release binary, a disposable Git repository, and an
  empty disposable home with `--no-session --profile bare`. `/commit` was
  visible/searchable; stage actions locked duplicate controls; two exact files
  were staged; `Walkthrough café` survived the default-Cancel confirmation;
  and explicit confirmation created commit `05d149d00e9b` with the expected
  parent/tree and exactly those two paths. Ctrl+C restored the alternate
  screen, the repository was clean, and the disposable home remained empty.

Physical-FreeBSD completion results:

- Exact source archive SHA-256:
  `8642f812a140ee0476c3e51e08a6f3a18c8e6c8fab3976d133aaa4f1fcf8a03d`;
  key changed-file hashes matched the local source.
- `cargo test --locked changes::tests`: PASS, 20 tests.
- Locked default suite: PASS, 1,319 main tests and 36 CLI tests; one documented
  manual-Chromium case ignored.
- Locked all-target/all-feature suite: PASS after removing 772 untracked
  AppleDouble files generated by macOS archive metadata during extraction:
  1,348 main tests, 116 experiment tests, and 36 CLI tests; one main and one
  experiment case ignored. The contaminated experiment attempt failed seven
  fixture tests because the binary `._*` extras correctly violated the exact
  UTF-8 fixture manifest; the clean exact-source rerun passed 116/116.
- Native formatter check, strict all-feature Clippy, all-feature release build,
  help/version, snapshot hygiene, FreeBSD 15.1 x86-64 ELF identity, and linked
  library inspection: PASS.
- FreeBSD release SHA-256:
  `d1ff2b32e57515efb1aaf3dede9cbdc1cae3adcbecc3991eb48bcead05b3cdfa`.

All disposable validation trees and the transfer archive were removed. The
Hermes Git surface was design inspiration only; no Cursor parity claim or
`PARITY.md` change was warranted.

## BSD-110 FreeBSD Recovery Regression Repair

Validated on 2026-08-15 locally and on physical FreeBSD
15.1-RELEASE-p2 amd64 with Rust/Cargo 1.96.1. The native run used an
unprivileged account, a disposable home/build root, offline Cargo dependencies,
and an exact archive of the BSD-110 tree with SHA-256
`06716749a4cebabeee10dbae66f9c83627eb91f468c3813e283f6efa00fc2b7b`.

Focused results:

- Recovery characterization and compatibility: PASS, 7/7 locally and on
  FreeBSD, including non-default directory modes, empty directories, exact
  final-manifest proof, V1 reads, corruption, and checkpoint reopening.
- Checkpoint verification: PASS, 4/4 locally and on FreeBSD, including the
  native `O_NOFOLLOW` symlink diagnostic.
- Worker sandbox, fixture boundary, and pricing filters: PASS locally; the
  FreeBSD all-feature suite asserts the documented macOS-only sandbox refusal.

Local completion results:

- `cargo test --no-fail-fast`: PASS, 1,308 main tests and 36 CLI tests; one
  documented manual-browser test ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,337 main
  tests, 118 experiment tests, and 36 CLI tests; one main and one experiment
  case ignored as documented.
- `cargo fmt --check`, strict all-feature Clippy, all-feature release build,
  release help/version, `git diff --check`, and snapshot hygiene: PASS.
- Local release SHA-256:
  `a8c1c5993136c5b423a92cf17c5844c41f7dda6065fc77e4197d1411f9117a43`.

Physical-FreeBSD completion results:

- Locked default suite: PASS, 1,295 main tests and 36 CLI tests; one documented
  manual-browser test ignored.
- Locked all-target/all-feature suite: PASS, 1,324 main tests, 116 experiment
  tests, and 36 CLI tests; one main and one experiment case ignored as
  documented.
- Native formatting, strict all-feature Clippy, all-feature release build,
  help/version, ELF identity, and linked-library inspection: PASS.
- FreeBSD release SHA-256:
  `da11a35941fc6924e0aa922b55602bf726b5698f8102213dbaa7fea6e7cd0fec`.

No network/provider request, credential access, runtime configuration,
installed binary, oracle/reference content, or permanent FreeBSD state was
changed. All disposable validation trees were removed.

## CFG-109 Partial-Config Agent Defaults

The exact pre-review working tree was preserved against baseline
`ce787efc4dce99d9ddb48a831bae5e45629657bc` and reconstructed byte-for-byte
before documentation edits. CFG-109 first added an omitted-`[agent]`
characterization that failed with `max_turns` 0 versus 100. After replacing the
derived Rust default with the semantic field defaults, current offline results
are:

- `cargo test config::config_store_tests -- --nocapture`: PASS, 67 tests,
  including omitted/partial agent tables, no-rewrite loading, explicit values,
  serialized production defaults, and provider snapshot behavior.
- `cargo test --no-fail-fast`: PASS, 1,308 main tests and 36 CLI tests; one
  documented manual-browser test ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,337 main
  tests, 115 experiment tests, and 36 CLI tests; the documented manual-browser
  and external-fixture tests ignored.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --release --all-features`: PASS.
- `git diff --check`, snapshot hygiene, and release `--help`/`--version`: PASS.

No provider/network request, runtime-config mutation, manual TUI comparison,
clean-Cursor observation, or physical-FreeBSD run was performed. Historical
sections below retain the command result that was true for each task at its
completion; they are not the current repository baseline.

## AGT-108 DeepSeek Delegation Target

Focused offline tests cover exact official root and `/v1` eligibility, custom
endpoint refusal, Flash=`simple` and Pro=`standard` ceilings, exact V4
cache-hit/cache-miss/output prices, complete and inconsistent usage subsets,
legacy four-card catalog compatibility, strict source parsing, and secret-free
doctor output. A mocked headless Sol parent routes one `simple` child to
`deepseek/deepseek-v4-flash`, preserves `Subagent` purpose, and makes no
external request. Full all-feature completion gates are recorded in AGT-108's
ExecPlan. The final release was atomically installed after preserving the prior
binary, and the exact Codex Sol parent was restored through the model picker.
Installed doctor reports all six cards fresh and DeepSeek
metered/standard/eligible. Authorized live run
`745e6c33-25d9-4164-afe5-ce9f88707218` made one `complexity="simple"` task
call, routed to `deepseek/deepseek-v4-flash`, forecast approximately
`$0.001156` incremental, returned `AGT108_DEEPSEEK_OK`, and completed without
fallback, retry, or credential disclosure.

## AGT-107 Browser-Authorized MiMo Token Plan Repair

Focused coverage classifies browser and manual MiMo credentials with all three
exact Token Plan endpoints, rejects key/endpoint mismatches before traffic,
proves saved-credential repair changes only billing metadata, and exercises the
saved/browser/manual TUI paths without rendering secrets. Pricing coverage
keeps the exact 0.8x UTC-window arithmetic, labels actual discounted usage as
`20% off-peak`, qualifies the PDT/PST display window, and proves forecasts stay
peak-only.

Completion and the explicitly authorized live Codex-to-MiMo acceptance results
are recorded in the AGT-107 ExecPlan. Final gates passed 1,329 main tests, 115
experiment tests, and 36 CLI tests, strict all-target/all-feature Clippy, and
the macOS release build. Live run `efae409d-c1f5-4b04-8b13-27fb27ef7fe7`
routed one simple child to `mimo/mimo-v2.5-pro` and returned
`AGT107_MIMO_OK` without fallback.

## AGT-106 macOS Inter-Provider Delegation

Focused offline coverage includes macOS MiMo socket regressions (speculative
empty connection, partial header timeout, oversize, bad host/path, invalid
ciphertext, success redirect, overall timeout), masked Token Plan TUI input and
snapshots, transactional credential/billing rollback, exact four-card pricing,
OpenAI long context, MiMo peak/off-peak/allocation boundaries, stale/parser
failure, deterministic routing, doctor output, and headless lifecycle/policy.

Mocked headless tests prove a Sol parent routes separate simple tasks to Luna
and MiMo without external traffic. They also prove shell/write/network and
unbounded named-agent grants fail before spawn, and that active asynchronous
children are cancelled/reaped with an explicit incomplete-subagents failure.

Completion commands:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --no-fail-fast
cargo build --release --all-features
```

Live OAuth, provider probes, and paid Sol/Luna/MiMo calls are forbidden for
this acceptance slice. `ca pricing refresh` is parser-covered but is not run by
the acceptance harness because it performs official-page network fetches.

## AUTH-105 MiMo Authorization Handoff Repair

Validated on 2026-08-15 on macOS and physical FreeBSD 15.1 amd64. Xiaomi's
direct published `/authorize` URL now replaces the unsuccessful
`genLoginUrl?currentPath=...` wrapper. Deterministic fixtures prove all four
published query parameters, exact X25519 SPKI/AES-GCM behavior, control/size
rejection, a single border-free URL line, byte-exact OSC 52, and tmux/screen
passthrough. Provider-auth input and reviewed wide/narrow snapshots prove that
the bordered popup no longer contains the long URL and Ctrl+Y does not claim
unobservable clipboard success.

macOS passed 1,269 applicable unit tests with one accepted manual browser ignore
plus all 36 CLI tests, formatting, strict all-target Clippy, release build, and
released help/version smokes. Physical FreeBSD passed 1,256 applicable unit
tests with the same ignore plus all 36 CLI tests, packaged native formatting,
locked strict all-target Clippy, release build, and help/version/ELF/linkage
smokes. A native PTY walkthrough automatically printed the exact direct Xiaomi
URL outside the alternate screen as one logical line without `│`; it requested
OSC 52 and was canceled before credential entry or configuration mutation.

The verified binaries are installed at `/Users/aji/.local/bin/ca` (SHA-256
`32a4f164f46109394e8e981bb9e1aba406692abd867f6d5553d97ffedecbac6c`)
and `/home/aji/bin/ca` (SHA-256
`4cf052f74bb849e09e734566087bc88a7a9cccd584355acd067014ef5cade691`).
The disposable FreeBSD build directory was removed. No provider credential,
configuration, session, oracle, or unrelated user file changed.

## AUTH-104 Automated Provider Authentication

Validated on 2026-08-14 on macOS and on physical FreeBSD 15.1 amd64 via the
`bsd` SSH host. Fixture coverage includes MiMo authorization URL construction,
X25519/AES-GCM decryption and callback/manual-input rejection; OpenAI browser
and device polling, denial/expiry, refresh and cache preservation/creation;
masked API keys; transactional probe rollback/retention/selection; canonical
and legacy credential resolution; profile isolation, ownership, modes,
symlink rejection, bounded reads, atomic replacement, deletion, and redacted
Debug; plus wide/narrow provider-picker and authentication-modal snapshots.

The final macOS suite passed 1,265 unit tests with one manual browser test
ignored and all 36 CLI tests. Formatting, strict all-target Clippy, and the
release build passed. The exact tree was copied to a disposable FreeBSD
directory: strict locked all-target Clippy and release build passed; the native
owner-matching suite passed 1,252 tests with one manual browser test ignored,
and all 36 CLI tests passed. Released FreeBSD help/version and ELF/linked-library
smokes passed. Running the portable ownership tests as root was deliberately
replaced by the owner-matching unprivileged run because root bypasses Unix mode
checks.

Existing macOS and FreeBSD Codex accounts were used without exposing
credentials. On macOS, both `--doctor --connect` and an ordinary no-session
marker request succeeded through `openai-codex` with `gpt-5.6-sol`, returning
`CA-AUTH104-OK` from a disposable config. On FreeBSD, the normal `aji` account's
owner-only cache and config produced a working Codex row in `--doctor --connect`
and an ordinary request returned exactly `CA-AUTH104-FREEBSD-OK`. The overall
FreeBSD doctor remained not-ready only because its separate direct-OpenAI row
references an absent `OPENAI_API_KEY`. MiMo, DeepSeek, and direct OpenAI live
checks were unavailable because their credentials or interactive sessions were
absent. No user provider configuration was modified.

## BSD-103 FreeBSD Regression Audit

Validated on 2026-08-14 against physical FreeBSD 15.1-RELEASE-p2 amd64 via the
`bsd` SSH host with packaged Rust/Cargo 1.96.1. The exact current working tree
was copied without `.git`, build outputs, or oracle/reference trees into a
disposable `/tmp/ca-freebsd.audit2.*` directory. Tests ran as unprivileged
`nobody` with a separate disposable home.

Native locked compilation, formatting, strict all-target Clippy, and the release
build passed. The native suite passed 1,230 applicable unit tests with one manual
browser test ignored plus all 36 CLI tests. Coverage includes descriptor-safe
workspace cleanup, signed runtime target identity, packaged `/usr/local/bin/git`
scheduled materialization, Python-backed local-model/LSP/MCP fixtures, Codex auth
cache handling, skills, scheduling, and email boundaries. The released binary's
`--help` and `--version` smokes passed; `file` identified a FreeBSD 15.1 x86-64
ELF and SHA-256 was
`27d89aef4db2f372961cbca13b4a516b97a0473d146af2794690267c8eaf0c3a`.

The local completion gates passed 1,243 unit tests with one manual browser test
ignored, all 36 CLI tests, formatting, strict all-target Clippy, and release
build. No installed binary, permanent FreeBSD configuration, session,
credential, provider request, mailbox, external service, oracle/reference file,
or project workspace was accessed or changed. The `required` sandbox remains a
macOS-only fail-closed boundary; FreeBSD process tests explicitly use the
documented `policy-only` mode.

## AUTH-061 Codex Auth Cache on FreeBSD

Validated on 2026-08-14 against the physical FreeBSD 15.1-RELEASE-p2 amd64
`bsd` host. The standard Codex cache was copied opaquely to
`/home/aji/.codex/auth.json`, owner `aji`, mode `0600`; no token value was
printed or recorded. The permanent FreeBSD `ca` configuration retains the
existing API-key provider, adds exact `openai-codex` as the default, and uses
explicit `policy-only` sandbox mode because the required macOS sandbox backend
is unavailable. Offline doctor reports Ready with that intentional warning.

Focused tests pass for owner/mode/symlink rejection, standard cache parsing,
atomic refresh preservation, namespaced account claims, exact account-header
application, Codex request options, service-tier resolution, and both supported
request-ID headers. A loopback native inspection observed both required auth
headers without retaining their values. The final native release made a real
unprivileged, no-session request using the permanent home/config and returned
exactly `FINAL_AUTH_OK`; its disposable binary SHA-256 was
`a22526d5f6c0cb26f903d462e33e9d8708e28d88ea31deb4c70ba165594c6c40`.

Local completion gates passed 1,242 unit tests with one manual browser test
ignored, all 36 CLI tests, formatting, strict Clippy, release build, and
released help/version smokes.

## BSD-102 FreeBSD Native Support

Validated on 2026-08-14 against physical FreeBSD 15.1-RELEASE-p2 amd64 via the
`bsd` SSH host with packaged Rust/Cargo 1.96.1. The source was built and tested
only from a disposable `/tmp/ca-freebsd.*` copy; `ca` was not installed and no
runtime configuration, session, credential, provider, or project workspace was
accessed. The host received the `rust` and `pkgconf` build-tool packages.

Native `cargo check --locked` passed without compile errors. The 21 focused
workspace-materialization tests passed as the unprivileged `nobody` account,
including no-follow symlink cleanup and permission-failure recovery; both
platform-prompt tests passed natively. After the physical host rebooted, the
native release build completed successfully and produced a FreeBSD 15.1 x86-64
ELF with SHA-256
`f61cceddf591b48388014c4e17b141b628198b351de5dd18142641513302ab5b`;
released `--help` and `--version` smokes passed. At that checkpoint strict native
Clippy reached five generic `nonminimal_bool` findings in in-progress
model-capability code after all FreeBSD-specific warnings were clear; BSD-103
later simplified those expressions and passed the strict gate.

The macOS completion checks passed 1,236 unit tests with one manual browser test
ignored, all 36 CLI tests, formatting, strict Clippy, release build, and released
`--help`/`--version` smokes.

## EML-101 Dedicated Draft-First Email Gateway

Validated on 2026-08-13 with Rust/Cargo 1.97.1. The focused release suite passed 29 email tests covering disabled/dedicated account validation, exact Keychain scope and redacted Debug, strict MIME/body/attachment bounds, empty allowlist and automated/self/bulk filtering before inference, case-insensitive self filtering, UID bootstrap/resynchronization and duplicate identity, crash-safe single-instance locking, hash-chain/payload drift, metadata-only request snapshots, one real loopback zero-tool provider request, deterministic threaded Drafts composition, byte-identical one-time ambiguity recovery, canonical scheduled recipients, deterministic `plutil`-linted service preview, and fake-`launchctl` install/uninstall.

Repository completion gates passed 1,235 unit tests with one accepted manual browser test ignored, 36 CLI tests, formatting, default/all-feature strict Clippy, both release builds, released help/version/email help, and zero pending snapshots. The released disposable walkthrough ran offline doctor with an empty allowlist and emitted the secret-free Drafts-only LaunchAgent preview; it created no data/session file and moved recoverably to `/Users/aji/.Trash/ca-eml101-walk-20260813` after inspection.

No live mailbox, IMAP/SMTP authentication, macOS Keychain read/write, external provider request, SMTP message, real LaunchAgent mutation, user configuration/session, installed binary, oracle/reference file, or RND-097 request/spend occurred. Those live checks remain intentionally blocked without separate explicit authorization.

## EML-100 Typed Himalaya Mailbox Tools

Completion gates passed 1,219 unit tests with one accepted manual browser test ignored, 36 CLI tests, formatting, default/all-feature strict Clippy, both release builds, and 16 release-profile fake-Himalaya tests. Focused coverage includes the current v2.0 message JSON shape, exact argv/stdin/cleared environment, immutable drafts and attachments, Bcc omission, stale/mutated/duplicate approval refusal, `DeliveryUnknown` without retry, Yolo/generic-grant refusal, digest-bound service approval, wide/narrow TUI previews, and recursive private sentinels across hooks/events/sessions/checkpoints/portable exports/request snapshots.

Validation used temporary owner-only directories and fake executables only. It made no live mailbox, SMTP, provider, webhook, or paid request; read no mailbox credential/config contents; and changed no user runtime, installed binary, Git history, oracle, or reference checkout.

## SCH-099 Durable Automation and LaunchAgent

Validated on 2026-08-13 with Rust/Cargo 1.97.1. Focused schedule coverage freezes V1 public/store/worker JSON, exercises absolute/delay/anchored interval/five-field IANA cron behavior across DST gaps and folds, skip/one-catch-up semantics, exact-confirmation disabled creation, hash-chain/legal transitions, lease capacities, pinned tracked Git materialization, exact-argv clean-environment script-only execution, complete recovery archive reconstruction and WSP cleanup, the then-inert EmailDraft variant later activated by EML-101, and deterministic fake-`launchctl` lifecycle.

Repository completion gates passed 1,195 unit tests with one accepted manual browser test ignored, 36 CLI tests, formatting, default/all-feature strict Clippy, default and experiment release builds, released help/version/schedule help, `plutil -lint` for a disposable preview, and zero pending snapshot files. Validation used disposable homes/repositories and fake executables only: no provider or webhook request, real LaunchAgent mutation, user runtime state, installed binary, source-checkout content mutation, oracle/reference write, or RND-097 reopening occurred.

## SKL-098 Agent Skills Compatibility

Validated on 2026-08-13 with Rust/Cargo 1.97.1. The focused suite covers portable roots, duplicate qualification, strict YAML and `openai.yaml`, complete resource digests, drift, link containment, inventory bounds, explicit/implicit policy, task isolation, and pure pin resolution. The full default run passed 1,180 unit tests plus 36 CLI tests with one pre-existing manual browser test ignored. `cargo fmt --check`, strict default/all-feature Clippy, and the release build passed. Released CLI smokes use a disposable HOME and local skill tree only; no provider request, credential, user skill/config/session, oracle/reference content, or external network is used.

`ca skill list/show` are offline and mutation-free. Enable/disable requires an exact instance ID and full digest and writes an owner-only atomic profile record. Local install/import and `new` first print a deterministic preview; only an exact `--confirm` publishes, publication never overwrites, and imported installations are recorded disabled.

## RND-097 Completion Results

RND-097 implementation is complete. No paid/provider request ran because the
released credential-only broker found no configured canonical OpenAI Sol/Luna
route and stopped before its first POST.

- A released fresh pilot completed preregistration, all 12 isolated fixture checks, and official price freezing. `run --authorize-max-usd 50.00` passed offline checks but stopped at canonical OpenAI broker preflight before a POST because no configured `openai` Sol/Luna route exists. Its durable report records 72 pending slots and zero settled, outstanding, or unknown USD.

| Command/check | Result |
|---|---|
| provider/usage focused checkpoint | PASS: 25 provider, 36 client, 12 stream, and 7 usage tests; compile/fmt/default strict Clippy passed at that checkpoint |
| `cargo test --features planner-executor-experiment --bin ca reasoning_experiment_protocol::tests --no-fail-fast` | PASS: 8 one-call provider/tool protocol, framing, drift, redaction, and bounds tests; feature strict Clippy is green |
| `cargo test --features planner-executor-experiment --bin ca reasoning_experiment_ledger --no-fail-fast` | PASS: 12 exact-money, price-card binding/math, transition, cap, concurrency, tamper, temp, and filesystem-safety tests |
| `cargo test --features planner-executor-experiment --bin ca-reasoning-artifact-experiment --no-fail-fast` | PASS: 112 tests, 1 intentionally ignored external fixture unit |
| `cargo test --bin ca --features planner-executor-experiment reasoning_experiment_worker::tests --no-fail-fast` | PASS: 7 broker/tool-worker tests covering one reservation/one POST/exact settlement, full-cost unknown handling, scrubbed environment, shell/`env`-wrapper refusal, typed exact-grant file reads/atomic rewrites, symlink refusal, and IPC-only OpenAI output-item retention |
| feature tool worker | PASS: 3 focused tests show the constrained sandboxed worker clears a fake parent secret and rejects both shell and `env` wrapper execution before spawn |
| WSP fixture characterization | PASS: a baseline fixture was materialized under the benchmark policy, verified with an external Cargo target, bound to a no-op recovery artifact, and explicitly cleaned |
| `cargo run --features planner-executor-experiment --bin ca-reasoning-artifact-experiment -- fixture-check --repo /Users/aji/project/ca` | PASS: all 12 WSP-materialized baseline/golden candidates; each baseline has its exact expected hidden failure and each golden passes public and hidden verification |
| `cargo test --features planner-executor-experiment --bin ca-reasoning-artifact-experiment recovery::tests --no-fail-fast` | PASS: 5 recovery-bundle/checkpoint tests covering binary/add/delete/mode round trip, corrupt-artifact non-mutation, owner-only immutable checkpoint publication, restart/reopen/revalidation, and symlink-root refusal |
| `cargo test --features planner-executor-experiment --bin ca-reasoning-artifact-experiment pilot::tests --no-fail-fast` | PASS: 12 offline preregistration/receipt/status/lifecycle tests covering exact 72-slot balance/calibration, immutable owner-only creation/load, preregistration-bound fixture receipts with input-drift rejection, read-only report/preflight behavior, duplicate trial-slot refusal, pending price/spend gates, terminal preservation of broker/tool dispatch failures, and immutable/fresh official-price-card binding |
| `cargo test --features planner-executor-experiment --bin ca-reasoning-artifact-experiment trial_state::tests --no-fail-fast` | PASS: 3 owner-only journal tests covering legal phase order, terminal ambiguous interruption, hash-chain tamper refusal, and non-mutating read-only opens |
| `cargo test --features planner-executor-experiment --bin ca-reasoning-artifact-experiment receipt_store::tests --no-fail-fast` | PASS: 3 sanitized provider/tool-receipt tests covering one-call binding, immutable publication, and request/response/client-ID/tool-argument/tool-output redaction |
| `cargo test --features planner-executor-experiment --bin ca-reasoning-artifact-experiment controller_ipc::tests --no-fail-fast` | PASS: 2 local framed-transport tests covering one complete frame and nonzero-child refusal without a provider |
| WSP lifecycle controller seam | PASS: preregistration/fixture-gated isolated workspace, durable execution boundary, safe provider/tool receipt projection, terminal interruption on ambiguous dispatch, recovery export/checkpoint, fresh-baseline reconstruction proof, phase journal, and explicit cleanup |
| sandboxed experiment child | PASS: 3 `experiment_sandbox_` tests for stdlib Cargo, parent-environment denial, and host-HOME denial; loopback denial passes separately |

The sandbox correction is now implemented: only self process identity and
`hw.pagesize_compat` are restored; public toolchain paths are read-only and
`/dev/null` is the only extra write target. Offline Cargo, parent-process
denial, loopback denial, and synthetic host-HOME denial pass. Do not broaden
process, sysctl, Mach, network, filesystem, parent-environment, or credential
access in the completed tool worker.

| `cargo run --features planner-executor-experiment --bin ca-reasoning-artifact-experiment -- preregister … && fixture-check … && freeze-price-card … && report …` | PASS in a disposable owner-only experiment: fixture receipt bound, three public OpenAI documentation sources hashed without retaining bodies, official price card frozen, and 72 slots remained pending. No configuration, credential, model endpoint, or paid request was used. |

Final completion gates also passed: 1,166 default unit tests with one accepted
manual ignore, 36 CLI tests, 47 feature reasoning integration tests,
formatting, default/all-feature strict Clippy, default and feature release
builds, help/version, and zero pending snapshots. The released controller
proved preregistration, safe execution resume, all 12 isolated fixtures,
official price freezing, explicit USD-50 authorization, and canonical-route
preflight. The paid statistical result remains unavailable and no production
routing claim is made.

## WSP-096 Completion Results

Commands run on 2026-08-12 after adding the reusable CoW disposable-workspace boundary:

| Command | Result |
|---|---|
| `cargo test workspace_materialization --no-fail-fast` | PASS: 22 manifest, clone/copy, quota, topology, descriptor-race, durable-receipt, recovery-proof, and cleanup tests |
| actual APFS characterization | PASS: same-volume regular files reported `all_cloned`; modifying the clone left source bytes unchanged |
| `cargo test --no-fail-fast` | PASS: 1152 unit tests, one existing ignored manual-Chromium test, and 36 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| released `--help` and `--version` | PASS: exit 0; `ca 0.1.0` |
| snapshot hygiene | PASS: zero pending `.snap.new` files |

The Unix implementation holds no-follow descriptors through scanning, copying, publication, reopen, and deletion. Source and independently scanned destination manifests must match; source drift prevents publication. Every observed entry participates in the bound, free-space admission reserves worst-case bytes per filesystem within the process, benchmark inputs reject all links, and scheduled inputs require an exact normalized tracked-path allowlist with only closed in-root links. Special files and regular hardlinks fail closed.

Published workspaces have owner-only durable receipts and survive handle drop. Cleanup can begin only with a private capability minted by hashing a bounded owner-only artifact outside both the workspace and private root, binding it to the workspace ID plus baseline/final manifests. Cleanup is exact-inode, retryable after partial failure, and cannot follow substituted children. SCH-099 now supplies cross-process schedule leases and Git tracked-list generation above this unchanged WSP boundary. Tests used temporary directories only; no provider traffic, credential access, user configuration/session, Git metadata, oracle/reference write, installed binary, or APFS volume snapshot occurred.

## RND-095 Completion Results

Commands run on 2026-08-12 after completing the strong-planner/cheap-executor research report and the isolated offline artifact/cost prototype:

| Command | Result |
|---|---|
| `cargo test --features planner-executor-experiment --bin ca-reasoning-artifact-experiment --no-fail-fast` | PASS: 16 strict schema, bounds, DAG/reference, existing/create path lifecycle, capability, descriptor-relative path safety, aggregate-I/O, artifact-hash, and cost tests |
| `cargo clippy --features planner-executor-experiment --bin ca-reasoning-artifact-experiment -- -D warnings` | PASS |
| released `schema` | PASS: deterministic closed JSON Schema emitted, 12,313 bytes |
| released `validate ARTIFACT --repo ROOT` | PASS: a disposable matching dependency fixture returned one stable artifact digest; changing the tracked bytes failed with exit 2 and an exact stale digest diagnostic |
| released `cost RUNS` | PASS: two trials (one success, one failure) and two date-stamped declared-rate requests totaled exactly `$3`, mean `$1.50`, dollars per success `$3` |
| `cargo test --no-fail-fast` | PASS: 1130 unit tests, one existing ignored manual-Chromium test, and 36 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| `cargo build --release --features planner-executor-experiment --bin ca-reasoning-artifact-experiment` | PASS |
| released default `--help` and `--version` | PASS: `ca 0.1.0`; default help contains no planner/artifact experiment command |
| snapshot hygiene | PASS: zero pending `.snap.new` files |

The offline `schema`, `validate`, and `cost` commands perform no provider request, command/tool execution, config/session/Keychain/memory write, or normal `ca` runtime integration. The explicit RND-097 `run`/`resume` commands are separately gated and isolated. A caller-supplied artifact still does not authenticate planner provenance, task identity, or full Git/worktree/task anchors.

The cost reducer requires five disjoint token categories plus caller-declared price-card ID, effective timestamp, and billing tier. Its totals are exact arithmetic over those declarations, not provider-invoice attestation. No live provider trial or spending was authorized, so the economic hypothesis remains unproven pending the controlled benchmark in the research report.

## PRV-094 Completion Results

Commands run on 2026-08-12 after adding provider-owned reasoning-state continuation for the canonical OpenAI, DeepSeek, and Xiaomi MiMo routes:

| Command | Result |
|---|---|
| `cargo test agent::provider::tests:: --no-fail-fast` | PASS: 23 decoder, serializer, bounds, lifecycle, and protocol tests |
| `cargo test agent::client::tests:: --no-fail-fast` | PASS: 32 client tests, including exact two-request OpenAI and DeepSeek/MiMo loopbacks plus endpoint A-to-B non-crossing |
| `cargo test provider_state --no-fail-fast` | PASS: 7 route identity, redaction, checkpoint, token estimate, request, session, and portability tests |
| focused main/scripted continuation tests | PASS: opaque state is retained without becoming visible output, and the independent scripted loop carries it into request two |
| `cargo test provider_template_catalog --no-fail-fast` | PASS: 3 catalog input/render tests |
| `cargo test provider_ --no-fail-fast` | PASS: 134 unit tests plus 2 CLI integration tests |
| `cargo test tui::render_tests --no-fail-fast` | PASS: 114 tests; reviewed provider-template mixed/filter/no-match/unavailable states at 100×30 and 60×24 |
| `cargo test --no-fail-fast` | PASS: 1130 unit tests, one existing ignored manual-Chromium test, and 36 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| released `--help` and `--version` | PASS: exit 0; `ca 0.1.0` |
| snapshot hygiene | PASS: zero pending `.snap.new` files |
| released disposable provider walkthrough | PASS: OpenAI, DeepSeek, and MiMo each completed a real two-request tool loop against literal loopback |

The OpenAI fixture proves request one asks for `reasoning.encrypted_content`, request two replays the exact completed reasoning → message → function-call sequence before `function_call_output`, and neither request uses `previous_response_id`. The DeepSeek and MiMo fixtures prove request two contains the exact whitespace/Unicode `reasoning_content`; generic Chat omits it. State is bounded (16 Responses output items; 256 KiB aggregate; depth, node, and ID caps; 256 KiB Chat reasoning), bound to the actual provider/protocol/normalized-endpoint digest, and cannot cross endpoint A to endpoint B.

Direct Session V4 persistence preserves both state variants through full loads, lazy pages, and branches while pre-change records remain append-compatible. Portable JSON/Markdown, checkpoint prompts, request snapshots, Debug, and UI snapshots omit the opaque payload. The released walkthrough used disposable homes/workspaces, blank credentials, `--no-session`, and three loopback servers; no live provider, external network, user config, Keychain item, saved session, oracle/reference file, installed binary, or opaque-state stdout/stderr output was involved.

A final adversarial review found and fixed three runner seams: plain service turns now retain state for the next service request; the production configured subagent tool passes the canonical provider ID rather than `unknown`; and cancellation that removes part of a provider tool-call set discards the affected exchange/state rather than replaying a conflicting OpenAI sequence. Each has a focused regression and the complete gates were rerun afterward.

## CFG-093 Completion Results

Commands run on 2026-08-12 after adding the searchable 40-row provider-template catalog:

| Command | Result |
|---|---|
| `cargo test provider_template --no-fail-fast` | PASS: 10 catalog/search/input/orchestration/render tests |
| `cargo test provider_ --no-fail-fast` | PASS: 121 unit tests plus 2 CLI integration tests |
| `cargo test tui::input_tests --no-fail-fast` | PASS: 65 tests |
| `cargo test tui::render_tests --no-fail-fast` | PASS: 114 tests; reviewed mixed/filter/no-match/unavailable catalogs, template editors, provider/setup footers, and command discovery at 100×30 and 60×24 |
| `cargo test --no-fail-fast` | PASS: 1106 unit tests, one existing ignored manual-Chromium test, and 36 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| released `--help` and `--version` | PASS: exit 0; `ca 0.1.0` |
| snapshot hygiene | PASS: zero pending `.snap.new` files |
| released disposable TUI walkthrough | PASS: direct filtered OpenAI opened the exact masked editor and canceled without a template save; filtered OpenAI Codex stayed inert with the complete OAuth requirement; `Shift+P` was visible in the returned provider manager |

The released walkthrough used a disposable home, a deliberately missing environment reference, and `--no-session`. Trusting only the disposable workspace was the sole config mutation; the hash stayed unchanged across both template cancel/refusal paths, no session was created, and no provider request was made. No user config, Keychain item, saved session, external provider, oracle/reference file, or installed binary was touched.

## CFG-092 Completion Results

Commands run on 2026-08-12 after adding the explicit OpenAI provider template:

| Command | Result |
|---|---|
| initial `cargo test provider_lists_offer_an_openai_template_without_replacing_custom_add --no-fail-fast` characterization | RED as intended on the missing `AddOpenAiProvider` action; it also exposed and corrected the test helper's required filter argument |
| `cargo test openai_template --no-fail-fast` | PASS: 4 exact input, config persistence, orchestration, and wide/narrow render tests |
| `cargo test provider_ --no-fail-fast` | PASS: 106 unit tests plus 2 CLI integration tests |
| `cargo test tui::input_tests --no-fail-fast` | PASS: 63 tests |
| `cargo test tui::render_tests --no-fail-fast` | PASS: 112 tests; reviewed template editor, provider manager, setup, and unclipped duplicate-refusal snapshots at 100×30 and 60×24 |
| isolated Responses probe and adapter tests | PASS: one exact `/v1/responses` provider-check test and one exact path/auth/body adapter test |
| `cargo test --no-fail-fast` | PASS: 1091 unit tests plus 36 CLI integration tests; one existing manual local-Chromium test remains ignored by the ordinary gate |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| released `--help` and `--version` | PASS: exit 0; `ca 0.1.0` |
| snapshot hygiene | PASS: zero pending `.snap.new` files |
| released disposable TUI walkthrough | PASS: setup visibly advertised `Shift+O`; the safe masked form opened with the exact template while the config hash stayed unchanged; Enter saved the exact Responses table and retained `deepseek/auto` |

The released walkthrough used a disposable home and no OpenAI credential. After save, asynchronous discovery reported the missing `OPENAI_API_KEY` locally; no provider request was made. The temporary config contained only `${OPENAI_API_KEY}`, never a key value, and the root moved recoverably to `/Users/aji/.Trash/ca-cfg092.Sj298T`. No user config, Keychain item, saved session, external provider, oracle/reference file, or installed binary was touched.

## AUD-091 Full Program Completion Audit

- 49/49 roadmap IDs resolve to exactly one row in `docs/FULL_CAPABILITY_AUDIT.md` and exactly one archived ExecPlan; no plan has an unchecked milestone or broken audit link.
- Exact public contract/default-closed tests: PASS. `ServeOp`/`ServeEvent` preserve released `ca.serve.v1` JSON; `ScheduleDefinition`, `PluginManifest`, and `AgentDependencyEdge` compile as inspectable public strict boundaries; all named shared types are present; fallback, memory, voice, and retention remain inactive by default.
- Focused service/schedule/plugin suites: PASS (21 service, 11 schedule-filtered plus released CLI, and 14 plugin/catalog-filtered plus released CLI tests).
- `cargo test --no-fail-fast`: PASS, 1087 unit tests and 36 CLI integration tests; one manual browser test remains ignored by the ordinary gate and passed explicitly under BRW-086.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- Released help/version plus default/named/bare profile and serve/attach/schedule/plugin/runtime/local/update smokes: PASS under a disposable home. The home remained empty and there were zero pending `.snap.new` files.

The smoke root moved recoverably to `/Users/aji/.Trash/ca-aud091-smoke-20260812`. The audit changed no runtime behavior beyond exact public type names/visibility, touched no oracle/reference source, and used no real config, Keychain data, sessions, provider, external service, or installed binary.

## UPD-090 Completion Results

- `cargo test self_update --no-fail-fast`: PASS, 9 focused updater tests.
- `cargo test update_help_and_status_are_local_and_do_not_create_state --test cli --no-fail-fast`: PASS.
- `cargo test --no-fail-fast`: PASS, 1085 unit tests and 36 CLI integration tests; one manual browser test remained ignored by the ordinary gate.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- Released help/version/update-status smokes under a disposable home: PASS; status created no state.

The released copied-binary walkthrough generated a disposable Ed25519 authority and current-target signed manifest, then exercised explicit trust, check, channel, pin, download, install, status, and rollback. The loopback server received exactly one manifest GET and one exact artifact GET. The copy was restored byte-for-byte, the repository release binary hash remained unchanged, retained state was owner-only, and no config or session was created. The fixture moved recoverably to `/Users/aji/.Trash/ca-upd090-walk-20260812`. No external service, user config, Keychain item, saved session, oracle/reference file, or installed binary was touched.

## LOC-089 Completion Results

Commands run on 2026-08-12 after adding explicit managed GGUF storage and llama-server supervision:

| Command | Result |
|---|---|
| initial `cargo test local_models --no-fail-fast` characterization | RED as intended: the command skeleton incorrectly accepted persistent bare-profile management |
| final `cargo test local_models --no-fail-fast`, local parser tests, and released CLI lifecycle | PASS: 9 store/download/supervisor/log/process tests plus explicit routing/conflict/no-write/import/inspect/remove coverage |
| `cargo test --no-fail-fast` | PASS: 1075 unit tests plus 35 CLI integration tests; one manual local-Chromium test remains ignored by the ordinary gate |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release help/version/local/profile smokes | PASS: root help exposes local-run flags; nested help exposes the complete lifecycle; empty list and named-profile positional prompt create no config |
| released signed runtime/GGUF lifecycle | PASS: trust, sync, install, GGUF import/inspect, one credentialless `local-ok` request, process cleanup, model/runtime removal |

The released walkthrough generated one tiny GGUF header and one disposable native loopback fake `llama-server`, placed the executable behind an Ed25519-signed EXT-088 catalog, and exercised the production required macOS sandbox. The run sent no Authorization header, left config and sessions absent, preserved the source GGUF, removed the managed model/runtime, and left no owned server process. The successful fixture moved recoverably to `/Users/aji/.Trash/ca-loc089-walk-20260812`; an earlier shell-script fake runtime exited inside the sandbox before provider traffic and its partial/debug fixtures moved to `/Users/aji/.Trash/ca-loc089-partial-20260812` and `/Users/aji/.Trash/ca-loc089-debug-20260812`. No real model/runtime/provider, external service, user config, Keychain item, saved session, oracle/reference file, or installed binary was touched.

## EXT-088 Completion Results

Commands run on 2026-08-12 after adding the explicit signed external-runtime manager:

| Command | Result |
|---|---|
| initial `cargo test external_runtime --no-fail-fast` characterization | RED as intended: bare incorrectly accepted persistent management; leading routing and redacted Debug already passed |
| final `cargo test external_runtime --no-fail-fast` plus leading-parser and released CLI filters | PASS: 10 trust/source/signature/revision/manifest/profile/staging/download/cancel/drift/rollback/remove tests, one parser test, and one CLI help/no-write test |
| `cargo test --no-fail-fast` | PASS: 1065 unit tests plus 34 CLI integration tests; one manual local-Chromium test remains ignored by the ordinary gate |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release help/version/runtime/profile smokes | PASS: root help exposes `runtime`; nested help lists the complete lifecycle; empty list creates no state |
| released signed loopback lifecycle | PASS: trust, sync, preview, install, path, update, rollback, list, removal preview, remove, and final list with exactly four GETs |

The released walkthrough generated a disposable Ed25519 key and two tiny inert random artifacts, served them from literal loopback, verified exact installed/updated/rolled-back bytes, retained two versions, then removed only that runtime. State was owner-only, no config directory or session was created, and the loopback process exited. The successful fixture moved recoverably to `/Users/aji/.Trash/ca-ext088-walk-20260812`; an initial fixture-generator attempt used the wrong local OpenSSL argument order, stopped before starting a server or invoking `ca`, and moved recoverably to `/Users/aji/.Trash/ca-ext088-partial-20260812-1786529018`. No real trust root, runtime, external network/service, config, Keychain item, saved session, oracle/reference file, or installed binary was touched.

## VOC-087 Completion Results

Commands run on 2026-08-12 after adding disabled-by-default explicit voice capture and transcription:

| Command | Result |
|---|---|
| `cargo test voice --no-fail-fast` | PASS: 22 closed-config, command/modal, stale/cancel, WAV, local-process, provider-loopback, usage, and snapshot tests |
| `cargo test --no-fail-fast` | PASS: 1054 unit tests plus 33 CLI integration tests; one manual local-Chromium test remains ignored by the ordinary gate |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release help/version/named-profile smokes | PASS in a disposable home; the initially missing named profile was not created |
| released disposable TUI walkthrough | PASS: `/voice` discovery, disabled guidance, and configured Ready modal; clean exit with no session, audio temp, or surviving process |

Automated transcription used generated WAV data, temporary local executables, fake process trees, and loopback HTTP only. The released TUI walkthrough deliberately stopped before Space in the configured Ready modal, so it requested no microphone permission, captured no audio, and made no provider request. Its exact temporary root was moved recoverably to `/Users/aji/.Trash/ca-voc087-smoke-20260812`. No real config, Keychain item, saved session, microphone input, external provider, oracle/reference file, or installed binary was touched.

## BRW-086 Completion Results

Commands run on 2026-08-12 after adding explicitly selected ephemeral local-browser automation:

| Command | Result |
|---|---|
| `cargo test --bin ca browser --no-fail-fast` plus registry/selection/workspace characterization | PASS: closed typed actions, default omission/exact inclusion, path/network preparation, fake launch/CDP/action lifecycle, cancellation re-blocking, owner/runtime cleanup, malformed authority, screenshot/download bounds, and descendant cleanup |
| `cargo test --no-fail-fast` | PASS: 1032 unit tests plus 33 CLI integration tests; one manual local-Chromium test is ignored by the ordinary gate |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release help/version/explicit-browser-selection smokes | PASS in a disposable home; no config/data paths were created |
| explicit disposable local-Chromium walkthrough | PASS: loopback navigation, bounded DOM snapshot, normalized PNG capture, close, no leaked process or temporary profile |

The automated browser fixture used an injected fake executable, literal-loopback HTTP/WebSocket CDP, temporary workspace, and no external network. The manual test used the existing `/Applications/Chromium.app` executable with a newly created temporary profile and one loopback-only page; it did not load a real browser profile or persist cookies. Smoke output moved recoverably to `/Users/aji/.Trash/ca-brw086-smoke-20260812`. No real config, Keychain item, saved session, external page/service, oracle/reference file, installed binary, or existing browser data was touched.

## PLG-085 Completion Results

Commands run on 2026-08-12 after adding local Ed25519-signed curated catalog snapshots:

| Command | Result |
|---|---|
| `cargo test --bin ca plugin --no-fail-fast` and the focused signed-catalog CLI integration | PASS: 14 trust/signature/revision/hash/traversal/symlink/profile/staging/local-source/full-tree/update/rollback tests plus one exact released CLI lifecycle |
| `cargo test --no-fail-fast` | PASS: 1019 unit tests plus 33 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release version and nested plugin-catalog help smokes | PASS; help reports `Usage: ca plugin catalog <COMMAND>` |

The released walkthrough generated a disposable Ed25519 key and two signed local catalog revisions under a temporary home. Wrong confirmation failed; both signatures and exact hashes verified; install/update stayed disabled; pre-enable skill use failed without provider traffic; rollback reverified and retained both versions; and explicit enable produced exactly one loopback request containing only the rolled-back v1 skill. Registry/catalog paths were 0700/0600 and no session was created. The fixture, including its disposable private key, moved recoverably to `/Users/aji/.Trash/ca-plg085-walk-20260812`. No real trust root, catalog/network service, config, Keychain item, saved session, oracle/reference file, external service, or installed binary was touched.

## PLG-084 Completion Results

Commands run on 2026-08-12 after adding explicit local declarative plugin bundles:

| Command | Result |
|---|---|
| `cargo test --bin ca plugin::tests` and the focused local-plugin CLI integration | PASS: 8 strict manifest/registry/digest/credential/collision/composition tests plus inert add, wrong confirmation, drift-before-traffic, skill/workflow/theme composition, disable/remove, and source preservation |
| `cargo test --no-fail-fast` | PASS: 1013 unit tests plus 32 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release help/version/plugin-help smokes | PASS |

The released CLI walkthrough used only a temporary home, local bundle/workspace, and credential-bearing loopback fake provider. Add remained disabled; list/show exposed only safe metadata; wrong confirmation failed; exact confirmation enabled the bundle; an activated plugin skill entered exactly one provider request; workflow/theme aliases resolved through existing boundaries; disable/remove left the source bundle intact; and no session was created. The inspected fixture moved recoverably to `/Users/aji/.Trash/ca-plg084-walk-20260812`. No real config, Keychain item, saved session, oracle/reference file, external service, installed binary, or native plugin code was touched.

## SCH-083 Completion Results

Commands run on 2026-08-12 after adding explicit profile-scoped scheduled tasks:

| Command | Result |
|---|---|
| `cargo test schedule::tests --no-fail-fast` and the focused scheduled prompt CLI integration | PASS: strict identifiers/routes/paths/webhooks, cron calculation, owner-only atomic stores, redacted Debug, worker authority, disabled-by-default behavior, exact loopback request, local result, and no session |
| `cargo test --no-fail-fast` | PASS: 1005 unit tests plus 31 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release help/version/schedule-help smokes | PASS |

The released CLI path used only a temporary home/workspace and credential-bearing loopback fake provider. Add wrote one disabled profile record; list did not expose prompt content; manual run sent exactly one pinned provider/model request through the isolated `Scheduled` prepared-request path, wrote one bounded `ca.schedule.result.v1` file, and created no session. No real config, Keychain, saved session, oracle/reference file, external service, system scheduler, or installed binary was touched.


## SRV-082 Completion Results

Commands run on 2026-08-12 after adding authenticated loopback WebSocket serving and `ca attach`:

| Command | Result |
|---|---|
| `cargo test service --no-fail-fast` plus the focused loopback attach CLI integration | PASS: 20 service/CLI matches plus wrong-token, Origin, busy-client, approved-tool, reconnect, binary, and oversized-frame coverage |
| `cargo test --no-fail-fast` | PASS: 997 unit tests plus 30 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release help/version/service/attach/non-loopback/profile smokes | PASS; invalid non-loopback forms exited 2 before token or listener I/O |
| isolated released listener/attach walkthrough | PASS: file authentication, wrong-token refusal, 33 ordered events, exact approval and interaction resumes, cancellation-to-idle, environment-auth reconnect, graceful listener stop, empty daemon stdout, and no saved sessions |

The released walkthrough used a disposable workspace/home, owner-only temporary token files, and a credentialless loopback Chat Completions provider. Five provider requests carried no Authorization header; neither token appeared in attach/daemon output. The approved file was absent before the exact client response, structured input reached only its matching tool result, cancellation preserved idle service state, and the environment-token reconnect restarted sequence zero. The inspected fixture moved recoverably to `/Users/aji/.Trash/ca-srv082-walk.GgCnrF`. No real provider, user config, Keychain item, token source, saved session, oracle/reference file, external service, or installed binary was touched.

## SRV-081 Completion Results

Commands run on 2026-08-12 after adding the versioned JSONL stdio service:

| Command | Result |
|---|---|
| `cargo test service --no-fail-fast` and the focused released-protocol CLI integration test | PASS: 16 matching protocol/CLI/runner tests plus one released-binary integration path |
| `cargo test --no-fail-fast` | PASS: 993 unit tests plus 29 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release help/version/profile/service-help smokes | PASS in a disposable home; missing named/bare profile paths remained absent |
| isolated released-service walkthrough | PASS: 33 ordered JSON events, five loopback requests, exact approval/interaction resumes, cancellation-to-idle, graceful shutdown, empty stderr, and no saved sessions |

The released walkthrough used a disposable workspace/home and a credentialless loopback Chat Completions provider. `write_file` could not mutate before the exact client approval; `request_user_input` resumed only after a validated choice; a hanging fifth request cancelled back to idle before shutdown. The request log contained the exact tool results, stdout remained parseable protocol JSON, and no session directory existed. The inspected fixtures moved recoverably to `/Users/aji/.Trash/ca-srv081-walk.eVP5po` and `/Users/aji/.Trash/ca-srv081-smoke.bdiovC`. No real provider, user config, Keychain item, saved session, oracle/reference file, external service, or installed binary was touched.

## TUI-080 Completion Results

Commands run on 2026-08-12 after adding profile-scoped themes and status surfaces:

| Command | Result |
|---|---|
| theme/statusline/UI-settings/config/orchestration focused suites | PASS: closed-value validation, comment-preserving profile writes, unchanged default rendering, stale-generation rejection, fail-closed sandboxing, timeout/descendant cleanup, and 22 reviewed wide/narrow snapshots |
| `cargo test --no-fail-fast` | PASS: 977 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release help/version/profile smokes | PASS in a disposable home; missing named/bare profile paths remained absent |
| isolated released-TUI walkthrough | PASS: theme preview/save/restart, status test/reorder/save/restart, periodic safe output, hidden command text, and bare preview-only refusal with an unchanged config checksum |

The custom status test used only `printf status-ok` under the production required macOS sandbox with network denied. The result stayed process-only, the command itself never appeared in manager state/output, and bare mode neither ran it nor changed the normal profile config. The inspected fixture moved recoverably to `/Users/aji/.Trash/ca-tui080-smoke.0jC8vo`. No real provider traffic, user config, Keychain item, saved session, oracle/reference file, external service, or installed binary was touched.

## TRM-079 Completion Results

Commands run on 2026-08-12 after adding user-owned durable terminals:

| Command | Result |
|---|---|
| terminal adapter/command/input/lifecycle/render focused suites | PASS: 30 terminal-filtered tests plus 14 reviewed populated/empty/unavailable/create/busy/confirmation/failure snapshots at 100×30 and 60×24 |
| `cargo test --no-fail-fast` | PASS: 957 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release help/version smoke | PASS |
| isolated released-TUI walkthrough | PASS: exact create/list/attach/detach/default-Cancel/confirmed-kill flow, terminal restoration, and untouched survivor at shutdown |

The walkthrough ran `target/release/ca --no-session` with a disposable home/workspace and fake tmux executable. Its argument log used exact `=name` targets, the attach path left and restored the alternate screen, default Enter cancelled the first kill confirmation, and the final `survivor` row remained in fake state after `ca` exited with no trailing tmux cleanup command. No session was written. The owned fixture stopped and moved recoverably to `/Users/aji/.Trash/ca-trm079-walk.eFYtFa`. No real tmux server, provider, user config, Keychain item, saved session, oracle/reference file, external service, or installed binary was touched.

## TUR-078 Completion Results

Commands run on 2026-08-12 after adding process-only soft pause:

| Command | Result |
|---|---|
| `cargo test activity`, `cargo test pause`, and pause render snapshots | PASS: typed request/reach/resume/cancel state, busy command discovery, provider/tool/finalization boundaries, and six reviewed 100×30/60×24 snapshots |
| `cargo test --no-fail-fast` | PASS: 940 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release help/version/profile smokes | PASS in a disposable home; the missing named profile remained absent |
| isolated released-TUI walkthrough | PASS: pause-before-tools held a file write and provider continuation until `/unpause`; pause-after-provider held finalization; Ctrl+C remained cancellation |

The walkthrough used `--no-session`, a disposable workspace/home, and a credentialless loopback provider. The retained tool call created `before-tool.txt` only after `/unpause`; every recorded request omitted Authorization, no session file existed, and the cancellation path could not be resumed. Owned fixtures stopped and the exact inspected root moved recoverably to `/Users/aji/.Trash/ca-tur078-walk.SWAxjP`. No real provider, user config, Keychain item, saved session, oracle/reference file, external service, or installed binary was touched.

## SES-077 Completion Results

Commands run on 2026-08-12 after repairing file undo and adding redo:

| Command | Result |
|---|---|
| `cargo test undo --no-fail-fast`, `cargo test redo --no-fail-fast`, and `cargo test file_mutation --no-fail-fast` | PASS: drift-safe history, exact transitions, capacity/clearing, command discovery, generation handling, and four typed mutation receipts |
| `cargo test --no-fail-fast` | PASS: 933 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release `--version`, `--help`, missing named-profile, and `bare` smokes | PASS; missing profile remained absent |
| isolated released-TUI walkthrough | PASS: approval-ordered create, direct edit, two undo/redo cycles, redo invalidation after a new edit, and external-drift refusal |

The released walkthrough used a disposable workspace/home and credentialless loopback Chat Completions provider under `--no-session`. The clean six-request sequence created `alpha`, edited it to `beta`, then proved exact states `alpha` → missing → `alpha` → `beta`; undoing once and making a new `gamma` edit produced `Nothing to redo`. Replacing the file externally with `external` made `/undo` report drift while preserving both the bytes and history. No receipt content entered request logs, all six requests omitted Authorization, and no session was written. The owned fixture stopped and the exact root moved recoverably to `/Users/aji/.Trash/ca-ses077-walk.Ro7XcE`. No real provider, user config, Keychain item, saved session, oracle/reference file, external service, or installed binary was touched.

## SES-076 Completion Results

Commands run on 2026-08-12 after adding lazy session-history storage and bounded resume paging:

| Command | Result |
|---|---|
| `cargo test session --no-fail-fast` | PASS: 107 matching storage, compatibility, lifecycle, paging, orchestration, input, and render tests |
| `cargo test prepending_lazy_history --no-fail-fast` | PASS: the visible transcript anchor remains stable when an older page is prepended |
| reviewed TUI snapshots | PASS: eight recent/loading/complete/failure states at 100x30 and 60x24; zero `.snap.new` files |
| `cargo test --no-fail-fast` | PASS: 923 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release `--version`, `--help`, missing named-profile, and `bare` smokes | PASS; missing profile remained absent |
| isolated released-TUI walkthrough | PASS: legacy V4 migration, bounded 64-message resume, two older-page loads, exact full-history submit, append reuse, and exact-pair archive/restore |

The released walkthrough used a disposable 130-message legacy V4 session and a credentialless loopback Chat Completions provider. The first save atomically replaced the legacy payload with storage-v1 metadata at the stable `.json` commit point and one generation-scoped JSONL log. Restart loaded 64/130 messages; two PageUp requests reached 130/130; the next submission hydrated and sent all original messages exactly once before appending two new canonical records to the same generation. Archive and restore moved the exact metadata/log pair. The fixture server stopped and the exact root moved recoverably to `/Users/aji/.Trash/ca-ses076-walk.0kIwK0`. No real provider, user config, Keychain item, saved session, oracle/reference file, external service, or installed binary was touched.

## CHG-075 Completion Results

Commands run on 2026-08-12 after adding the guided changes walkthrough:

| Command | Result |
|---|---|
| baseline `cargo test changes --no-fail-fast`, `cargo test review --no-fail-fast`, and command-registry suite | PASS: 10 change matches, 30 review matches plus one CLI integration test, and the command-registry tests |
| `cargo test walkthrough --no-fail-fast` | PASS: 16 matching capture, hash, refusal, report-validation, command, input, render, and orchestration tests |
| reviewed TUI snapshots | PASS: 14 capture/generate/complete/stale/empty/refused/failed states at 100x30 and 60x24; zero `.snap.new` files |
| `cargo test --no-fail-fast` | PASS: 909 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release `--version`, `--help`, missing named-profile, and `bare` smokes | PASS; missing profile remained absent |
| isolated released-TUI walkthrough | PASS: fresh result, exact-source stale transition, explicit refreshed result with a different source hash, and local untracked-input refusal |

The released walkthrough used a disposable Git repository/home and credentialless loopback Chat Completions provider. Its two POSTs each contained exactly two isolated messages and only `submit_change_walkthrough`; neither carried Authorization, canonical history, or the later untracked file content. The second exact diff/hash replaced the first only after explicit `Shift+R`, and the untracked refusal made no third request. No session or named-profile file was created. The owned fixture stopped and the exact root moved recoverably to `/Users/aji/.Trash/ca-chg075-walk.TOmS6p`. No real provider, user config, Keychain item, saved session, oracle/reference file, external service, or installed binary was touched.

## SKL-074 Completion Results

Commands run on 2026-08-12 after adding semantic skill suggestions:

| Command | Result |
|---|---|
| `cargo test skills --no-fail-fast` | PASS baseline: 10 unit/render plus one CLI integration test |
| `cargo test skill --no-fail-fast` | PASS final: 24 unit/render plus two CLI integration tests |
| `cargo test --no-fail-fast` | PASS: 893 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| release `--version`, `--help`, missing named-profile, and `bare` smokes | PASS; missing profile remained absent |

Eight reviewed 100x30/60x24 snapshots cover idle composer suggestions, ranked alias/related and exact-ID pagers, and no-match guidance. A released disposable 100x30 TUI walkthrough used two project skills and a credentialless loopback provider: typing surfaced canonical suggestions without traffic; `/skills` showed the ranked non-activating pager; exact activation was required; and exactly one later request contained the active body but not the inactive body. The config hash was unchanged, no session JSON was written, the fixture was stopped, and the exact root moved recoverably to `/Users/aji/.Trash/ca-skl074-walk.D3LXcD`. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched.

## RUN-073 Completion Results

Commands run on 2026-08-12 after adding explicit multi-run and best-of-N:

| Command | Result |
|---|---|
| `cargo test multi_run --no-fail-fast` | PASS: 16 matching tests covering bounded definitions, deterministic routes/costs, process-only state, command/input safety, request-local route execution, missing-route zero traffic, temporary-Git orchestration, structured judge/fusion, usage/request ownership, and rendering |
| route-runner focused regressions | PASS: per-subagent request ownership and missing request-local route before traffic |
| reviewed TUI snapshots | PASS: 14 setup/start/running/failure/cancel/ready/judged/fusion states at 100x30 and 60x24; four usage snapshots changed intentionally; zero `.snap.new` files |
| `cargo test --no-fail-fast` | PASS: 886 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS under a disposable home; named and bare model listing remained local and missing configs were not created |
| isolated released-TUI walkthrough | PASS: three exact worktrees from one HEAD, two distinct local diffs, one contained malformed-stream candidate failure, sole-schema judgment selecting candidate 2, and sole-schema source-2 fusion proposal |

The walkthrough used two credentialless loopback Chat Completions routes, a temporary home, `--no-session`, and a temporary Git repository. Its seven POSTs were two request/tool-loop pairs for successful candidates, one malformed peer request, one judgment request advertising only `report_multi_run_judgment`, and one fusion request advertising only `report_multi_run_fusion`; none carried Authorization. The main checkout and config hash remained unchanged, no session file was created, and all three branches/worktrees survived. The successful fixture root was moved recoverably to Trash after inspection; the loopback process stopped. No real provider, user config, Keychain item, saved session, oracle/reference file, external service, or installed binary was touched.

## WFL-070 Completion Results

Commands run on 2026-08-11 after adding the declarative workflow engine:

| Command | Result |
|---|---|
| `cargo test workflow --no-fail-fast` | PASS: 24 matching unit tests covering bounded TOML/JSON loading, validation, deterministic scheduling, retries/evidence, authority, persistence/forks, commands, orchestration, and rendering |
| reviewed TUI snapshots | PASS: 16 preview/clear/running/waiting-approval/paused/failed/blocked/complete states at 100×30 and 60×24; command-palette and usage snapshots updated intentionally |
| `cargo test --no-fail-fast` | PASS: 834 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` and snapshot hygiene | PASS; zero pending `.snap.new` files |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/bare/invalid-profile smoke | PASS under a disposable home; version `ca 0.1.0`, bare catalog loaded without a write, invalid names failed |
| isolated released-TUI walkthrough | PASS: a three-step branching workflow completed through four loopback requests, one scoped `read_file` grant, one successful-tool evidence gate, and bounded dependency output propagation |

The walkthrough used a credentialless loopback Chat Completions fixture and a temporary home/workspace. The first step advertised only `read_file`, its continuation preserved that step route, and both dependent no-tool steps received the successful direct dependency output under the workflow control boundary. The saved Session V4 reported `complete` with three succeeded steps and zero canonical user messages. Disposable walkthrough and smoke roots were moved recoverably to Trash, the owned loopback process was stopped, and no real provider, user config, Keychain item, saved session, oracle/reference file, or installed binary was touched.

## GOA-069 Completion Results

Commands run on 2026-08-11 after adding durable finite goal sessions:

| Command | Result |
|---|---|
| `cargo test goal --no-fail-fast` | PASS: 19 matching unit tests covering config/session compatibility, commands, isolated evaluator requests, lifecycle transitions, stale generations, limits, and usage ownership |
| reviewed TUI snapshots | PASS: 14 active/evaluating/paused/terminal/limit states at 100x30 and 60x24; command-palette and usage snapshots updated intentionally |
| `cargo test --no-fail-fast` | PASS: 810 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` and snapshot hygiene | PASS; zero pending `.snap.new` files |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/named-profile smoke | PASS under disposable homes; version `ca 0.1.0`, missing named profile remained unwritten |
| isolated released-TUI walkthrough | PASS: one main response, fresh evaluator continuation, one request-only continuation, and fresh terminal evaluation completed with exactly 36 reported provider tokens; save/resume retained exact state |
| isolated released-TUI pause walkthrough | PASS: pausing an in-flight goal produced no evaluator request and retained the main request's 11 reported tokens in the paused goal |

The completion walkthrough used a credentialless loopback Chat Completions fixture and a temporary home/workspace. Its four POSTs were main, evaluator, continuation, evaluator; evaluator requests advertised only `report_goal_evaluation`, while main requests retained the normal tool set. The saved Session V4 contained exactly two assistant messages and no evaluator prompt/report or synthetic user continuation. A separate one-request pause fixture persisted `pause_reason = user`, zero continuations, and 11 tokens. Disposable walkthrough and smoke roots were moved recoverably to Trash, the owned fixture/expect processes were stopped, and no real provider, user config, Keychain item, saved session, oracle/reference file, or installed binary was touched.

## MEM-068 Completion Results

Commands run on 2026-08-11 after adding opt-in project and user memory:

| Command | Result |
|---|---|
| memory/config/runtime/prompt/tool/TUI focused tests | PASS: 28 matching tests covering default-off/no-write behavior, profile/workspace isolation, validators, symlink refusal, bounded recall, process-only proposals, explicit acceptance, worker reload, and safe tool selection |
| reviewed TUI snapshots | PASS at 100×30 and 60×24 for populated/empty managers, editor, proposal confirmation, bare guidance, and inline failure |
| `cargo test --no-fail-fast` | PASS: 791 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` and snapshot hygiene | PASS; zero pending `.snap.new` files |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/named-profile smoke | PASS under a disposable home; version `ca 0.1.0`, missing named profile remained unwritten |
| isolated released-TUI walkthrough | PASS: proposal generation enabled explicitly, one proposal remained process-only, Cancel-first acceptance persisted it, export succeeded, and the next request received the stale/reverify-labelled recall index |

The walkthrough used a credentialless loopback Chat Completions fixture, `--no-session`, and a temporary home/workspace. Exactly three requests were made; the first advertised `propose_memory`, the second contained only its safe queued marker, and the third contained the approved recall plus stale guard. No request exposed an API-key/header field, no session file was created, and no real provider, user config, Keychain item, saved session, oracle/reference file, or installed binary was touched.

## MM-067 Completion Results

Commands run on 2026-08-11 after adding typed multimodal content and local image inspection:

| Command | Result |
|---|---|
| image/content/provider/config/request/tool focused tests | PASS: exact text-only fixtures, all three adapter image shapes, model gating, six raster formats, metadata stripping, corrupt/oversized/workspace/mismatch failures, safe persistence, and one-continuation payload lifetime |
| reviewed TUI snapshots | PASS at 100×30 and 60×24 for clipboard processing, image-tool success, and incompatible-model failure |
| `cargo test --no-fail-fast` | PASS: 768 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` and snapshot hygiene | PASS; zero pending `.snap.new` files |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/named-profile smoke | PASS under a disposable home; version `ca 0.1.0` |
| isolated released-headless walkthrough | PASS: exactly two loopback requests; the provider called `view_image`, the continuation contained one image block and its safe marker, and JSONL contained no base64 |

The walkthrough used a credentialless loopback Chat Completions fixture, `--no-session --yolo --output-format jsonl`, and the read-only oracle fixture `oracles/mimo1.png`. Its temporary home contained only the injected config and was moved recoverably to `/Users/aji/.Trash/ca-mm067-walk.BMMOEJ`; smoke output moved to `/Users/aji/.Trash/ca-mm067-list-models.out`. No real provider, user config, Keychain item, saved session, oracle/reference file, installed binary, or persistent image payload was touched.

## DOC-066 Completion Results

Commands run on 2026-08-11 after adding bounded local PDF text extraction:

| Command | Result |
|---|---|
| registry/PDF/tool focused tests | PASS: typed read-only schema, Main/Subagent/Review exposure, default and inclusive ranges, continuation metadata, 20-page span, 10,000-page document, 32-MiB file and 56-KiB text caps, encrypted/malformed/panic-safe failures, workspace escape, terminal sanitization, and embedded-file non-traversal |
| reviewed TUI snapshots | PASS at 100×30 and 60×24 for successful ranged extraction and safe encrypted-document failure |
| `cargo test --no-fail-fast` | PASS: 754 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` and snapshot hygiene | PASS; zero pending `.snap.new` files |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS under disposable homes; version `ca 0.1.0` |
| isolated released-headless walkthrough | PASS: the provider called `read_pdf` for only page 2 of a generated two-page PDF, received `SECOND WALKTHROUGH PAGE` plus truthful totals, continued once, and completed without session state |

The walkthrough used a credentialless loopback Chat Completions fixture, `--auto-review`, `--no-session`, a generated 871-byte PDF, and a temporary home/workspace. It made exactly two provider requests; the second contained one successful tool result and no embedded or unrelated page text. The temporary home retained only its injected config. Walkthrough and smoke roots moved recoverably to `/Users/aji/.Trash/ca-doc066-walk.laga8k` and `/Users/aji/.Trash/ca-doc066-smoke.Q8kLmT`. No real provider, user config, Keychain item, saved session, oracle/reference file, external PDF utility, or installed binary was touched.

## LSP-065 Completion Results

Commands run on 2026-08-11 after adding configured semantic navigation:

| Command | Result |
|---|---|
| LSP/config/registry/selection focused tests | PASS: default-empty and bounded safe config, exact schemas, disabled-profile removal, Unicode-to-UTF-16 conversion, all six fake-server request families, 100-item result cap, external URI rejection, oversized framing, timeout, and process cleanup |
| reviewed TUI snapshots | PASS at 100×30 and 60×24 for successful semantic navigation and safe external-location failure |
| `cargo test --no-fail-fast` | PASS: 748 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` and snapshot hygiene | PASS; zero pending `.snap.new` files |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS under disposable homes; version `ca 0.1.0` |
| isolated released-headless walkthrough | PASS: the first provider request advertised `lsp_definition`, the fake server initialized lazily, received `{line: 0, character: 3}` for a 1-based position after an astral scalar, returned `sample.rs:1:2`, continued the provider turn, shut down, and left no session or child process |

The walkthrough used a loopback Chat Completions fixture, `/usr/bin/python3` as an explicitly configured fake language server, `--no-session`, and a temporary home/workspace. Its LSP transcript contained exactly initialize, initialized, didOpen, definition, shutdown, and exit. The recoverable fixture and smoke roots were moved to `/Users/aji/.Trash/ca-lsp065-walkthrough.R4MsZJ` and `/Users/aji/.Trash/ca-lsp065-smoke.eSlm8Y`. No real provider, user config, Keychain item, saved session, oracle/reference file, installed binary, or downloaded executable was touched.

## MCP-064 Completion Results

Commands run on 2026-08-11 after deferring MCP tool discovery:

| Command | Result |
|---|---|
| MCP/search/selection focused tests | PASS: zero-start active catalog, exact activation, context partition, list-change invalidation, disabled/network denial, eight-server/concurrency bounds, full-catalog compatibility, schema validation, and lifecycle cleanup |
| headless CLI integration | PASS: first request advertises search but no remote schema, search activates one exact tool for the next turn, restrictive replacement makes zero MCP starts, and unapproved search exits 4 without a child |
| reviewed TUI snapshots | PASS at 100×30 and 60×24 for successful search, safe failed search, and MCP status active counts |
| `cargo test --no-fail-fast` | PASS: 739 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` and snapshot hygiene | PASS; zero pending `.snap.new` files |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: help/version were local; disposable named-profile model listing created no config/profile state |
| isolated released-headless walkthrough | PASS: three provider requests observed search-only, one exact activated remote schema, then both safe tool results; the fake MCP child initialized, listed, executed once, and closed |

The walkthrough's first prepared request contained 35 ordinary built-ins plus `search_mcp_tools` and no `mcp__` schema. The second added only `mcp__fake__echo`; the third contained the activation result and `from-deferred-mcp`. The temporary config hash remained `c5a95584c2de174d20e962cca951972dca5c442f67de9fcc72ccb6b7ae698c09`, and the temporary home contained only that injected config. The server stopped, the MCP child closed, and the exact disposable root moved recoverably to `/Users/aji/.Trash/ca-mcp064-walkthrough.CdpB1E`. No real config, Keychain item, session, oracle/reference file, external service, or installed binary was touched.

## MCP-063 Completion Results

Commands run on 2026-08-11 after adding explicit MCP OAuth:

| Command | Result |
|---|---|
| OAuth/config/store focused tests | PASS: safe config validation, exact profile/server account isolation, bounded fake/macOS/bare stores, missing-environment zero-I/O behavior, pinned discovery, PKCE callback validation, device polling outcomes, cancellation, refresh rotation, and exact disconnect |
| MCP/command/input focused tests | PASS: authenticated catalog uses only the selected profile credential, missing credentials become typed authorization-required state before MCP POST, `/mcp` actions are idle-only, and Esc cancels the generation-bound task |
| reviewed OAuth snapshots | PASS at 100×30 and 60×24 for browser progress, device progress, and safe failure; status snapshots now label OAuth without endpoints or credentials |
| `cargo test --no-fail-fast` | PASS: 733 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` and snapshot hygiene | PASS; zero pending `.snap.new` files |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: help/version were local; a disposable named-profile model listing created no config/profile state |
| isolated released-TUI walkthrough | PASS: a temporary-home loopback OAuth fixture reached `AuthorizationRequired`, displayed only the server ID and actionable `/mcp connect secure` guidance, and exited cleanly under `--no-session` |

The released walkthrough made protected-resource and authorization-server discovery requests but no MCP POST or token request. The remote body marker and all bearer/token markers were absent from the temporary home and TUI output. The loopback process was stopped and the exact disposable roots were moved recoverably to `/Users/aji/.Trash/ca-mcp063-walkthrough.5adCfI` and `/Users/aji/.Trash/ca-mcp063-smoke.BuJ23D`. Successful token persistence and authenticated MCP startup are covered hermetically with the injected fake store; the walkthrough deliberately did not open a browser or touch Keychain. No real config, Keychain item, saved session, oracle/reference file, external service, or installed binary was touched.

## MCP-062 Completion Results

Commands run on 2026-08-11 after adding opt-in MCP Streamable HTTP:

| Command | Result |
|---|---|
| MCP/config focused tests | PASS: 20 stdio/HTTP tests covering legacy decode, safe authority/header validation, zero-I/O missing references, exact project/network trust, JSON/SSE caps, success/error/redirect/timeout paths, catalog/tool/resource/reconnect/shutdown, macOS sandboxing, collisions, cancellation, and cleanup |
| headless MCP/tool-selection/bare regressions | PASS: namespaced execution and cleanup, prompt-default approval, restrictive-base non-bypass, and bare extension omission |
| reviewed MCP status snapshots | PASS at 100×30 and 60×24; rows label `stdio` or `streamable_http` and never show endpoints or header data |
| `cargo test --no-fail-fast` | PASS: 717 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` and snapshot hygiene | PASS; zero pending `.snap.new` files |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: help/version were local; bare model listing used a disposable home and created no profile/config state |
| isolated released-headless walkthrough | PASS: one environment-authenticated loopback HTTP MCP server initialized, advertised and executed `mcp__remote__echo`, returned `HTTP MCP worked` into the second provider request, and received session deletion |

The walkthrough's five MCP POSTs all used the exact `/mcp` path and resolved reference header, while the provider observed the namespaced schema on both requests. The run returned `walkthrough complete`, used `--no-session`, and left only its injected config in the temporary home. The loopback process was stopped and the exact disposable roots were moved recoverably to `/Users/aji/.Trash/ca-mcp062-walk.70Y3lE` and `/Users/aji/.Trash/ca-mcp062-smoke.1csl0O`. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched.

HTTP failure bodies, authentication challenges, endpoints, header names/values, and referenced environment contents are absent from manager Debug, status, diagnostics, snapshots, sessions, and model-visible errors. JSON responses and raw SSE events are independently capped at 1 MiB; ambient proxies, redirects, URL credentials/query/fragment data, and literal credential headers are rejected or disabled.

## FAL-061 Completion Results

Commands run on 2026-08-11 after adding explicit interactive provider fallback:

| Command | Result |
|---|---|
| fallback/config/session focused tests | PASS: closed connect/timeout/429/5xx classification; strict exact-pair/default/duplicate/self/cause validation; provider-delete and model-rename maintenance with comment preservation; Session V4 round-trip and branch filtering |
| interactive orchestration focused tests | PASS: exhausted 503 activation, OpenAI Responses backup preparation, separate usage spans, pinned tool continuation, partial-output refusal, 401/403/404/protocol refusal, backup-failure no-chain behavior, and next-turn primary restoration |
| `cargo test --no-fail-fast` | PASS: 707 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` and snapshot hygiene | PASS; zero pending `.snap.new` files |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: help/version were local; missing named profile model listing returned without creating a profile file |
| isolated released-TUI walkthrough | PASS: primary returned two bounded 503 responses, visible sanitized fallback marker named only the two exact pairs and safe cause, backup returned `OK from backup`, and `--no-session` created no session state |

The loopback request log contained exactly two `primary-model` requests to the primary Chat Completions path and one `backup-model` request to the backup path; all three preserved the tool set and contained no credential. The temporary config was the only file under its temporary home, and the disposable roots were moved recoverably to `/Users/aji/.Trash/ca-fal061-walk.kvpmMt` and `/Users/aji/.Trash/ca-fal061-smoke.3KRFkr`. Four persisted/streaming provenance snapshots were reviewed at 100×30 and 60×24. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched.

Fallback deliberately applies only to interactive main turns. Headless, scripted, verification, review, checkpoint, subagent, provider-probe, and `/btw` requests continue to target their exact configured provider/model.

## AUTH-060 Blocked Decision

On 2026-08-11 the fresh official Codex manual's Authentication and Access tokens sections were reviewed before any source work. The documented ChatGPT browser/device flow belongs to Codex CLI, the ChatGPT desktop app, and the IDE extension; Codex access tokens are limited to Codex CLI or app-server clients, and the docs direct general OpenAI API calls to Platform API keys. No supported third-party OAuth client/audience/refresh/direct-provider contract was published for `ca`.

Per AUTH-060's explicit stop condition, no Rust, config, tests, credentials, or runtime state changed and no Cargo gate was rerun for this evidence-only blocked plan. The immediately preceding PRV-059 full gate remains the repository baseline. Reopening requires an official supported contract or a separately approved Codex app-server adapter design.

## PRV-059 Completion Results

Commands run on 2026-08-11 after adding the opt-in Anthropic Messages adapter:

| Command | Result |
|---|---|
| Anthropic adapter/decoder focused tests | PASS: exact top-level system/text/tool-use/tool-result/effort bytes; byte-split thinking/text/multiple-tool deltas; cache-aware usage; valid stop, refusal, error, unknown-future, malformed, mismatched, oversized, and premature-stream behavior |
| client/probe/catalog/config focused tests | PASS: `/messages` path, stable version header, conditional `x-api-key`, no bearer auth, credentialless requests, timeout, receiver-drop cancellation, safe setup probe, Anthropic model-catalog auth, explicit config decode, and unchanged earlier adapters |
| `cargo test --no-fail-fast` | PASS: 697 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` and snapshot hygiene | PASS; zero pending `.snap.new` files |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: help/version were local; missing named profile model listing returned without creating its profile file |
| isolated rebuilt-binary walkthrough | PASS: explicit Anthropic protocol used one credentialless `/v1/messages` request, streamed reasoning plus `OK`, mapped usage, and created no session/profile file |

The loopback walkthrough captured request SHA-256 `cc0d9ce9de2d0b7569627f75d659a4f542daf7f9f239a425272971ebac6f40c3`, observed `anthropic-version: 2023-06-01`, no `x-api-key` or Authorization header, model `claude-fixture`, streaming enabled, and fixed `max_tokens = 16384`. The server stopped after one request; the root was moved recoverably to `/Users/aji/.Trash/ca-prv059-walk.GgUqhk`. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched. PRV-059 adds no TUI state, so the snapshot set stayed unchanged.

Live Anthropic traffic remains intentionally excluded. The adapter manually replays text/client-tool history and does not claim thinking-signature persistence, prompt-cache policy, cloud-host variants, hosted tools, or multimodal support.

## PRV-058 Completion Results

Commands run on 2026-08-11 after adding the opt-in OpenAI Responses adapter:

| Command | Result |
|---|---|
| adapter/decoder focused tests | PASS: exact text/function/function-output/tool/reasoning request bytes; byte-split text/reasoning/function deltas; done-value reconciliation; nested usage; completed, failed, incomplete, refusal, malformed, oversized, unknown-item, and premature-stream behavior |
| client/probe/config focused tests | PASS: `/responses` path, conditional bearer auth, credentialless requests, local non-text rejection, timeout, receiver-drop cancellation, safe setup probe, explicit/default/unsupported protocol decode, provider-edit preservation, and unchanged 25-test client contract |
| `cargo test --no-fail-fast` | PASS: 687 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: help/version were local; missing named profile model listing returned without creating its profile file |
| isolated rebuilt-binary walkthrough | PASS: explicit Responses protocol used one credentialless `/v1/responses` request, streamed reasoning plus `OK`, mapped usage, and created no session/profile file |

The loopback walkthrough captured request SHA-256 `085889359bfab7fc91047ee7046edeeb280ac6bdbd92b6784148773ab8589cc2`, observed no Authorization header, and left only its injected temporary config. The server stopped after one request; the root was moved recoverably to `/Users/aji/.Trash/ca-prv058-walk.OoQ8wS`. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched. PRV-058 adds no TUI state, so the snapshot set stayed unchanged with zero `.snap.new` files.

One pre-existing elapsed-time assertion exceeded one second only while four focused Cargo processes contended concurrently; it passed immediately alone and again in the serial full suite. Live OpenAI traffic remains intentionally excluded.

## PRV-057 Completion Results

Commands run on 2026-08-11 after isolating the historical Chat Completions wire contract behind a protocol-neutral provider adapter:

| Command | Result |
|---|---|
| provider/client/stream/config/request focused tests | PASS: exact compact body bytes, optional-field omission, assistant/tool continuation order, `/chat/completions`, conditional bearer auth, retry-body identity, typed reasoning/text/tool/finish/usage mapping, malformed/bounded streams, safe 401 body disposal, config default/explicit/unsupported behavior, edit preservation, and debug/snapshot redaction |
| existing runner characterizations | PASS: interactive main/continuation, side question, checkpoint, review, provider-backed subagent, headless/check, scripted, and provider-probe paths retain `PreparedModelRequest` plus canonical `ProviderEvent` behavior |
| `cargo test --no-fail-fast` | PASS: 676 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: help/version were local; missing named profile model listing returned without creating its profile file |
| isolated rebuilt-binary walkthrough | PASS: omitted protocol used exact `POST /v1/chat/completions`, credentialless auth omission, successful SSE completion, and no config rewrite/session creation |

The loopback walkthrough captured request SHA-256 `943acd362ead6bf46a713ecc646cdb51f45cbb0b556e23844e38249a140de1c7`, returned `OK`, kept the sole temporary config protocol-free, and created no session/profile file. The server stopped after one request; the exact temporary root was moved recoverably to `/Users/aji/.Trash/ca-prv057-walk.etUheW`. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched. PRV-057 has no visible TUI state, so the reviewed snapshot set remained unchanged and no `.snap.new` files were produced.

## HED-056 Completion Results

Commands run on 2026-08-11 after adding opt-in fresh verification and locally validated structured headless output:

| Command | Result |
|---|---|
| contract/parser/schema focused tests | PASS: flag ordering/conflicts, regular-file/no-follow reads, byte/depth/node/property/result bounds, remote-reference rejection, local-reference wrapper rewriting, local Draft 2020-12 validation, and tool-support zero-traffic failure |
| headless runner focused tests | PASS: 23 v1/v2, fresh-context, pass/fail report, sole-final, verify-only mutation refusal, repair approval/yolo, structured output, output-last-message, provider/approval/exit, and broken-pipe tests |
| loopback CLI contract tests | PASS: passing/failing check with exit 9 and validated structured output plus invalid-schema zero traffic |
| `cargo test --no-fail-fast` | PASS: 667 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/conflict smoke | PASS: all new flags documented; `--check-repair` without `--check` exited 2 without creating config |
| isolated rebuilt-binary walkthrough | PASS: unchanged v1, v2 pass/fail verification, repair blocked then explicitly authorized, valid structured output, and invalid-schema zero-I/O rejection |

The disposable credentialless loopback walkthrough observed failed verification exit 9, unauthorized repair exit 4 with no file, explicit `--yolo` repair with one executed tool, structured `{answer:42,source:"walkthrough"}` mirrored atomically to the last-message file, and invalid remote-reference schema exit 2 while the provider request count remained unchanged. No sessions were created. The server stopped cleanly; the exact walkthrough and smoke roots were moved recoverably to `/Users/aji/.Trash/ca-hed056-walk.vXbQ0x` and `/Users/aji/.Trash/ca-hed056-smoke.bwZcoZ`. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched.

## APR-055 Completion Results

Commands run on 2026-08-11 after routing interactive, headless, MCP, and provider-subagent execution through one typed rich-permission evaluator:

| Command | Result |
|---|---|
| permission/config/broker/runner-focused tests | PASS: Deny/session/Ask/Allow precedence, legacy compatibility, canonical path and symlink denial, safe command prefixes, unsafe-shell fallback, no-side-effect yolo regressions, shared MCP/subagent routing, session-grant bounds, and comment-preserving append/remove |
| `/permissions` command/input/orchestration tests | PASS: discovery, filtering, paste isolation, selection, Cancel-first removal, busy locking, bare persistence refusal, stale generations, and inline failure recovery |
| permission-manager snapshots | PASS: reviewed ten wide/narrow populated, empty, confirmation, busy, and failure states; zero pending `.snap.new` files |
| `cargo test --no-fail-fast` | PASS: 657 unit tests plus 26 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: help/version, lazy named-profile doctor JSON, and corrected JSON headless invocation completed without creating a named profile |
| isolated rebuilt-binary walkthrough | PASS: Ask over broad Allow, process-only session grant and removal, unsafe-shell prompt under allow-all, separate network approval, and headless unsafe-shell refusal with no side effect |

The disposable credentialless loopback walkthrough showed four safe manager rows (three configured and one process-only grant), kept deletion Cancel-first, removed only the session grant, and prompted again afterward. An unsafe redirection remained approval-gated under allow-all and created neither `UNSAFE` nor `DENIED`; network use retained its distinct approval. `--no-session` created no saved session, and no session grant entered TOML. Both temporary processes stopped cleanly; the exact root was moved recoverably to `/Users/aji/.Trash/ca-apr055-walk.EAQDJy`. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched.

## TOL-054 Completion Results

Commands run on 2026-08-11 after routing every model-visible tool set through one process-only restrictive resolver:

| Command | Result |
|---|---|
| resolver and runner-focused unit tests | PASS: replacement/include/exclude order, case normalization, profile bounds, model/capability/grant restrictions, built-in/MCP resolution, and unavailable-call enforcement across interactive, headless, scripted, review, checkpoint, and subagent paths |
| CLI integration tests | PASS: 26 tests, including positional prompt/attachment parsing, MCP inclusion, restrictive-base non-bypass, unknown-name zero-traffic behavior, and unchanged approval outcomes |
| tool-selection failure snapshots | PASS: reviewed wide/narrow safe failure states; zero pending `.snap.new` files |
| `cargo test --no-fail-fast` | PASS: 635 unit tests plus 26 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile and flag-order smoke | PASS: all three flags are documented; positional prompts and `@file` attachments parse on either side; a missing named profile stayed lazy |
| isolated rebuilt-binary walkthrough | PASS: exact filtered TUI request inspection, bare default request, unknown-name zero-provider-call, and approval-preservation checks |

The disposable credentialless loopback walkthrough captured `read_file` followed by `search_files` after replacement/include/exclude, with `write_file` absent. `/request` showed exactly those two schemas. Bare advertised 34 built-ins without `activate_skill`; an unknown selection exited 2 without a POST; an advertised `write_file` call exited 4 at approval and created no file. Tool flags were absent from the temporary config, and `--no-session` created no saved session. The exact root was stopped and moved recoverably to `/Users/aji/.Trash/ca-tol054-walk.cQ9jkM`. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched.

## REQ-053 Completion Results

Commands run on 2026-08-11 after routing every current model runner through a typed prepared-request boundary and adding the process-only `/request` inspector:

| Command | Result |
|---|---|
| `cargo test request --no-fail-fast` plus runner-focused probe/review/script/subagent/checkpoint tests | PASS: 27 request-filtered tests and all explicit runner characterizations |
| request-inspector snapshots | PASS: reviewed ten wide/narrow empty, populated, omitted, binary-metadata, and redacted states; zero pending snapshots |
| `cargo test --no-fail-fast` | PASS: 617 unit tests plus 25 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: help/version and lazy missing named-profile model listing returned zero without creating a profile |
| isolated rebuilt-binary TUI walkthrough | PASS: empty inspector, main plus approved tool continuation, `/btw`, and explicit provider check all showed the correct successive request kind and local/demo identity |

The credentialless loopback captured exactly four POST bodies: main and continuation retained all 35 advertised tools, while side-question and provider-check requests had no tools. Opening `/request` made no request. The bare temporary home retained only the exact initial config and no session file; the server/TUI stopped cleanly. Walkthrough, smoke, and snapshot-log artifacts were moved recoverably to `/Users/aji/.Trash/ca-req053-walk.1UidTh`, `/Users/aji/.Trash/ca-req053-smoke.egchxM`, `/Users/aji/.Trash/ca-req053-final-smoke.EOj9yA`, and `/Users/aji/.Trash/ca-req053-snap.log`. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched.

## HOK-052 Completion Results

Commands run on 2026-08-11 after replacing synchronous partial hook handling with the bounded interactive main-session lifecycle and `/hooks` repair manager:

| Command | Result |
|---|---|
| `cargo test hooks::tests --no-fail-fast` | PASS: 38 runner, config, lifecycle, process cleanup, sandbox, cancellation, and non-leakage tests |
| `cargo test hook_manager --no-fail-fast` | PASS: 6 input, orchestration, render, and real uppercase Shift-letter regressions |
| hook-manager snapshots | PASS: reviewed 16 wide/narrow populated, empty, no-match, busy, failure, cancellation, bare, and startup-blocked states; zero pending snapshots |
| `cargo test --no-fail-fast` | PASS: 606 unit tests plus 25 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: help/version and bare model listing returned zero; no config was created |
| isolated rebuilt-binary TUI walkthrough | PASS: responsive delayed startup, explicit hook test, exact successful/failed tool lifecycle order, fail-closed repair, bare isolation, and timeout descendant cleanup |

The walkthrough used only a disposable home/workspace, credentialless loopback provider, and temporary hook commands. The successful tool path logged PreToolUse then PostToolUse; the nonzero command path logged PreToolUse then PostToolUseFailure. A failing SessionStart auto-opened `/hooks`; disabling that exact entry revalidated and unblocked the composer. Bare mode listed the six configured entries, refused Test, and left the event log at exactly 18 lines. A final one-second hook test spawned a 30-second child, reported `timed out · 1005 ms`, and the recorded child PID no longer existed immediately afterward. The loopback server and TUI were stopped. The exact walkthrough, smoke, snapshot-scratch, and help-output paths were moved to `/Users/aji/.Trash/ca-hok-walk.tTUpGZ`, `/Users/aji/.Trash/ca-hok-smoke.e8EbCM`, `/Users/aji/.Trash/ca-hok-snap-new.2Fyv4v`, and `/Users/aji/.Trash/ca-hok-help.out`, where they remain recoverable. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched.

## SES-051 Completion Results

Commands run on 2026-08-11 after adding explicit saved-session retention previews and confirmed batch deletion:

| Command | Result |
|---|---|
| `cargo test session_retention --no-fail-fast` | PASS: 16 config, preview-core, command, input, orchestration, and render tests |
| `cargo test retention_batch --no-fail-fast` | PASS: 3 exact deletion, whole-batch preflight, and truthful partial-unlink tests |
| command/config/retention snapshots | PASS: reviewed 14 wide/narrow disabled, editor, empty/populated preview, busy, bare, and failure states plus the intentional command-palette row; zero pending snapshots |
| `cargo test --no-fail-fast` | PASS: 576 unit tests plus 25 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: help/version returned zero, missing named-profile doctor stayed ready without creating its profile, and bare resume was rejected with exit 2 |
| isolated rebuilt-binary TUI walkthrough | PASS: saved 30-day/keep-2 rules without deletion, reviewed three exact candidates, exercised default Cancel, then confirmed only that exact batch |

The walkthrough started with seven active sessions plus one archived fixture. Saving preserved all seven active files and the leading TOML comment. The preview identified Expired parent and Expired child as age+count candidates and Count overflow as count-only, while reporting current 1, pinned 1, and archived 1 as protected. Default Cancel left all files intact; explicit Delete permanently left only the current, pinned, and two newest eligible active sessions plus the archived session. The temporary walkthrough and smoke roots were moved to `/Users/aji/.Trash/ca-ses051-walk.TgWWt1` and `/Users/aji/.Trash/ca-ses051-smoke.cpmQaf`, where they remain recoverable. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched.

## SES-050 Completion Results

Commands run on 2026-08-10 after adding bounded session export and sanitized inert-history import:

| Command | Result |
|---|---|
| `cargo test session_portability --no-fail-fast` | PASS: 16 schema, bounds, sanitization, filesystem, command, input, and orchestration tests |
| `cargo test session_export_ --no-fail-fast` and `cargo test session_import_ --no-fail-fast` | PASS: focused overwrite, busy-state, failure, parsing, and fresh-session paths |
| portability and command-palette snapshots | PASS: reviewed wide/narrow overwrite, export/import busy, import failure, long-path/error clipping, and updated command discovery states; zero pending snapshots |
| `cargo test --no-fail-fast` | PASS: 557 unit tests plus 25 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: help/version returned zero, a missing named profile produced a ready JSON doctor report without creating runtime files, and bare resume was rejected with exit 2 |
| isolated rebuilt-binary TUI walkthrough | PASS: exported Markdown/JSON, exercised Cancel-first and confirmed atomic replacement, rejected unresolved tool state without execution, and imported fresh distinct UUIDs into default and named profile stores |

The walkthrough verified 0600 export files; the portable JSON had only `schema`, `title`, `source_provider`, `source_model`, `created_at`, and `messages`. Imported sessions received the temporary workspace and current `local/demo` selection but no parent, pin, queued inputs, or tool state. The malicious unresolved fixture named `touch NEVER`; import failed inline and no `NEVER` file was created. Default and named profiles each retained exactly one distinct imported UUID. The temporary walkthrough and smoke roots were moved to `/Users/aji/.Trash/ca-ses050.9sJAvi` and `/Users/aji/.Trash/ca-ses050-smoke.qVPLqi`, where they remain recoverable. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched.

## SES-049 Completion Results

Commands run on 2026-08-10 after adding saved-session pinning plus exact archive/restore:

| Command | Result |
|---|---|
| `cargo test session::tests --no-fail-fast` | PASS: 14 lazy-schema, stable-key, pin-ordering, UUID/traversal/collision, exact-transfer, deletion, branch, and profile tests |
| `cargo test tui::input::session_picker_tests --no-fail-fast` | PASS: 8 Active/Archive navigation, action, protection, confirmation, busy, and paste-isolation tests |
| `cargo test tui::render_tests::session_ --no-fail-fast` | PASS: 5 render tests; reviewed wide/narrow active, archive, empty, busy, confirmation, and error snapshots; zero pending snapshots |
| `cargo test tests::session_ --no-fail-fast` | PASS: 10 orchestration tests including pin reordering, exact transfer refresh, active-session rejection, failure recovery, and stale-result rejection |
| `cargo test runtime::tests --no-fail-fast` | PASS: 5 default/named/bare runtime-path tests |
| `cargo test --no-fail-fast` | PASS: 539 unit tests plus 25 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: help/version and expected bare-resume rejection; named-profile doctor emitted valid JSON, returned the expected non-ready exit, and did not create the missing profile |
| isolated rebuilt-binary TUI walkthrough | PASS: pinned an active session, archived/restored one exact inactive file, preserved a child branch and parent metadata, protected the current session, and isolated a named-profile archive |

Storage tests prove archive/restore preserves exact bytes, fails closed on destination collision, rejects traversal and UUID mismatch, and never traverses descendants. The walkthrough restored the default-profile archive with SHA-256 `db57850ad2c734df3c60a649b176175b1a5eafe4e0cbbe1c8eb88ff7aac79802`; the named profile archived only its own fixture while the three default-profile sessions remained active. The temporary walkthrough and smoke roots were moved to Trash and remain recoverable. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched.

## SES-048 Completion Results

Commands run on 2026-08-10 after adding Session V4, persistent queued drafts, and `--no-session`:

| Command | Result |
|---|---|
| focused session/queue/runtime/CLI/orchestration/input suites | PASS: lazy V1-V4 reads/upgrades, ordered queue identity, restored non-submission, failed-drain rollback, capability isolation, parser conflicts, and in-memory retry/branch behavior |
| no-session title snapshots | PASS: 2 reviewed wide/narrow states; zero pending snapshots |
| `cargo test --no-fail-fast` | PASS: 524 unit tests plus 25 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/conflict smoke | PASS (`ca 0.1.0`); `--no-session` is documented and rejects every startup resume path |
| isolated rebuilt-binary TUI walkthrough | PASS: resumed one visible V4 queued draft without provider traffic, deleted and persisted it, then used no-session save/resume guidance, 401 `/retry`, `/fork`, and `/rewind` with no session/config mutation |

The walkthrough used a credentialless loopback provider, a disposable home/workspace, a disabled fake MCP entry, one temporary skill, and a no-op SessionStart hook. Before and after the no-session run, the existing session hash stayed `23a40974df35913dcbf0e3e6cea8afbcffde2528e01f1cd19d2f3b1eaf91c6d6`, the config hash stayed `70a6d0a3cb968443f4863dd0010cd5eccb415a56781d0bba5437e4643a514bd1`, and exactly one session file remained. The loopback process was stopped and the exact temporary directory was moved to Trash, where it remains recoverable. No real config, Keychain item, saved session, oracle/reference file, external provider, or installed binary was touched.

## KEY-047 Completion Results

Commands run on 2026-08-10 after adding validated keymaps and process-only history search:

| Command | Result |
|---|---|
| focused keymap/history suites | PASS: 17 parser/config/command/input/orchestration/render tests, including fixed Ctrl+R and pending-prefix emergency controls |
| keymap/history snapshots | PASS: 12 reviewed wide/narrow manager, capture, bare, error, populated, empty, and no-match states; zero pending snapshots |
| `cargo test --no-fail-fast` | PASS: 515 unit tests plus 25 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS (`ca 0.1.0`); missing named-profile doctor did not create a profile file |
| isolated rebuilt-binary TUI walkthrough | PASS: restored a filtered draft, captured one- and two-stroke history bindings, verified restart persistence and process-only history, reset the binding, opened Diff with Ctrl+R, and observed bare-mode refusal |

The walkthrough used a credentialless loopback provider and a disposable home/workspace. The saved session contained only the two submitted main prompts and responses; searched/restored history was not separately persisted. The custom binding reloaded after restart, reset to the default, and left only an empty `[keymap]` table. The loopback process was stopped and the exact temporary directory was moved to Trash, where it remains recoverable. No real config, Keychain item, saved session, oracle/reference file, external provider, installed binary, or unrelated workspace state was touched.

## BTW-046 Completion Results

Commands run on 2026-08-10 after adding process-only side questions:

| Command | Result |
|---|---|
| `cargo test btw_ -- --nocapture` | PASS: 9 command/input/loopback/orchestration/render tests |
| protocol-complete-prefix characterization | PASS: current user and resolved tools retained; unresolved suffix trimmed without canonical mutation |
| BTW snapshots | PASS: 10 reviewed wide/narrow editing, running, complete, failure, and cancelled states; zero pending snapshots |
| `cargo test --no-fail-fast` | PASS: 498 unit tests plus 25 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS (`ca 0.1.0`) |
| isolated rebuilt-binary TUI walkthrough | PASS: completed `/btw` while a delayed main request remained active, cancelled a second BTW request, and observed the main turn complete independently |

Loopback request capture proves BTW sends exactly one frozen request with no `tools` field and excludes live partial text/reasoning, unresolved tool suffixes, hook context, pasted images, and queued drafts. Completed, 401, cancelled, stale, and closed-generation paths are typed and usage-accounted. The temporary session contained `MANUAL-MAIN` and `MAIN-ANSWER-SAVED` but neither side-question nor side-answer marker; the temporary config SHA-256 remained `d0809c2dfa530066ce5b79a9fb1896226ab85a4571ab65118917ed309b2ccc4f`. The loopback server and explicit temporary files/directories were stopped and removed. No real config, Keychain item, session, external provider, oracle file, or installed binary was touched.

## PRV-045 Completion Results

Commands run on 2026-08-10 after adding explicit credentialless local-provider discovery:

| Command | Result |
|---|---|
| `cargo test provider_discovery::tests -- --nocapture` | PASS: 8 exact-port/header/redirect/body/deadline/concurrency loopback tests |
| `cargo test provider_ -- --nocapture` | PASS: 73 provider-related unit tests plus 2 matching CLI tests |
| provider discovery snapshots | PASS: 8 reviewed wide/narrow scanning, populated, empty, and safe-failure states; provider palette wide/narrow updates reviewed; zero pending snapshots |
| `cargo test --no-fail-fast` | PASS: 488 unit tests plus 25 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: help/version, bare model listing, and missing named-profile doctor; no profile file created |
| isolated rebuilt-binary TUI walkthrough | PASS: approved-port loopback scan found two models, explicit rescan repeated the result, Enter pre-filled provider ID/base URL/model, Esc canceled back through both overlays, and `/quit` exited cleanly |

The walkthrough used only port 1234 and a temporary home. The temporary config SHA-256 was identical before and after discovery/editor cancellation, no session file was created, and the loopback server plus temporary directories were stopped and removed. No configured credential, real user config, Keychain item, saved session, oracle file, external service, or installed binary was touched.

## USG-044 Completion Results

Commands run on 2026-08-10 after adding the process-only usage and cost ledger:

| Command | Result |
|---|---|
| `cargo test usage --no-fail-fast` | PASS: 13 ledger/pricing/parser/config/command/orchestration/render tests |
| focused review/probe/subagent loopback tests | PASS: each provider request produced exactly one correctly categorized safe record |
| usage snapshots | PASS: four reviewed wide/narrow empty and populated states, including running and failed requests; zero pending snapshots |
| `cargo test --no-fail-fast` | PASS: 474 unit tests plus 25 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS: local help/version and missing named/bare profile model listing; no profile file created |
| isolated rebuilt-binary TUI walkthrough | PASS: a temporary credentialless loopback provider produced a completed request with 11 input, 2 output, 3 cached, and 1 reasoning token plus exact `$0.000031` cost; a malformed stream remained a failed unknown-usage record |

The loopback server, temporary config, and temporary session were inspected and removed. No user config, Keychain item, saved session, external provider, oracle file, or installed binary was touched.

## CTX-043 Completion Results

Commands run on 2026-08-10 after adding exact model context-limit editing:

| Command | Result |
|---|---|
| `cargo test context --no-fail-fast` | PASS: 35 context/config/command/input/orchestration/render tests |
| context-limit focused tests | PASS: 15 tests covering precedence, validation, persistence, commands, input, orchestration, and rendering |
| context-limit snapshots | PASS: 10 reviewed wide/narrow populated, inherited, bare, busy, and failure snapshots; zero pending snapshots |
| `cargo test --no-fail-fast` | PASS: 461 unit tests plus 25 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS (`ca 0.1.0`) |
| isolated rebuilt-binary TUI walkthrough | PASS: trusted only the temporary workspace record, opened and canceled `/context-limit`, reopened it, changed the threshold from 24000 to 20000, and observed the safe success status |

Coverage proves field-wise profile/exact/legacy inheritance, positive and below-window validation before writes, exact profile-versus-context-table selection, named-profile lazy creation, bare refusal, comment/capability/unrelated-TOML preservation, paste/composer isolation, busy locking, failure-in-place, and between-turn cache invalidation. The temporary home contained only a credentialless unreachable loopback provider, its temporary workspace trust record, and the edited temporary context threshold; it was inspected and explicitly removed. No provider traffic, real config, Keychain item, session, oracle file, or installed binary was touched.

## SAN-042 Completion Results

Commands run on 2026-08-10 after adding the next-launch sandbox manager:

| Command | Result |
|---|---|
| `cargo test sandbox_` | PASS: 16 command/config/input/orchestration/render and macOS sandbox tests |
| sandbox hub snapshots | PASS: 10 reviewed wide/narrow populated, empty/bare, Cancel-first confirmation, busy, and failure snapshots; zero pending snapshots |
| `cargo test --no-fail-fast` | PASS: 446 unit tests plus 25 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt help/version/profile smoke | PASS (`ca 0.1.0`) |
| isolated rebuilt-binary TUI walkthrough | PASS: opened `/sandbox` from the real command palette, canceled an `off` downgrade, verified config remained `required`, and confirmed bare mode refused persistence |

Coverage proves strict editable values with fail-closed legacy parsing, comment-preserving atomic config mutation, saved/effective separation, immutable startup mode across TUI execution paths, safe config routing, paste isolation, bounded navigation, busy locking, bare refusal, and readable wide/narrow states. The temporary home contained only a credentialless unreachable loopback provider and was explicitly removed after inspection. No provider traffic, real config, Keychain item, saved session, oracle file, or installed binary was touched.

## CFG-041 Completion Results

Commands run on 2026-08-10 after adding immutable named profiles and bare capabilities:

| Command | Result |
|---|---|
| focused profile/config/secret/session/capability tests | PASS: lazy creation/rollback, path and Keychain isolation, deletion isolation, bare no-write behavior, in-memory retry, and command guidance |
| named and bare profile snapshots | PASS: 4 reviewed wide/narrow title/config snapshots |
| profile CLI tests and temporary-home smoke checks | PASS: positional ordering, validation, `--test` rejection, missing-profile no-write model/doctor paths, bare extension suppression, help/version, and bare resume rejection |
| `cargo test --no-fail-fast` | PASS: 431 unit tests plus 25 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |

Coverage proves default-path compatibility, whole-file named config selection, first-successful-save creation and rollback, profile-scoped credentials and sessions, active-profile title/config/doctor output, and bare process-only selection/branch behavior. Temporary homes, fake secret stores, loopback providers, and temporary sessions were used; the smoke directory and output files were removed afterward.

## OUT-040 Completion Results

Commands run on 2026-08-10 after making file, job, web, generic, and headless bounds truthful:

| Command | Result |
|---|---|
| focused read/job/web/headless tests | PASS: ranges, Unicode/CRLF, long lines, independent captures, typed outcomes, ownership, loopback caps/errors/timeouts, and labelled previews |
| bounded tool and job-detail snapshots | PASS: 4 reviewed wide/narrow snapshots |
| `cargo test --no-fail-fast` | PASS: 417 unit tests plus 23 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt `--help` and `--version` | PASS (`ca 0.1.0`) |

Coverage proves the 200-line default/500-line maximum/64-KiB file contract, exact long-line and result omissions, independent 64-KiB job stream captures, shared model budget, nonzero/timeout/cancel typing, non-process foreground ownership, decoded 1-MiB WebFetch cap, safe HTTP/content/transport failures, and UTF-8-safe generic/headless clipping. Tests used temporary files, process-owned jobs, and loopback servers only.

## SES-039 Completion Results

Commands run on 2026-08-10 after adding confirmed saved-session deletion:

| Command | Result |
|---|---|
| focused store/input/orchestration tests | PASS: exact file, traversal, UUID, branch, protection, mapping, paste, cancel, busy, and failure cases |
| session picker confirmation/error snapshots | PASS: 6 reviewed wide/narrow picker/confirmation/error snapshots |
| `cargo test --no-fail-fast` | PASS: 406 unit tests plus 23 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt `--help` and `--version` | PASS (`ca 0.1.0`) |

Coverage proves active-session protection at both input and orchestration, exact storage-key/UUID deletion, no descendant cascade, Cancel-by-default, blocking busy state, filter/selection preservation, inline failure recovery, and picker paste isolation. Tests used temporary session/config roots only; no user session or runtime state was changed.

## CTX-038 Completion Results

Commands run on 2026-08-10 after adding complete request context attribution and checkpoint visibility:

| Command | Result |
|---|---|
| focused context, prompt-byte, tool-partition, orchestration, and render tests | PASS: 11 context tests plus focused integration cases |
| context dashboard and checkpoint marker snapshots | PASS: 4 reviewed wide/narrow snapshots |
| `cargo test --no-fail-fast` | PASS: 397 unit tests plus 23 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt `--help` and `--version` | PASS (`ca 0.1.0`) |

Coverage proves exact prompt-byte preservation, deterministic component attribution, MCP-free previews, prepared built-in/MCP partitioning, complete-estimate checkpoint thresholds, sanitized checkpoint inspection, and derived transcript markers without canonical-message mutation. No provider, user runtime state, oracle file, or installed binary was changed.

## CFG-036 Completion Results

Commands run on 2026-08-10 after adding the searchable config hub:

| Command | Result |
|---|---|
| focused config command/input/orchestration tests | PASS: 4 tests |
| review-model config preservation test | PASS |
| config hub render snapshots | PASS: 4 reviewed wide/narrow populated/no-match snapshots |
| `cargo test --no-fail-fast` | PASS: 387 unit tests plus 23 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |

Coverage proves filter/paste isolation, idle-only discovery, deterministic delegation, safe hub construction, and atomic review-model persistence without default/credential/comment loss. No real credential, user runtime state, oracle file, or installed binary was changed. A live terminal walkthrough was not run; deterministic wide/narrow rendering is the visual evidence.

## DIA-035 Completion Results

Commands run on 2026-08-10 after adding provider-aware diagnostics:

| Command | Result |
|---|---|
| focused doctor/command/render tests | PASS: 6 tests and 2 reviewed wide/narrow snapshots |
| focused doctor CLI tests | PASS: 3 offline/missing-config/loopback-connect tests |
| `cargo test --no-fail-fast` | PASS: 382 unit tests plus 23 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| rebuilt `--help` and `--version` | PASS (`ca 0.1.0`) |
| isolated missing-config `--doctor` | PASS: exit 1, safe failure, no config creation |

Offline diagnostics use only safe raw-config summaries and perform no provider I/O or runtime mutation. `--connect` reuses the bounded one-request provider probe and emits only typed outcomes. No real credential, user config, Keychain item, session, oracle file, or installed binary was read or changed.

## Bootstrap Results

These commands were run from the repository root while creating the harness:

| Command | Result |
|---|---|
| `cargo test` | PASS: 3 unit tests, 0 benchmarks |
| `cargo fmt --check` | FAIL: all 16 Rust source files reported formatting diffs |
| `cargo clippy --all-targets -- -D warnings` | FAIL: 5 lints treated as errors |
| `cargo build --release` | PASS |
| `./target/release/ca --help` | PASS: usage printed, exit 0 |
| `./target/release/ca --version` | FAIL/HAZARD: `--version` is not parsed; it entered provider execution and returned a 401 |

Clippy failures:

- identical branches in `src/tui/input.rs` path completion;
- manual `Option::filter` in `src/agent/tools.rs`;
- map key/value iteration when only values are used in `src/test.rs`;
- two collapsible nested `if` statements in `src/approval.rs`.

Toolchain observed for this run:

- `rustc 1.97.1`;
- `cargo 1.97.1`;
- Darwin arm64.

Update this section only with actual output. Do not erase failures until the corresponding gate passes.

## TST-001 Completion Results

Commands run on 2026-08-01 after adding the hermetic foundation:

| Command | Result |
|---|---|
| `cargo test tui::render_tests -- --nocapture` | PASS: 8 tests |
| `cargo test test::tests -- --nocapture` | PASS: 5 tests |
| `cargo test` | PASS: 16 tests total |
| targeted `rustfmt --check` on 8 changed Rust files | PASS |
| `cargo fmt --check` | FAIL: pre-existing repository-wide formatting drift remains |
| `cargo clippy --all-targets -- -D warnings` | FAIL: 4 pre-existing lints remain; the touched `src/test.rs` values-only iteration lint is fixed |
| `cargo build --release` | PASS |
| `./target/release/ca --help` | PASS; its stale `--test <url>` label remains owned by `CLI-007` |

No `.snap.new` files remained after snapshot review. The remaining clippy errors are the identical path-completion branches in `src/tui/input.rs`, manual `Option::filter` in `src/agent/tools.rs`, and two collapsible `if` statements in `src/approval.rs`.

## PRO-002 Completion Results

Commands run on 2026-08-01 after canonicalizing tool-result serialization:

| Command | Result |
|---|---|
| `cargo test agent::client::tests -- --nocapture` | PASS: 11 protocol tests |
| `cargo test test::tests -- --nocapture` | PASS: 5 fake-backend/scripted-runner tests |
| `cargo test` | PASS: 27 tests total |
| `rustfmt --edition 2021 --check --config skip_children=true src/agent/client.rs src/agent/client_tests.rs src/test.rs src/test_tests.rs` | PASS |
| `cargo fmt --check` | FAIL: pre-existing repository-wide formatting drift remains (2,907 diff lines) |
| `cargo clippy --all-targets -- -D warnings` | FAIL: the same 4 pre-existing diagnostics remain; no new diagnostic is in a PRO-002 file |
| `cargo build --release` | PASS |
| `./target/release/ca --help` | PASS; `CLI-007` still owns the stale `--test <url>` label |

The protocol suite covers one and multiple calls, deterministic ordering, denied and failed calls, empty completed output, unresolved calls, duplicate IDs, legacy `Role::Tool` state, incremental fail-before-mutation behavior, and serde-restored conversation state. The scripted and headless assembly paths reuse the same canonical append helper. The ad-hoc verification marker was `AD_HOC_VERIFY_OK tests=27 protocol=11 backend=5 snapshots=10 pending=0`; its temporary script was removed after execution.

## APR-003 Completion Results

Commands run on 2026-08-01 after implementing deterministic multi-tool approvals:

| Command | Result |
|---|---|
| `cargo test approval::tests -- --nocapture` | PASS: 45 approval queue/policy tests |
| `cargo test tests::duplicate_persistent_denial_preserves_error_status -- --nocapture` | PASS: 1 production classification regression |
| `cargo test tui::render_tests -- --nocapture` | PASS: 9 render tests |
| `cargo test` | PASS: 71 tests total |
| `cargo fmt --check` | FAIL: pre-existing repository-wide formatting drift remains |
| `cargo clippy --all-targets -- -D warnings` | FAIL: the same 4 pre-existing diagnostics remain; APR-003's newly introduced diagnostics were fixed |
| `cargo build --release` | PASS |
| `./target/release/ca --help` | PASS; the stale `--test <url>` label remains owned by `CLI-007` |
| snapshot inventory | PASS: 14 accepted snapshots, 0 pending |

The approval suite covers FIFO batches, progress accounting, reset/reuse, mixed policy classification, no unresolved empty results, denial/error status, duplicate-decision prevention, stale completion rejection, waiting/in-flight cancellation, and exactly-once continuation. The production regression verifies identical persistent denials retain `Error` status for every call ID. Approval snapshots cover `[1/1]`, `[1/3]`, and `[2/3]` at 100×30 and 60×24. No test requires network, credentials, user config/session files, or shell execution.

## UNI-004 Completion Results

Commands run on 2026-08-01 after implementing UTF-8-safe composer editing:

| Command | Result |
|---|---|
| `cargo test tui::input_tests -- --nocapture` | PASS: 18 input tests |
| `cargo test tui::render_tests -- --nocapture` | PASS: 11 render tests |
| `cargo test` | PASS: 91 tests total |
| `cargo check` | PASS |
| `cargo fmt --check` | FAIL: pre-existing repository-wide formatting drift remains |
| `cargo clippy --all-targets -- -D warnings` | FAIL: 3 pre-existing diagnostics remain; no new diagnostic in input.rs or UNI files |
| `cargo build --release` | PASS |
| `./target/release/ca --help` | PASS; the stale `--test <url>` label remains owned by `CLI-007` |
| snapshot inventory | PASS: 16 accepted snapshots, 0 pending |

The input suite covers scalar byte-boundary helpers, ASCII/emoji/combining/CJK movement, backspace, delete, insertion, multiline paste, Vim a/h/l/x/w/b, and Unicode Ctrl+W/@. The render suite covers defensive cursor clamping. Two new Unicode cursor snapshots at 100×30 and 60×24 were manually reviewed and accepted. The identical-branch clippy lint in `input.rs` was minimally fixed as explicitly permitted. No test requires network, credentials, user config/session files, or shell execution. Stage-1 spec-compliance and stage-2 code-quality reviews both PASS with no blockers. Formatting and the 3 remaining clippy findings are pre-existing baseline, not a UNI-004 PASS. No manual live TUI/provider/oracle parity was run.

## Test Inventory

The latest CFG-032 validation reported 343 unit tests plus 20 CLI integration tests, for 363 total.

- 11 canonical provider-message protocol tests under `agent::client::tests`;
- 3 exhaustive tool-registry invariant tests plus typed execution, skill activation, and independent-call regressions;
- 11 injectable headless runner tests and 18 subprocess CLI tests using loopback fake SSE/MCP providers;
- 10 local-stdio MCP trust/lazy/protocol/resource/schema/sandbox/network/cancellation/cleanup tests;
- 6 fresh-review tests covering Git targets/privacy/caps and isolated fake-provider reporting/read-tool protocol;
- 3 revision/evidence/approval/lineage work reducer tests plus 7 V1/V2/V3 session/store/branch tests;
- 6 asynchronous agent manager/limits/approval/scope/nested-inheritance/job/shutdown tests plus typed registry/session/headless coverage;
- 7 strict skill discovery/precedence/symlink/hash/catalog/budget tests plus save/restore/command/approval coverage;
- 5 structured-interaction state/secret tests, 3 orchestration ordering/exact-once tests, and 4 modal snapshots;
- 47 approval queue/policy tests plus production duplicate-denial and network-yolo regressions;
- 5 hermetic fake-backend/scripted-runner tests under `test::tests`;
- 7 temporary-root session schema/store/branch/reasoning/work-persistence tests;
- 5 pure queue ordering/editing/outcome tests plus 4 production turn-transition tests;
- 7 context checkpoint/boundary/reconstruction/threshold/config tests;
- 4 temporary-repository Git change service tests;
- 10 asynchronous job lifecycle/responsiveness/sandbox/ownership tests plus job tool/parser/approval tests;
- 17 config-store tests plus 2 provider-secret boundary/redaction/reference tests;
- 42 deterministic input tests under `tui::input_tests`;
- 39 deterministic render tests under `tui::render_tests`;
- 74 reviewed `insta` snapshots covering startup, streaming/activity, collapsed/expanded reasoning, completed tool, bounded/scrolled/no-match model-picker states, distinct Add Provider/Add Model/Edit Model forms, cursor-centered long values, MiMo/DeepSeek fallbacks, command palette, workspace roots, skill catalog, MCP status, persistent plans/tasks, asynchronous agents, session picker, timeline, queue preview/view, context dashboard, working-tree list/diff, jobs list/detail, structured interactions, single/multi-tool approval progress, and Unicode cursor positions at 100×30 and 60×24.

The Cargo tests inject `ChatBackend`, use in-memory shell history, and keep filesystem activity inside temporary directories. The six repository `tests/*.ca` scenarios are still opt-in live-provider scripts, not Cargo integration tests. `src/test.rs::run_test` retains that live entry point while its extracted runner core can now be driven hermetically.

Focused commands:

```bash
cargo test tui::input_tests -- --nocapture
cargo test tui::render_tests -- --nocapture
cargo test agent::client::tests -- --nocapture
cargo test test::tests -- --nocapture
cargo test approval::tests -- --nocapture
cargo test tests::duplicate_persistent_denial_preserves_error_status -- --nocapture
cargo test
```

Snapshot review workflow:

1. run the focused snapshot test without `INSTA_UPDATE` and inspect its `.snap.new` output;
2. reject volatile paths, current time, secrets, and accidental whitespace churn;
3. accept only reviewed snapshots, using `INSTA_UPDATE=always` on the focused test if `cargo-insta` is unavailable;
4. rerun without update mode;
5. require zero remaining `.snap.new` files.

## MOD-019 Completion Results

Commands run on 2026-08-09 after explicit model capabilities, reasoning transport, authoritative lifecycle state, and terminal-text sanitization:

| Command | Result |
|---|---|
| focused config/client/stream/lifecycle/queue/headless/command/input/session/sanitizer/orchestration tests | PASS |
| `cargo test tui::render_tests -- --nocapture` | PASS: 25 tests, 46 accepted snapshots, 0 pending |
| `cargo test` | PASS: 209 unit tests plus 15 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| `./target/release/ca --help` | PASS |
| `./target/release/ca --version` | PASS: `ca 0.1.0` |

No live provider, credentials, runtime config/session files, installed binary, live TUI, or oracle tree was used or changed. Clean Cursor comparison remains blocked under `PAR-005`.

## WSP-020 Completion Results

Commands run on 2026-08-09 after adding workspace containment, prepared approval, sandbox/network policy, and job ownership:

| Command | Result |
|---|---|
| workspace, approval, jobs, job-tool, headless, session, command/input, and orchestration focused suites | PASS |
| macOS sandbox filesystem and loopback denial/grant tests | PASS |
| `cargo test tui::render_tests -- --nocapture` | PASS: 26 tests, 48 accepted snapshots, 0 pending |
| `cargo test` | PASS: 226 unit tests plus 15 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |

Coverage includes Git-primary discovery and instruction boundaries, all built-in path/cwd resource fields, traversal/absolute/symlink/create containment, read-only roots, session branch/restore semantics, prepared deny/allow/network ordering, yolo boundaries, cross-owner job control, unsupported required mode, scoped workspace writes, outside-write denial, and loopback denial/grant. Tests used temporary roots/config homes and short-lived local processes only. No user runtime state, credentials, oracle tree, installed binary, live TUI, or external network was touched.

## SKL-021 Completion Results

Commands run on 2026-08-09 after adding strict filesystem skill discovery, activation, prompt, and persistence boundaries:

| Command | Result |
|---|---|
| `cargo test skills::tests -- --nocapture` | PASS: 7 precedence/frontmatter/size/duplicate/symlink/hash/catalog/budget tests |
| focused tool/headless/CLI/session/restore/save/command tests | PASS |
| `cargo test tui::render_tests -- --nocapture` | PASS: 27 tests, 50 accepted snapshots, 0 pending |
| `cargo test` | PASS: 243 unit tests plus 16 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |

The loopback subprocess test proves metadata-only prompts without activation, body inclusion with repeatable `--skill`, and exit 2 for an unknown ID before provider I/O. Tool tests prove mutation approval and next-turn body injection. Session tests prove ID/path/SHA-only persistence, source/fork retention, restored-workspace rediscovery, and changed-ref reconfirmation. All roots/homes/processes were temporary; no user runtime state, credentials, oracle tree, installed binary, live TUI, or external network was touched.

## MCP-022 Completion Results

Commands run on 2026-08-09 after adding sandboxed local-stdio MCP configuration, trust, discovery, execution, resources, and lifecycle management:

| Command | Result |
|---|---|
| `cargo test mcp::tests -- --nocapture` | PASS: 10 trust/lazy/stdio/pagination/call/resource/schema/redaction/sandbox/network/reconnect/cancellation/cleanup tests |
| focused config, registry, headless, command, and loopback MCP CLI suites | PASS, including default exit 4 and approved namespaced execution |
| `cargo test tui::render_tests -- --nocapture` | PASS: 28 tests, 52 accepted snapshots, 0 pending |
| `cargo test` | PASS: 257 unit tests plus 18 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |

The fake stdio server proves lazy startup, paginated discovery, calls, text-resource list/read, list-change refresh, schema rejection, filtered environment, output/stderr bounds and redaction, malformed/timeout failure, reconnect, cancellation, process-group cleanup, and required macOS sandbox filesystem/loopback denial/grant behavior. All config/workspace roots and children were temporary; no user runtime state, credentials, oracle tree, installed binary, live TUI, or external network was touched.

## PLN-023 Completion Results

Commands run on 2026-08-09 after adding Session V3 and persistent revisioned work artifacts:

| Command | Result |
|---|---|
| `cargo test work::tests -- --nocapture` | PASS: 3 CAS/evidence/user-approval/fork-lineage tests |
| `cargo test session::tests -- --nocapture` | PASS: 7 V1/V2/V3 migration, stable save, rollback, sorting, timeline, and branch tests |
| focused registry, typed tool orchestration, `/plan`/`/tasks`, and command suites | PASS |
| `cargo test tui::render_tests -- --nocapture` | PASS: 29 tests, 54 accepted snapshots, 0 pending |
| `cargo test` | PASS: 264 unit tests plus 18 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |

Coverage proves lazy same-file V1/V2-to-V3 upgrade, defaults for optional work/review/agent fields, atomic rollback, stale-revision non-mutation, successful-recorded-event evidence, user-only approval, typed plan tool application, full-fork ID regeneration/source lineage, historical omission notices, and no running-agent inheritance. All session roots were temporary; no user runtime state, credentials, oracle tree, installed binary, live TUI, or external network was touched.

## AGT-024 Completion Results

Commands run on 2026-08-09 after replacing inline task execution with the asynchronous subagent manager:

| Command | Result |
|---|---|
| `cargo test agent::subagent::manager_tests -- --nocapture` | PASS: 6 stable lifecycle, limits, messaging, approval, scope, nested inheritance, network, job, timeout, and shutdown tests |
| focused registry, session, headless, command, and manager-tool suites | PASS |
| `cargo test tui::render_tests -- --nocapture` | PASS: 30 tests, 56 accepted snapshots, 0 pending |
| `cargo test` | PASS: 271 unit tests plus 18 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |

Coverage proves stable spawn/list/await/message/cancel, concurrency queues, depth/children/turn/runtime/output bounds, summary-only default context, per-child approval isolation, exact path/command/network non-widening, registry/workspace/sandbox execution, owner-scoped job cleanup, `task` compatibility routing, headless fail-closed behavior, interrupted summary persistence, and branch/shutdown non-resumption. Provider-network execution remains intentionally unrun without credentials; protocol consumption uses the already-tested shared assembler. No user runtime state, credentials, oracle tree, installed binary, live TUI, or external network was touched.

## REV-025 Completion Results

Commands run on 2026-08-09 after adding fresh-context model review:

| Command | Result |
|---|---|
| focused `changes`, `review`, registry, command, provider-selection, and session tests | PASS: working/base/commit/custom, unsafe refs, 1 MiB file boundaries, untracked privacy, isolated fake-provider messages, read-tool continuation, structured reports, persistence, and fail-closed config/protocol behavior |
| `cargo test tui::render_tests -- --nocapture` | PASS: 31 tests, 58 accepted snapshots, 0 pending |
| `cargo test --no-fail-fast` | PASS: 281 unit tests plus 18 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |

The reviewer receives a newly constructed two-message context containing only project instructions and bounded review data; it never receives canonical chat, checkpoints, plans, skills, MCP state, or agents. Its advertised tools are read-only file/Git helpers and the sole structured reporter. Temporary repositories and injected fake providers supplied all automated coverage. No live provider, credentials, user runtime state, oracle tree, installed binary, or running TUI was touched, and no Cursor parity claim is made.

## CFG-026 Completion Results

Commands run on 2026-08-10 after adding the provider/model manager:

| Command | Result |
|---|---|
| `cargo test config_store_tests --no-fail-fast` | PASS: 10 temp-root parsing, placeholder, atomic patch, provider/model validation, default, profile/context migration, and compatibility tests |
| `cargo test tui::input_tests --no-fail-fast` | PASS: 33 tests, including typed add/edit actions and isolated form navigation/save/cancel behavior |
| focused provider-editor orchestration test | PASS: inline error retention, successful config reload, immediate list/default/current selection refresh, and unresolved-key preservation |
| `cargo test tui::render_tests --no-fail-fast` | PASS: 32 tests, 60 accepted snapshots, 0 pending |
| `cargo test --no-fail-fast` | PASS: 302 unit tests plus 20 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |

Coverage proves picker discovery, filtered edit targeting, keyboard-only field editing, cancel/error behavior, add-to-existing and add-new-provider paths, environment-variable-name validation, comment and unresolved-placeholder preservation, model-profile/context key migration, atomic default selection, and post-save runtime refresh. All config roots were temporary; no user config/session, credentials, oracle tree, installed binary, provider network, or external service was touched. Wide and narrow snapshots were manually reviewed; no Cursor parity claim is made.

## CFG-027 Completion Results

Commands run on 2026-08-10 after adding live provider model discovery:

| Command | Result |
|---|---|
| `cargo test model_catalog::tests --no-fail-fast` | PASS: 3 loopback authentication, sorting/deduplication, partial failure, missing-key, malformed JSON, and 1 MiB cap tests |
| `cargo test config_store_tests --no-fail-fast` | PASS: 11 tests, including atomic discovered-model fallback/default persistence with placeholder preservation |
| `cargo test tui::input_tests --no-fail-fast` | PASS: 35 tests, including first-open/explicit refresh and deterministic configured/live merge ordering |
| focused catalog orchestration test | PASS: multiple live rows, offline fallback, sanitized unavailable-provider status, and live-only selection persistence |
| `cargo test tui::render_tests --no-fail-fast` | PASS: 33 tests, 62 accepted snapshots, 0 pending |
| `cargo test --no-fail-fast` | PASS: 310 unit tests plus 20 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |

The catalog boundary uses at most 8 concurrent provider requests, a 10-second timeout, a 1 MiB response cap, 2,048 IDs per provider, 256 bytes per validated ID, bearer authentication, no response-body error display, and terminal sanitization. Provider successes merge with configured models; failures preserve configured and prior successful fallback entries. Automated network checks used loopback only. A credential-safe live request to Xiaomi's configured endpoint was attempted, but `XIAOMI_API_KEY` was unavailable in the validation process, so no Xiaomi live-catalog result is claimed. No user config/session, credentials, oracle tree, installed binary, or live TUI was touched.

## CFG-028 Completion Results

Commands run on 2026-08-10 after recovering MiMo selection from the observed zero-live catalog state:

| Command | Result |
|---|---|
| `cargo test model_catalog --no-fail-fast` | PASS: 5 catalog tests plus one similarly named skill test; MiMo documented fallback, unrelated-provider exclusion, loopback auth, partial failure, missing key, malformed JSON, and response bounds covered |
| `cargo test config_store_tests --no-fail-fast` | PASS: 12 tests, including legacy `${XIAOMI_API_KEY}` to `MIMO_API_KEY` runtime compatibility without raw-config rewriting |
| focused live and failed catalog orchestration tests | PASS: live merge/persistence and zero-live MiMo fallback both covered |
| `cargo test tui::render_tests --no-fail-fast` | PASS: 34 tests, 64 accepted snapshots, 0 pending |
| `cargo test --no-fail-fast` | PASS: 315 unit tests plus 20 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |

New configs use the official `MIMO_API_KEY` reference and include both MiMo V2.5 chat models. Existing configs retain their raw legacy placeholder but can resolve the official name at runtime. When live discovery fails, the picker merges the documented `mimo-v2.5` and `mimo-v2.5-pro` fallback and reports its source separately from live results. Speech-only MiMo IDs are intentionally excluded because this client sends chat-completion requests. Wide and narrow zero-live snapshots were reviewed. No live Xiaomi request, user config/session, credentials, oracle tree, installed binary, or running TUI was touched.

## CFG-029 Completion Results

Commands run on 2026-08-10 after completing provider-wide picker catalogs:

| Command | Result |
|---|---|
| `cargo test model_catalog --no-fail-fast` | PASS: 7 catalog tests plus one similarly named skill test; current DeepSeek/MiMo fallback, public Ofox auth/filtering, unrelated-provider exclusion, loopback bearer auth, partial failure, malformed JSON, and bounds covered |
| `cargo test config_store_tests --no-fail-fast` | PASS: 13 tests, including raw-preserving runtime migration from discontinued DeepSeek aliases |
| focused catalog orchestration tests | PASS: live merge/persistence plus zero-live MiMo and DeepSeek fallbacks covered |
| `cargo test tui::render_tests --no-fail-fast` | PASS: 35 tests, 66 accepted snapshots, 0 pending |
| public `https://api.ofox.ai/v1/models` check | PASS: 129 current IDs returned; 100 advertise text output plus tool support and are suitable for this agent's picker |
| `cargo test --no-fail-fast` | PASS: 320 unit tests plus 20 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |

New configs seed `deepseek-v4-flash` and `deepseek-v4-pro`; discontinued official aliases are removed from runtime selection without rewriting raw TOML. Ofox's exact official host uses its documented public model-list endpoint without sending an Authorization header, and its extended metadata filters out non-text and no-tool models. MiMo retains the CFG-028 fallback. Ollama/custom providers remain driven only by configured and live evidence. No credential-dependent DeepSeek/MiMo request, user config/session, oracle tree, installed binary, or running TUI was touched.

## CFG-030 Completion Results

Commands run on 2026-08-10 after making the populated picker operable and its state visible:

| Command | Result |
|---|---|
| `cargo test tui::input_tests --no-fail-fast` | PASS: 38 tests covering advertised shortcut/composer preservation, current-row opening, bounded/page/jump navigation, 100-row selection, and no-match behavior |
| focused provider-editor/catalog orchestration and error-safety tests | PASS: discovered-only add-to-provider setup, immediate live-selection editability, filtered refresh selection alignment, natural status counts, and URL-safe failure text |
| `cargo test tui::render_tests --no-fail-fast` | PASS: 37 tests, 70 accepted snapshots, 0 pending; current/provider status, no-match, and 100-row scroll states manually reviewed at wide/narrow sizes |
| `cargo test --no-fail-fast` | PASS: 329 unit tests plus 20 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |
| isolated rebuilt-binary TUI check | PASS: trusted only a temporary home, opened with real `Ctrl+K`, displayed six choices including `mimo-v2.5-pro` and `deepseek-v4-pro`, selected MiMo Pro, and observed the active header/status update |

The picker retains a flat searchable catalog, but selection is relative to the filtered result set and bounded at every key path. A fixed visible window keeps controls available for provider catalogs with 100 or more models. The navigation cursor and active model have distinct cues; the selected provider reports configured/live/fallback/unavailable state; no-match copy keeps Add discoverable. Discovered-only Edit opens a valid prefilled add draft, and catalog errors are classified before rendering. Automated tests used in-memory or temporary state; the interactive check used and removed an isolated temporary home. Opening the picker exercised the ordinary public Ofox refresh path; no credential-dependent request, user runtime config/session, oracle tree, or installed binary was used or changed.

## CFG-031 Completion Results

Commands run on 2026-08-10 after separating provider/model creation and making editor fields genuinely editable:

| Command | Result |
|---|---|
| `cargo test config_store_tests --no-fail-fast` | PASS: 14 tests, including exact safe environment-reference extraction with literal/composite key hiding |
| `cargo test tui::input_tests --no-fail-fast` | PASS: 41 tests covering contextual Add Model, separate Add Provider, cursor editing, UTF-8 boundaries, field traversal, and picker/editor paste isolation |
| focused provider-editor orchestration tests | PASS: selected-provider/base/environment prefilling, add/edit/discovered-only saves, inline errors, atomic placeholder preservation, success copy, and current/default refresh |
| `cargo test tui::render_tests --no-fail-fast` | PASS: 39 tests, 74 accepted snapshots, 0 pending; Add Provider/Add Model/Edit Model, cursor, long-value, picker action, and no-match states manually reviewed at wide/narrow sizes |
| `cargo test --no-fail-fast` | PASS: 336 unit tests plus 20 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |
| isolated rebuilt-binary TUI check | PASS: selected MiMo, opened “Add model to mimo,” observed the prefilled endpoint and `MIMO_API_KEY`, moved the visible cursor into the endpoint, inserted a character in the middle, and canceled without saving |

`Shift+A` now uses the selected/current provider and focuses an empty Model ID; `Shift+P` opens the full new-provider form. The raw-config lookup returns only exact syntactically valid `${ENV_VAR}` names. Editor fields support Unicode-safe cursor movement/insertion/deletion and a cursor-centered long-value window. Paste follows overlay precedence for the picker filter and editor field, leaving the chat composer unchanged. All automated state was in memory or temporary roots, and the isolated TUI home was removed. Its ordinary public Ofox refresh timed out and surfaced the classified status safely; no credential-dependent request, user runtime config/session, oracle tree, or installed binary was used or changed.

## CFG-032 Completion Results

Commands run on 2026-08-10 after adding secure direct API-key entry:

| Command | Result |
|---|---|
| `cargo test provider_secrets --no-fail-fast` | PASS: 2 redaction/Unicode/reference tests |
| `cargo test config_store_tests --no-fail-fast` | PASS: 17 tests, including injected Keychain resolution, missing-item fail-closed behavior, raw-config exclusion, and rollback after storage failure |
| `cargo test tui::input_tests --no-fail-fast` | PASS: 42 tests, including masked paste, environment/direct source switching, action/debug exclusion, and save-in-progress input locking |
| focused provider-editor orchestration tests | PASS: environment and injected direct-Keychain saves both reload/select; raw config contains only safe references |
| `cargo test tui::render_tests --no-fail-fast` | PASS: 39 tests, 74 accepted snapshots, 0 pending; picker edit/key copy and masked Add Provider/Add Model/Edit Model forms reviewed at wide/narrow sizes |
| `cargo test --no-fail-fast` | PASS: 343 unit tests plus 20 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |
| isolated rebuilt-binary TUI check | PASS: opened the real picker/Add Provider flow, pasted a non-secret marker through bracketed paste, observed only a fixed eight-dot mask and Keychain copy, then canceled before save |

Direct API-key input is held in a host-only redacted/zeroizing buffer and never enters the typed action, status copy, snapshots, or raw config. Successful macOS saves use a provider-scoped generic-password Keychain item and persist only `keychain://ca/provider/<provider>`; environment-variable names remain supported. Blocking Keychain/config work runs outside the TUI event loop, and a storage failure atomically restores the prior config while retaining masked input for retry. Automated persistence tests use an injected fake store. The manual temporary home was inspected, contained no direct-key/provider save, and was removed. A real default-Keychain save/read and credential-dependent provider authentication were intentionally not run; no user config/session, Keychain item, oracle tree, or installed binary was changed.

## CFG-033 Completion Results

Commands run on 2026-08-10 after adding the provider-first management hub:

| Command | Result |
|---|---|
| `cargo test config_store_tests --no-fail-fast` | PASS: safe zero-model snapshots, provider-only preservation, duplicate rejection, exact scoped deletion, discovered replacement persistence, no-replacement non-mutation, and Keychain-failure rollback use temporary config/fake-secret stores |
| `cargo test tui::input_tests --no-fail-fast` | PASS: 45 input tests, including provider ordering/filtering/navigation, paste isolation, list/detail/actions, zero-model Add Model, editor return paths, default-cancel confirmation, and busy locking |
| focused provider-manager orchestration tests | PASS: safe command construction, provider-only reload, current/default replacement, discovered-model persistence, affected-provider cache preservation, and no-replacement blocking |
| `cargo test tui::render_tests --no-fail-fast` | PASS: 43 tests, 92 accepted snapshots, 0 pending; provider command, populated/empty/no-match list, detail, masked settings, delete confirmation, busy, and inline failure states reviewed wide/narrow |
| `cargo test --no-fail-fast` | PASS: 361 unit tests plus 20 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |
| isolated rebuilt-binary TUI check | PASS: `/provider` showed one row per provider; MiMo fallback exposed Pro; a loopback catalog exposed two live models; provider settings/Add Model/Add Provider opened and canceled back correctly; confirmed MiMo deletion selected and persisted `backup/backup-model` |

Manager state contains only raw-config provider/model IDs, a credential-stripped endpoint, and safe credential-source classification. Provider-only edits and deletion use atomic raw-TOML patches; exact Keychain cleanup follows the config write and restores the original document on failure. Refresh requests from provider detail and successful edits target only that provider. The isolated home and local catalog process were stopped; the home was moved to Trash and is recoverable there. A real Keychain deletion and credential-dependent external provider request were intentionally not run. User config, sessions, Keychain, oracle/reference files, and installed binary were unchanged. `/provider` is an oracle-informed `ca` extension, not a Cursor parity claim.

## CFG-034 Completion Results

Commands run on 2026-08-10 after adding provider setup and credential checks:

| Command | Result |
|---|---|
| `cargo test provider_ --no-fail-fast` | PASS: 57 unit tests plus 1 matching CLI test; isolated authenticated/credentialless probes, missing-environment zero-I/O, typed HTTP/transport/protocol outcomes, setup navigation, editing, finish selection, persistence, and safe auth copy |
| `cargo test tui::input_tests --no-fail-fast` | PASS: 47 input tests, including explicit test/cancel locking, checklist/detail/finish transitions, passing-only finish, setup editor routing, and composer/paste isolation |
| `cargo test tui::render_tests --no-fail-fast` | PASS: 45 tests, 0 pending; wide/narrow checklist, checking, working/failure, missing environment, needs-model, finish, no-passing, command-palette, and safe failed-chat states reviewed |
| `cargo test --no-fail-fast` | PASS: 376 unit tests plus 20 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--help` and `--version` | PASS: `ca 0.1.0` |
| isolated rebuilt-binary TUI check | PASS: `/provider setup` listed four temporary providers current-first; authenticated and credentialless probes worked and advanced; a missing environment reference failed locally; a loopback 401 stayed in repair detail and displayed no response-body marker |

Each probe sends exactly one fixed user-only completion request using one deterministic representative model, no system/history/tools/images/reasoning, a 20-second timeout, and a 64 KiB aggregate event bound. A valid completed stream is required; generated text is discarded. Resolved credentials exist only inside the background client task, and provider setup state receives IDs plus typed outcomes. Provider HTTP bodies and request URLs are discarded at the shared client boundary, credentialless requests omit Authorization, and unresolved environment references fail before I/O. Existing immutable `/retry` tests prove a repaired/default switch retries in a child branch without modifying the source session. The temporary home contained only fake environment references, was inspected, and was moved to Trash; loopback processes were stopped. User config, sessions, Keychain, oracle/reference files, and installed binary were unchanged. This is an oracle-informed `ca` extension, not a Cursor parity claim.

## Validation Ladder

### 1. Focused characterization

Run the smallest test that proves the changed behavior. Add one first if none exists.

Examples:

```bash
cargo test <module_or_test_name> -- --nocapture
cargo test --test <integration_target>
```

### 2. Unit/integration suite

```bash
cargo test
```

Requirements:

- no network;
- no user config/session/clipboard access;
- temporary directories for filesystem mutations;
- deterministic time, IDs, and provider output.

### 3. Format and lint

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Both are required for completion. The 2026-08-15 exact-tree review passes both,
including strict all-feature Clippy. A task that does not explicitly own broad
formatting must not run `cargo fmt` across the repository; use `--check` and
keep its diff local.

### 4. Release build and CLI smoke

```bash
cargo build --release
./target/release/ca --help
```

`--version` and `--help` are local exits after CLI-007 and are safe smoke checks.

### 5. Deterministic render/protocol checks

Required for relevant work:

- ratatui `TestBackend` snapshots at representative wide and narrow sizes;
- startup, streaming, tool result, approval, picker, diff, error, and Unicode fixtures;
- fake-provider assertion of outbound role/order/tool-call IDs;
- snapshot review that does not blindly accept changes.

### 6. Opt-in live integration

Only when credentials/service scope is explicit:

```bash
./target/release/ca --test tests/basic.ca
```

Before running, inspect the selected `.ca` file. Cases can read/write files, run commands, send messages, use image generation, alter sessions, and call providers. Record endpoint/service assumptions and redact secrets.

### 7. Manual TUI/parity check

Required for user-visible behavior after deterministic tests pass:

- record terminal emulator and width/height;
- use exact input/keystrokes;
- verify terminal restoration after cancel/error/exit;
- capture expected vs actual text without secrets;
- for parity, cite the fixture/evidence in `PARITY.md`.

## Feature-to-Test Map

| Change | Minimum focused evidence |
|---|---|
| provider/SSE/tool calls | fake transport + message sequence + malformed stream tests |
| approvals | policy units + multi-call exactly-once integration |
| input/vim/paste/completion | key-event table including UTF-8 boundaries |
| rendering/markdown/diffs/overlays | wide+narrow text snapshots and Unicode |
| config/session/context | temp-root round trip and compatibility case |
| shell/filesystem tools | temp workspace, timeout/error/truncation cases |
| CLI flags/headless | subprocess stdout/stderr/exit assertions with fake provider |
| parity slice | clean Cursor observation + deterministic `ca` fixture |

## Known Gaps

- no clean, versioned Cursor runtime fixture exists, so all parity rows remain
  `Unverified` even though local characterization is broad;
- asynchronous responsiveness/cancellation is covered for shell jobs; other synchronous orchestration paths need focused reproduction before refactoring;
- the six checked-in `tests/*.ca` scenarios still require live provider behavior when invoked through `ca --test`;
- the manual local-Chromium test and complete external experiment-fixture test
  remain intentionally ignored by the ordinary all-feature gate;
- the RND-097 paid hypothesis remains unmeasured; its credential, pricing,
  preregistration, and hard spend gates remain closed.

Current work to close these gaps is in `NEXT_STEPS.md`.

## CMD-010 Completion Results

Commands run on 2026-08-09 after centralizing command metadata and dispatch:

| Command | Result |
|---|---|
| `cargo test tui::commands --no-fail-fast` | PASS: 4 registry/parser tests |
| `cargo test slash_palette --no-fail-fast` | PASS: 2 navigation/action tests |
| `cargo test idle_only_palette_commands --no-fail-fast` | PASS: 1 busy-state test |
| `cargo test command_palette --no-fail-fast` | PASS: 2 reviewed wide/narrow snapshots |
| `cargo test` | PASS: 105 tests total |
| `rustfmt --edition 2021 --check src/tui/commands.rs` | PASS |
| `cargo fmt --check` | FAIL: pre-existing repository-wide formatting drift remains |
| `cargo clippy --all-targets -- -D warnings` | FAIL: the same 3 pre-existing diagnostics in `src/agent/tools.rs` and `src/approval.rs` |
| `cargo build --release` | PASS |
| `./target/release/ca --version` and `--help` | PASS |

No `.snap.new` files remain. The pre-existing live TUI process remained running and was not restarted or interrupted; no provider, user config, or user session mutation was used for validation.

## SES-011 Completion Results

Commands run on 2026-08-09 after introducing stable sessions and branches:

| Command | Result |
|---|---|
| `cargo test session::tests --no-fail-fast` | PASS: 6 temporary-store tests |
| `cargo test timeline_confirmation --no-fail-fast` | PASS: typed branch-confirmation input test |
| `cargo test session_picker_snapshots --no-fail-fast` | PASS: reviewed wide/narrow snapshots |
| `cargo test timeline_snapshots --no-fail-fast` | PASS: reviewed wide/narrow snapshots |
| `cargo test` | PASS: 114 tests total |
| `rustfmt --edition 2021 --check src/session.rs` | PASS |
| `cargo fmt --check` | FAIL: pre-existing repository-wide formatting drift remains |
| `cargo clippy --all-targets -- -D warnings` | FAIL: the same 3 pre-existing diagnostics in `src/agent/tools.rs` and `src/approval.rs`; no SES diagnostic |
| `cargo build --release` | PASS |
| `./target/release/ca --version` and `--help` | PASS |

Store tests cover version-2 round trip, stable replacement, version-1 deferred migration, injected atomic-write failure, updated-time sorting/search, full and historical forks, source immutability, and complete-turn boundaries. Tests use only temporary roots; no user session file was read or written during validation.

## QUE-012 Completion Results

Commands run on 2026-08-09 after implementing queue/steer/interrupt state:

| Command | Result |
|---|---|
| `cargo test queue::tests --no-fail-fast` | PASS: 5 pure queue/outcome tests |
| `cargo test busy_enter_transition --no-fail-fast` | PASS: queue-by-default transition |
| `cargo test steering_preserves_partial --no-fail-fast` | PASS: partial text and protocol sanitization |
| `cargo test queue_view_edit --no-fail-fast` | PASS: edited-item identity retained for steering |
| queue preview and queue view snapshot tests | PASS: 4 reviewed wide/narrow snapshots |
| `cargo test` | PASS: 128 tests total |
| `rustfmt --edition 2021 --check src/queue.rs` | PASS |
| `cargo fmt --check` | FAIL: pre-existing repository-wide formatting drift remains |
| `cargo clippy --all-targets -- -D warnings` | FAIL: the same 3 pre-existing diagnostics; the introduced argument-count lint was fixed |
| `cargo build --release` | PASS |
| local `--version`/`--help` smoke and live PID check | PASS; PID 51440 remained running |

The suite also covers delete/reorder/edit, edited-item removal, Alt+Enter, Ctrl+S action emission, failed/cancelled no-drain behavior, and approval/question precedence. No provider credentials or user runtime queue state were used.

## CTX-013 Completion Results

Commands run on 2026-08-09 after replacing destructive compression with checkpoints:

| Command | Result |
|---|---|
| `cargo test context::tests --no-fail-fast` | PASS: 7 fake-model/boundary/reconstruction/threshold/config tests |
| `cargo test context_dashboard_snapshots --no-fail-fast` | PASS: 2 reviewed wide/narrow snapshots |
| `cargo test` | PASS: 136 tests total |
| `rustfmt --edition 2021 --check src/context.rs` | PASS |
| `cargo fmt --check` | FAIL: pre-existing repository-wide formatting drift remains |
| `cargo clippy --all-targets -- -D warnings` | FAIL: the same 3 pre-existing diagnostics; no CTX diagnostic |
| `cargo build --release` | PASS |
| local `--version`/`--help` smoke and live PID check | PASS; PID 51440 remained running |

Coverage includes structured fake-model generation, complete-turn/tool-call boundaries, canonical-history preservation, checkpoint request reconstruction, known/unknown thresholds, same-transcript no-retry, failure rollback, string-list config compatibility, session checkpoint restoration, and wrapped dashboard rendering. No live provider request or user session/config mutation was used for validation.

## DIF-014 Completion Results

Commands run on 2026-08-09 after adding full working-tree review:

| Command | Result |
|---|---|
| `cargo test changes::` | PASS: 4 temporary-repository/service tests |
| change-review input tests | PASS: 3 typed-action/precedence tests |
| working-tree snapshot tests | PASS: 4 reviewed wide/narrow snapshots |
| `cargo test` | PASS: 145 tests total |
| targeted `rustfmt --check` for touched Rust files | PASS |
| `cargo fmt --check` | PASS in the current workspace |
| `cargo clippy --all-targets -- -D warnings` | FAIL: the same 3 pre-existing diagnostics in `src/agent/tools.rs` and `src/approval.rs`; no DIF diagnostic |
| `cargo build --release` | PASS |
| local `--version`/`--help` smoke and live PID check | PASS; PID 51440 remained running |

Coverage includes staged, unstaged, deleted, renamed, untracked, Unicode, long lines, non-repository errors, ANSI/control sanitization, tracked-only diff opening, and wrapped list/diff rendering. Tests mutate only temporary Git repositories; the production review surface has no working-tree mutation actions.

## ASY-006 Completion Results

Commands run on 2026-08-09 after adding asynchronous shell jobs:

| Command | Result |
|---|---|
| `cargo test jobs::tests -- --nocapture` | PASS: 7 lifecycle/responsiveness tests |
| `cargo test job_tool_tests -- --nocapture` | PASS: 2 schema/background-tool tests |
| direct-shell parser and approval tests | PASS: 2 tests |
| jobs input tests | PASS: 2 cancel/stdin/detail tests |
| jobs list/detail snapshot tests | PASS: 4 reviewed wide/narrow snapshots |
| `cargo test` | PASS: 160 tests total |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | FAIL: the same 3 pre-existing diagnostics in `src/agent/tools.rs` and `src/approval.rs`; no ASY diagnostic |
| `cargo build --release` | PASS |
| local `--version`/`--help`, nested-runtime search, snapshot-artifact check, live PID check | PASS; no blocking runtime calls or `.snap.new` files, PID 51440 remained running |

Coverage includes streamed stdout/stderr, background completion, stable IDs, stdin, cancellation, timeout, bounded-tail output, active-job shutdown, foreground responsiveness, tool schemas, approval enforcement, direct-shell prefix parsing, and jobs list/detail interaction. Tests launched only short-lived local commands and explicitly cleaned active jobs; no daemon or user runtime state was created.

## CLI-007 Completion Results

Commands run on 2026-08-09 after making argument parsing side-effect safe:

| Command | Result |
|---|---|
| `cargo test cli_parser -- --nocapture` | PASS: 3 parser tests |
| `cargo test --test cli -- --nocapture` | PASS: 3 subprocess tests |
| `cargo test` | PASS: 97 tests total |
| targeted formatter inspection | PASS for the new parser and integration-test blocks; whole-file drift remains in `src/main.rs` |
| `cargo clippy --all-targets -- -D warnings` | FAIL: the same 3 pre-existing diagnostics in `src/agent/tools.rs` and `src/approval.rs` |

The subprocess suite uses a temporary `HOME` and verifies `--help`, `--version`, and unknown options do not create `~/.config/ca/config.toml` or enter provider execution.

## ENV-009 Completion Results

Commands run on 2026-08-09 after adding injected runtime paths and secret-safe raw config patches:

| Command | Result |
|---|---|
| `cargo test runtime::tests --no-fail-fast` | PASS: 2 injected/missing-home tests |
| `cargo test config_store_tests --no-fail-fast` | PASS: 2 temp-root creation/placeholder tests |
| `cargo test context::tests::shell_history_uses_only_the_injected_path --no-fail-fast` | PASS: 1 temp-root history test |
| `cargo test` | PASS: 162 unit tests plus 3 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | FAIL: the same 3 pre-existing diagnostics in `src/agent/tools.rs` and `src/approval.rs`; no ENV diagnostic |
| `cargo build --release` | PASS |
| local `--version` and `--help` | PASS: `ca 0.1.0`; help exited locally |
| developer/Hermes fallback search | PASS: no `andrewzimmerman`, `.hermes/.env`, or `source_hermes_env` in production source |

All new filesystem tests use `tempfile`; no user config, sessions, history, credentials, oracle file, installed binary, or live process was touched.

## TUI-015 Completion Results

Commands run on 2026-08-09 after removing unused mouse capture and introducing the lifecycle guard:

| Command | Result |
|---|---|
| `cargo test lifecycle_tests --no-fail-fast` | PASS: 3 fake-control drop/failure/unwind tests |
| mouse-capture source search | PASS: no `EnableMouseCapture` or `DisableMouseCapture` remains |
| `cargo test` | PASS: 165 unit tests plus 3 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | FAIL: the same 3 pre-existing diagnostics in `src/agent/tools.rs` and `src/approval.rs`; no TUI-015 diagnostic |
| `cargo build --release` | PASS |
| local `--version` and `--help` | PASS |

The lifecycle tests do not manipulate a real terminal. No live TUI, runtime state, oracle file, or installed binary was touched; no layout snapshot changed.

## SSE-008 Completion Results

Commands run on 2026-08-09 after adding truthful provider streams and branch-safe retry:

| Command | Result |
|---|---|
| `cargo test agent::stream::tests --no-fail-fast` | PASS: 6 byte-framing/assembler/error tests |
| partial-failure and immutable-retry focused tests | PASS: 2 orchestration/session tests |
| `/retry` registry test | PASS: visible and idle-only |
| failure/retry render test | PASS: 2 reviewed wide/narrow snapshots; 0 pending `.snap.new` files |
| `cargo test` | PASS: 175 unit tests plus 3 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | FAIL: the same 3 pre-existing diagnostics in `src/agent/tools.rs` and `src/approval.rs`; no SSE diagnostic |
| `cargo build --release` | PASS |
| local `--version` and `--help` | PASS |

Coverage includes one-byte UTF-8 splitting, LF/CRLF, comments, multiline data, usage-only events, `[DONE]`, invalid UTF-8/JSON, event caps, transport errors, clean premature EOF, incomplete reasons, tool identity/argument integrity, partial-text preservation, failure no-drain, and source-session immutability after retry. No provider network, credentials, user state, oracle file, installed binary, or live TUI was used.

## PAR-005 Evidence Result

The 2026-08-09 clean-runtime attempt is recorded in `tests/fixtures/parity/cursor-runtime-blocked-2026-08-09.md`.

- The modified dated wrapper was rejected as stock evidence and was not executed.
- The clean backup bundle was attempted with an empty temporary home and network denied; it exited before CLI/TUI output because it is not a standalone entrypoint.
- No output fixture, version, dimensions, or keystrokes are claimed.
- No parity row changed from `Unverified`; no production test/implementation was added.
- Temporary directories were removed and no oracle/runtime file was modified.
- Repository validation after the documentation-only record: 175 unit + 3 CLI tests PASS; format/release/help/version PASS; strict Clippy remains at the same 3 pre-existing findings.

## TOL-016 Completion Results

Commands run on 2026-08-09 after centralizing tool discovery, validation, dispatch, profiles, and approval metadata:

| Command | Result |
|---|---|
| `cargo test agent::registry::tests -- --nocapture` | PASS: 3 registry/schema/profile/collision tests covering all 27 built-ins |
| `cargo test agent::tools::job_tool_tests -- --nocapture` | PASS: 3 schema, typed failure, profile, and background-job tests |
| independent identical-call regression | PASS: two distinct call IDs executed the same command twice in a temporary root |
| duplicate persistent-denial regression | PASS: both call IDs retained typed error status |
| `cargo test` | PASS: 180 unit tests plus 3 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS: the prior tool/approval baseline is cleared |
| `cargo build --release` | PASS |
| local `--version` and `--help` | PASS: `ca 0.1.0`; help exited locally |

All execution tests use temporary roots and short-lived local processes. No provider network, credentials, user state, oracle file, installed binary, or live TUI was used. No render output changed, so existing wide/narrow snapshots remained the reviewed visual baseline.

## HED-017 Completion Results

Commands run on 2026-08-09 after extracting structured headless automation:

| Command | Result |
|---|---|
| `cargo test headless::tests -- --nocapture` | PASS: 9 envelope/output/policy/exit/broken-pipe tests |
| `cargo test --test cli -- --nocapture` | PASS: 15 local CLI and loopback-provider subprocess tests |
| `cargo test` | PASS: 189 unit tests plus 15 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--version` and `--help` | PASS: `ca 0.1.0`; all headless flags documented |

The subprocess matrix covers default text, aggregate JSON, JSONL/`stream-json`, clean stdout/stderr separation, tool call/result continuation, persistent policy/yolo behavior, last-message output, empty/setup/approval/transport/protocol/max-turn/internal exits 2–8, and SIGINT 130. The injectable suite covers broken-pipe exit 0 and network/interaction gating. Tests use temporary homes/roots, short-lived local processes, and loopback only; no external provider, credentials, user runtime state, oracle file, installed binary, or live TUI was used.

## INT-018 Completion Results

Commands run on 2026-08-09 after replacing marker-string questions with typed interactions:

| Command | Result |
|---|---|
| `cargo test interaction::tests -- --nocapture` | PASS: 5 mode/shortcut/Other/order/secret tests |
| focused mixed-batch and exact-resolution tests | PASS: 3 orchestration regressions |
| `cargo test interaction_snapshots -- --nocapture` | PASS: 2 tests / 4 reviewed wide+narrow snapshots |
| `cargo test agent::registry::tests -- --nocapture` | PASS: canonical tool advertised; compatibility alias hidden |
| `cargo test` | PASS: 199 unit tests plus 15 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| local `--version` and `--help` | PASS |

Coverage proves exact call-ID resolution, stale-ID rejection, no synthetic user message, composer/queue/kill-ring isolation, call-order blocking for later automatic tools, headless exit 4 for both tool names, nine-choice limits, cancellation, redacted debug output, zeroization, and non-model secret results. No `.snap.new` files remain. No provider network, credentials, user state, oracle file, installed binary, or live TUI was used.

## WT-071 Completion Results

Commands run on 2026-08-11 after adding the explicit worktree lifecycle:

| Command | Result |
|---|---|
| `cargo test worktree --no-fail-fast` | PASS: real-Git registry/create/status/diff/cleanup plus command/input/orchestration/render coverage |
| `cargo test agent_spawns_capture_immutable_workspace_snapshots --no-fail-fast` | PASS: running children retain their spawn root; later spawns receive the attached root |
| `cargo test attached_worktree_becomes_the_exact_workflow_definition_root --no-fail-fast` | PASS: relative workflow definitions resolve only under the attached workspace |
| worktree render snapshots | PASS: 14 reviewed wide/narrow list/status/diff/title/cleanup/busy/error snapshots |
| `cargo test --no-fail-fast` | PASS: 855 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| released `--help`, `--version`, named-profile, and bare-profile temporary-home smokes | PASS |

The real-Git suite covers derived profile paths, invalid labels/refs, repository/profile isolation, manifest bounds, symlinked parent rejection, identity mismatch, missing records, bounded terminal-safe diffs, untracked-name-only reporting, dirty refusal, clean removal, branch preservation including a unique worktree commit, and no automatic cleanup. The disposable released-TUI walkthrough created and attached a profile-owned tree, ran a loopback workflow that read `WT071_ATTACHED_ROOT` from that exact root, refused an untracked dirty tree, preserved it on the default Cancel action, then removed only the cleaned directory/manifest record while leaving the branch. Temporary config, sessions, repository, server, and worktree data were moved to Trash; no real config, Keychain, saved session, oracle, external service, or installed binary was touched.

## AGT-072 Completion Results

Commands run on 2026-08-12 after adding dependency-aware plan-agent graphs:

| Command | Result |
|---|---|
| focused `agent_graph`, `work`, graph orchestration, subagent evidence/capacity, input, and render tests | PASS: validation/CAS, scheduling, restore/fork safety, exact worktree preflight, evidence/retry, manager locking, and user controls |
| agent-graph render snapshots | PASS: 14 reviewed wide/narrow empty/ordinary/ready/running/busy/retry/completed surfaces |
| `cargo test --no-fail-fast` | PASS: 869 unit tests plus 28 CLI integration tests |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo build --release` | PASS |
| released `--help`, `--version`, named-profile, and bare-profile temporary-home smokes | PASS |

Coverage proves bounded exact-task graph validation, duplicate/self/unknown/cyclic-edge rejection, stable plan-order readiness, `AgentManager` concurrency accounting, immutable exact grants/workspaces, missing-worktree failure before child/provider traffic, typed successful-tool evidence, append-only failed/completed attempts, dependency blocking, explicit retry, cancellation, interrupted/paused restore, full-fork dependency remapping, and legacy session compatibility. The disposable released-TUI walkthrough configured three tasks over two profile-owned worktrees, started two independent roots concurrently, paused on a required-tool evidence failure, kept the dependent blocked, then recorded a second attempt after Cancel-first retry; the retry read only `SECOND_TREE_MARKER` from its assigned worktree and released the dependent after both gates passed. The completed durable session recorded attempt statuses `evidence_failed` then `completed` with `read_file`, plus `DEPENDENCY_RELEASED_OK`. Temporary config, sessions, repository, loopback server, and worktrees were moved to Trash; no real config, Keychain, saved session, oracle, external service, or installed binary was touched.

## INT-195 Completion Result

On 2026-08-27, focused protocol, broker, real five-client stdio query/evidence, Rust API-impact, CA mechanistic activity, MCP compatibility, and TUI snapshot tests passed. `cargo fmt --all -- --check`, strict all-target/all-feature workspace Clippy, and one warning-free all-feature workspace release build passed. The full workspace suite, installation, live provider traffic, Git release-readiness checks, and protected-tree checks were intentionally not run under the INT-195 contract.

## INT-196 Completion Result

On 2026-08-27, focused protocol, tracked read/search/directory/glob, broker receipt-bound, transparent-tool mapping/chunking/capsule, session projection, retry-preservation, tool-surface exclusion, and real release-binary stdio compatibility tests passed. The real CA client started outside the pinned disposable repository, routed ordinary `list_dir` and `search_files` calls through codex-team without root discovery, received familiar outputs and one revision-bound capsule, and reconfirmed the exact 11/15/28 catalogs with zero resources. `cargo fmt --all -- --check`, strict all-target/all-feature workspace Clippy, one all-feature workspace release build, and release help/version smokes passed. The full workspace suite, installation, live provider traffic, Git gates, and protected-tree checks were intentionally not run under the INT-196 contract.

## INT-197 Completion Result

On 2026-08-27, focused remote protocol, manifest hardening, fixed SSH/rsync construction, Transmission RPC, CA configuration/routing/approval/exclusion, and real hermetic CA-to-release-codex-team handoff tests passed. The existing MCP SDK fixture remained 8/8 with unchanged catalogs/resources. `cargo fmt --all -- --check`, strict all-target/all-feature workspace Clippy, one all-feature workspace release build, and release remote-help smoke passed. No live host, credential, runtime profile, installation, full workspace suite, Git gate, or protected tree was used.

## INT-198 Completion Result

On 2026-08-27, the 9-test shared protocol suite, 2 codex-team setup transaction tests, 11 CA registry tests, 17 tool-selection tests, explicit setup-approval regression, 8/8 existing MCP SDK fixture, and real temporary-profile CA-to-debug/release-codex-team setup handoffs passed. Preview wrote nothing; digest-bound apply created private idempotent configuration that CA loaded successfully. Formatting, strict all-target/all-feature workspace Clippy, one all-feature release build, release setup help, and CA version passed. No live host/provider, credential value, runtime profile, installed binary, full workspace suite, Git gate, or protected tree was used.

## INT-199 Completion Result

On 2026-08-27, focused workflow protocol, secure policy-vault, three-depth preview, correction lattice, exact approval, bounded execution/repair, remote-adapter, CA facade, approval, scheduling, recovery, feedback, recursion-exclusion, and broker/MCP tests passed. The single workspace test pass passed every target except two five-client broker startup races; after fixing startup/idle coordination, both cases and the complete focused MCP SDK target passed 8/8 without repeating the broad suite. Formatting, strict all-target/all-feature workspace Clippy, and one all-feature workspace release build passed. No live remote/provider traffic, credential access, installation, runtime-profile change, Git gate, commit, or protected-tree edit occurred.

## INT-200 Completion Result

On 2026-08-27, focused protocol, normalized Transmission legacy/JSON-RPC 2.0, rTorrent HTTP/Unix/TCP/SSH-SCGI, cross-client inventory, workflow approval/drift/control, selection-aware rsync, private setup, CA facade, scheduling, exclusion, and real MCP compatibility tests passed. The one workspace test pass found one obsolete legacy Transmission CLI-fixture expectation after every preceding target passed; the fixture now explicitly selects legacy mode, asserts the normalized receipt, and passes in its focused rerun without repeating the broad suite. Formatting, strict all-target/all-feature workspace Clippy, one all-feature workspace release build, and the release-binary exact 11/15/28 zero-resource MCP fixture passed. No live torrent client, remote/provider traffic, credential access, installation, runtime-profile change, Git gate, commit, or protected-tree edit occurred.

## TUI-212 Completion Result

On 2026-08-28, focused copy-picker, Markdown-link, Mermaid,
terminal-awareness, input, command-registry, configuration, lifecycle, and
render tests passed. The new wide/narrow copy and Mermaid snapshots were
manually reviewed. The corrected serial default workspace matrix and the
all-target/all-feature serial workspace matrix passed completely; formatting,
strict all-target/all-feature workspace Clippy, and the all-feature workspace
release build also passed. `git diff --check`, zero `.snap.new`, the exact
protected-baseline comparison, and `codex-team verify protected-trees --json`
passed. No live provider, credential, user runtime profile, installation,
oracle, or reference file was used or changed.
