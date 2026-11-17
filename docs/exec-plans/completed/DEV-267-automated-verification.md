# DEV-267: automated development verification and feedback

## Purpose and acceptance

Implement the attached user request as a small reusable unattended workflow for
the Rust CLI/TUI workspace. A build is insufficient: required checks include the
existing default/all-feature suites, strict format/lint, release execution,
loopback protocol and real terminal behavior. Preserve failure diagnostics,
separate infrastructure failures, bound repair to three evidence-led attempts,
protect established test criteria, and demonstrate a genuine intentional defect
in a disposable source copy followed by restoration and a passing gate.

## Starting state and scope

Application root is `/Volumes/projects/ca/ca`; parent is a container. Existing
questionnaire changes are dirty and retained. Initial status and patch are
preserved under `scratch/verification-discovery/`. Rust 1.97.1, Cargo, Python 3,
Git and existing stdlib terminal fixtures are available. `.agent/PLANS.md` is
absent; use the existing purpose/decisions/validation/outcome plan convention.
No new package dependency, live provider, production runtime mutation, service
installation, or network-backed `.ca` execution is part of this testing task.
The user's current no-production-mutation requirement governs over historical
automatic binary installation instructions for this task.

## Decisions

- `scripts/verify.sh` invokes a stdlib Python runner with fresh private runtime
  directories, scrubbed application environment, pinned locked/offline Cargo
  commands and retained stage logs plus machine-readable results.
- Full matrices run one test thread following current repository precedent.
- A separate evaluator judges actual logs/receipts and optional measured baseline
  comparisons. Failed runs remain failures; no automatic rerun hides them.
- A repair driver accepts an explicit repair command, checks protected criteria
  and source changes, keeps all attempts, and stops after at most three repairs.
- Agent instructions require the workflow for ordinary development. Inspect
  supported Codex hooks before adding any optional project lifecycle integration.
- Validator demonstration copies source with dirty/untracked changes into a
  disposable tree, changes production behavior only, proves an existing test
  fails, restores the exact original bytes, then proves the check passes.

## Work and validation log

- Discovery complete; implementation, runtime probes and independent isolation
  review delegated in parallel. Existing tests and behavior are the criteria.
- Required: runner contract tests, independent review, full real workspace gate,
  passing/failing/restored defect demonstration and retained evidence paths.
- Implemented the stdlib runner, real terminal probe, separate evidence evaluator,
  bounded protected repair driver and optional project SessionStart/Stop hooks.
- Independent review corrected source compilation being misclassified as
  infrastructure when dependent runtime could not execute; tests now retain
  failed compiler diagnostics and separately reject malformed evidence. Also
  corrected interruption cleanup for the PTY child's separate process group and
  capped hook continuation chains even when Codex changes the turn ID.
- Stable workflow contract suite: 64 passing checks, no skips. Project hook
  commands produce valid protocol JSON for direct SessionStart/Stop execution;
  actual Codex dispatch/trust has not been claimed.
- `scratch/verification-demonstration-final/result.json`: passing original
  existing watcher test, detected production minimum-interval defect (exit 101,
  exact assertion at types.rs:302), restored passing test. Source digest
  `7f923dbd4628b15c7e76cbebfadde832786a0ddbf37ae01a7002fa9f29932719`
  matches both restored copy and untouched original. The first demonstration's
  source-stability rejection remains in `scratch/verification-demonstration/`;
  this correctly detected concurrent evaluator finalization.
- Full gate `scratch/verify/DEV-267-baseline/` has passed preflight, 64 workflow
  checks, formatting and strict lint; complete Rust matrices/release/runtime and
  independent final audit are in progress. No application source edits were
  made; all nine pre-existing tracked source diffs remain identical.
- Final independent review found volatile Rust panic thread IDs/compile timing
  could evade the repeated-failure shortcut. Deliberately stopped the first full
  attempt during default compilation (infrastructure/interruption exit 2, not
  a test result), retaining its complete earlier successful stages. Normalized
  those fields and owned temp prefixes while preserving test names/observed
  assertion values; 19 repair-driver contracts pass. A final complete gate will
  run against the now-frozen criteria.

## Outcome

Complete. The corrected full entry point exits 0 with all eight required stages
and independent evaluation passing. Default matrix: 3,853 passed, 0 failed,
43 ignored, 0 filtered, 27 targets. All-feature: 4,002 passed, 0 failed,
44 ignored, 0 filtered, 28 targets. All 68 workflow contracts and eight real
release CLI/terminal/protocol/tool cases pass. Full evidence is retained at
`scratch/verify/DEV-267-corrected/`; the deliberate defect demonstration passes
original/failing defect/restored phases at
`scratch/verification-demonstration-frozen/`. Source remains frozen and all
pre-existing tracked implementation/test edits are preserved. No production
installation or runtime mutation was performed.

Optional lifecycle hooks are configured with verified direct protocol handling
and consumption of this actual independent passing report; native dispatch
requires project review/trust. Independent review confirms required target
counts, raw artifact hashes and the production-defect demonstration. Host fixture
coverage, existing ignored cases, native host facilities, lexical criteria
guard limits and single-run performance measurements are explicitly documented.

## Evidence-led environment corrections

- The original full run's main target finished with 2,982 passed, 21 failed,
  42 ignored and zero filtered. The runner had put `TMPDIR` beneath
  `/private/tmp`, inside the ordinary sandbox's deliberate `/tmp` allowance and
  broad `/tmp` ownership fixtures. Both sandbox failures reproduce there; all
  21 unchanged exact cases pass under the corrected short private
  `/private/var/tmp` root. Independent review verifies all 23 retained log hashes
  in `scratch/verification-temp-repair/result.json`.
- The native isolation fixtures also reject a symlinked Cargo registry. Both
  representative existing tests fail against that link and pass against the
  runner's real private dependency-byte copy; four logs and unchanged binary
  identity are independently checked in
  `scratch/verification-cache-repair/result.json`. User Cargo configuration and
  credentials are not copied, and fixture writes cannot change the user's cache.
- Interrupting the known faulty environment exposed a surviving test child:
  Cargo exited before its process group finished. Group-aware termination now
  waits, escalates after a bounded grace and stops log writers before hashing.
  A real spawned child that ignores TERM fails the original contract and passes
  after repair; independent execution also passes. The interrupted receipt is
  infrastructure failure, separately from its already-observed assertion
  failures; it is not passing matrix evidence.
- All 68 current workflow contracts pass. Fresh validator demonstration
  `scratch/verification-demonstration-frozen/result.json` independently confirms
  actual pass/intentional defect failure/restored pass, exits 0/101/0 and exact
  preservation of all 1,110 inputs. Frozen source digest is
  `582629ea10476aa30ee7322287c77fca8e21d365c8303ae644478151148314ed`.
  All nine pre-existing tracked source/test diffs are byte-identical to the
  initial patch. Corrected full gate is running in
  `scratch/verify/DEV-267-corrected/` against this frozen tree.
