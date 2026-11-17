# Automated verification and evidence-led repair

## Focused development and prompt installation

For a small change, run the relevant behavioral tests, formatting and strict
Clippy through the focused entry point:

```sh
python3 scripts/check-change.py --selector provider_copy_pane --selector provider_filter::tests
```

It keeps one locked disposable source slot and one private dependency cache at
stable paths under `scratch/check-change/`. Unchanged files keep their mtimes;
each run gets fresh application state and a new receipt. Cargo settings and
credentials are excluded. `--prepare-only` measures source/cache preparation
without compiling or claiming test success. The first compile can be cold;
later checks reuse its compiler outputs instead of changing source/cache paths.

When installing a changed Rust binary, bump the root package once before the
final focused run and add `--release`. Run that job asynchronously. It builds
the ca binary with all features once and records its source/artifact identity.
Reuse that exact candidate for installation:

```sh
bash tools/bug257_install.sh
```

The installer checks the candidate, preserves a hashed recovery copy, signs and
atomically replaces the executable, then checks the installed signature,
identity, version and help. Do not rebuild between validation and installation.
Procedure/documentation-only edits do not need another Rust build or patch bump.

The full gate below remains available for explicitly requested or broad
cross-cutting verification. Its criteria are unchanged. Focused results are not
a complete workspace certification, and interrupted stages are never passes.

## Complete workspace gate

From the application checkout (the nested `ca/` directory), run:

```sh
./scripts/verify.sh
```

Python 3 and the pinned Rust 1.97.1 toolchain with rustfmt/Clippy must be on PATH.
Dependencies must already be cached. On a new machine prepare them with
`cargo fetch --locked` before starting the offline workflow. macOS and Linux
need PTYs, loopback sockets, Git and subprocess execution. No extra Python
package is needed. Host-platform execution is required; these tests are not a
cross-platform certification.

## What runs

1. Locked/offline workspace metadata and verifier contract tests.
2. Workspace formatting and strict all-target/all-feature Clippy.
3. Full default and all-feature workspace tests, serially, without exclusions.
4. The all-feature workspace release build.
5. The real release CLI and terminal: help/version, startup, submitted input,
   outbound loopback request, streamed reply rendered in the terminal, actual
   file-writing tool and tool-result continuation, exact read-only independent
   review of that write, narrow/wide resize, clean exit and terminal restoration,
   and unchanged binary identity. The reviewer reads the actual file before its
   structured decision; retained request, candidate and terminal evidence proves
   review completion and parent finalization.
6. A separate deterministic evaluator checks raw logs, required checks, counts,
   source stability, binary identity and runtime receipts. Implementation claims
   alone cannot pass the evaluator.

The existing Rust suite covers units, integration, headless protocols, durable
state, approval boundaries and deterministic render snapshots. Real terminal
execution complements it. Ignored tests are counted and reported; the workflow
does not enable external providers, native promotion/activation, VM operations,
manual Chromium or paid experiments. Live-provider behavior and long soaks
require separate explicit scope and evidence.

## Evidence and exits

Every run creates a new private directory under `scratch/verify/`. Specify a new
directory with `--output PATH`. Existing directories are rejected so failures
cannot be overwritten. `report.json` records every stage's command, exit,
duration, log/hash, source inventory digest, binary hash, resource high-water
mark and evaluator outcome. `evaluation.json` contains actionable diagnostics,
per-target test totals, ignored cases and runtime measurements. Raw VT output,
terminal text frames, provider requests and real tool output are retained under
`runtime/`. The real tool file also remains in its recorded disposable workspace;
the evaluator binds both the copied evidence and the actual file. The printed
report path is the unattended runner's result.

Exit 0 means all required verification passed. Exit 1 means behavior, build,
lint, regression or evaluation failed. Exit 2 means infrastructure failed, such
as missing tools/cache, launch denial, deadline, unavailable artifacts or damaged
evidence. Read the actual stage log before diagnosing. Some permission denials
can also be legitimate negative fixtures; classification uses the observed
failure output and never converts nonzero exits to passing results.

Application HOME, XDG state, Git settings and temporary paths are disposable;
ambient provider keys, proxy/SSH settings and custom application binary overrides
are not inherited. Only dependency cache bytes and compiled build artifacts are
reused. Dependency bytes are copied into a real private Cargo registry so native
verification fixtures can use them without writing the user's cache. User Cargo
settings/credentials are not loaded. Short private temporary
paths under `/var/tmp` (macOS `/private/var/tmp`) prevent Unix-socket address
overflows while keeping fixtures outside the ordinary sandbox's allowed `/tmp`
tree and broad `/tmp` ownership fixtures. Their paths are recorded and
retained for diagnosis. Remove old run directories and their recorded temporary
fixtures when their evidence is no longer needed.

