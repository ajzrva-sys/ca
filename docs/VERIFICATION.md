# Verification

Run the unattended completion gate from the repository root:

```sh
cargo fetch --locked
./scripts/verify.sh --output scratch/verify/my-change
```

The first command prepares the dependency cache. The gate uses the pinned Rust
toolchain, locked offline dependencies and Python 3. Install rustfmt, Clippy and
Git; allow native subprocesses, PTYs and loopback sockets. FreeBSD required
isolation additionally needs the shared jail service described in
[FreeBSD setup](FREEBSD.md). Run on the target platform: a build or a different
host's tests do not qualify native isolation.

## Required stages

1. Workspace metadata and verifier contract tests.
2. Workspace formatting and strict all-target/all-feature Clippy.
3. Complete default and all-feature workspace tests, serially, without exclusions.
4. The all-feature workspace release build.
5. Native release CLI and terminal checks: help/version, startup, a submitted
   prompt, a loopback request and rendered response, a real file-writing tool,
   tool continuation, independent read-only review, resize, clean exit and
   terminal restoration.
6. A separate evaluator checking raw results, required checks, test totals,
   source stability, artifact identity and runtime receipts.

Existing ignored cases are counted explicitly. External providers, long soaks,
manual native integrations and operating systems not actually tested require
separate evidence. A passing compilation is not proof of runtime behavior.

## Read the evidence

Use a new output directory for every run. `report.json` records stage commands,
exits, durations, logs, source and binary identities, resource measurements and
evaluator outcome. `evaluation.json` supplies diagnostics and test totals.
Runtime receipts retain terminal output, loopback requests and actual tool
results. Inspect the failing stage's raw log before diagnosing it.

Exit 0 means all required stages passed. Exit 1 means a behavior, build, lint,
regression or evaluation failure. Exit 2 means an infrastructure failure, such
as a deadline, missing prerequisite or damaged evidence. A timeout, interrupted
run or partial matrix cannot be counted as a pass. Retain failed attempts rather
than rewriting their logs or relaxing existing acceptance criteria.

The workflow uses private HOME, XDG, Git and temporary state, scrubs ambient
provider credentials and application overrides, and reuses dependency/build
caches. It does not install binaries, publish changes or operate production
services. Fixture isolation is not a host security boundary: native host
facilities still need their own controlled fixtures.

## Verify changes and the verifier

Use focused regressions while implementing, then run the full entry point after
source is stable. Record the tested input identity. A source change after a gate
starts needs validation for the final combined source. Documentation-only
follow-ups do not change the executable identity.

For verification infrastructure, demonstrate detection and exact restoration in
a disposable source copy:

```sh
./scripts/verify.sh --self-test
```

The self-test first passes the watcher assertion, introduces a real production
interval defect, proves the existing test fails, restores exact bytes, and proves
the test passes again. Its receipt is separate from the complete workspace gate.

An optional reviewed repair loop is available:

```sh
python3 scripts/verify-loop.py --output scratch/verify-loop/my-change \
  --repair-command 'your-reviewed-repair-command'
```

Without a repair command it runs verification and returns evidence for diagnosis.
Automatic repairs are bounded to three attempts per failure and cannot rewrite
established tests, fixtures, snapshots or verification criteria. Stop on an
external blocker or exhausted repair rather than claiming success.

## Comparisons

A comparison needs a complete passing baseline on a comparable platform with
compatible evaluation criteria:

```sh
./scripts/verify.sh --output scratch/verify/baseline
./scripts/verify.sh --output scratch/verify/candidate \
  --baseline scratch/verify/baseline/report.json
```

The evaluator rejects lost tests, new ignores and gross runtime/resource
regressions. Single-run terminal timings and peak memory are smoke measurements;
cache-dependent build durations are not performance benchmarks. Live model
comparisons need paired tasks and separate request/cost accounting. See
[publication validation](PUBLICATION_VALIDATION.md) for the checks actually run.