This is fixture isolation, not OS security containment. Native macOS Keychain
access is not blocked by HOME; tested fixture paths use credentialless local
providers. Use a disposable VM for tests requiring a hard boundary around native
host facilities. Native workspace ownership also uses shared host coordination
locks; the workflow never clears another process's lock or ownership state.
The verification command does not install binaries, publish,
or operate production services/data.

## Closed loop

For each change, state the requirement and expected observation; establish a
baseline; implement; run the focused behavioral regression; then run the full
entry point. On failure, read the evidence, form a root-cause hypothesis and
make a narrow correction. Preserve the original failure. Rebuild/retest and
compare the new evidence. Stop after three repair attempts for the same failure,
on an external blocker, or when the next action would require unauthorized
production/irreversible operations. Repair exhaustion is failure.

The driver automates that bounded cycle for an explicitly configured repair
command:

```sh
python3 scripts/verify-loop.py --output scratch/verify-loop/my-change \
  --repair-command 'your-reviewed-repair-command'
```

Without `--repair-command`, it runs verification and returns the evidence for
the current development agent to diagnose. A repair command receives the actual
report path and attempt metadata through documented environment variables (see
`python3 scripts/verify-loop.py --help`). It runs only after a verified failure,
not an infrastructure failure. It must make a source correction. The driver
hashes established tests, snapshots, fixtures, manifests, test sections and
verification/evaluation code before repair and stops if they change. It does not
accept rewritten criteria as a fix or repeat unchanged unsuccessful attempts.
This guard detects changes; a trusted repair command is not an OS sandbox. Use a
disposable checkout for experimental or model-driven repair commands and review
the resulting diff before adoption.

Codex consumes the workflow through [AGENTS.md](../AGENTS.md). Optional project
SessionStart/Stop hooks remind the agent and request bounded continuation when
source changes lack current passing evidence; they do not perform the expensive
gate inside a hook. Codex CLI 0.160.0 on the development host exposes stable
enabled hooks. The project hook definition must be reviewed/trusted in `/hooks`
before Codex runs it; no global setting or trust record is changed here. This
trust requirement is documented by [official OpenAI hooks documentation](https://learn.chatgpt.com/docs/hooks).
Direct execution and repository instructions work independently of hook trust.
The entry point can be called from CI or a Codex scheduled task; no recurring
schedule is created because none was requested.

## Baselines and measured improvement

```sh
./scripts/verify.sh --output scratch/verify/baseline
# Implement the proposed change, keeping established acceptance criteria.
./scripts/verify.sh --output scratch/verify/candidate \
  --baseline scratch/verify/baseline/report.json
```

Comparisons require a complete passing baseline on a comparable platform and
compatible evaluation criteria. Test losses/new ignores and gross terminal
latency/resource regressions fail evaluation. The smoke thresholds permit
twice the baseline plus 100 ms for timings and twice baseline peak RSS plus
16 MiB. Single-run runtime timings and child peak
memory are smoke measurements; build durations depend on cache state. Repeated
paired runs on a controlled host are needed to substantiate optimization claims.
Document the hypothesis, measured effect, unacceptable regression threshold and
decision. Reject or revert an unsuccessful candidate in the isolated checkout;
the workflow never silently resets the user's working tree.

## Validate the validator

```sh
./scripts/verify.sh --self-test
```

This copies current source (including dirty/untracked inputs) into a disposable
directory. The unchanged watcher test first passes. The demonstration changes
only production `MIN_INTERVAL_SECS` from 2 to 1 and proves the existing assertion
fails. It restores the exact source bytes and proves the test passes again.
Before/after source hashes and all three stage logs remain in the self-test
receipt. It uses the same execution and failure classification machinery as the
full entry point. This focused demonstration is distinct from the full clean
workspace gate; no defective source or binary is installed.

## Maintenance

Keep required stages and evaluator expectations aligned. A legitimate test or
criteria change must be reviewed separately from an automated repair; add a
failure-first verifier contract case before changing the guard. Use independent
review for semantic requirements not captured by deterministic criteria. Add
feature-specific protocol/terminal cases when behavior changes. Do not merely
count tests or accept snapshot updates to make a candidate pass. Preserve failed
receipts and state exactly which platforms, providers and durations were tested.
