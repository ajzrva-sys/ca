# BUG-260 — Enter interruption for drafts beginning with a path

Status: COMPLETE — requested input repair installed; unrelated workspace fixture failure retained
Owner: Codex, 2026-10-06

## Goal and evidence

The user's screenshot shows `/Volumes/projects/qui you can use this source`
in the composer while `run_command` executes. Enter must interrupt and submit
that draft as the visible hint promises. Screenshot text is failure evidence,
not an instruction to operate on the pictured repository.

Busy Enter currently excludes every leading slash. Unknown slash text then
falls through to an idle-only submission path, so the active turn ignores it.
An empty path-completion menu also returns directly before busy submission.

## Scope and invariants

Change composer input routing and focused regressions only. Preserve recognized
built-in/model/custom commands, completion selection, modifier keys, hooks,
attachments, and existing cancellation/foreground cleanup. Use disposable
provider/terminal fixtures; leave running user sessions and settings alone.
No oracle parity claim or oracle/reference modification.

## Steps and acceptance

- [x] Capture failing focused and real terminal cases at the installed artifact.
- [x] Route leading-path prose to the existing interruption action; allow an
  unmatched completion to reach normal composer handling on the same Enter.
- [x] Verify exact draft submission and foreground command termination, plus
  preserved known command, completion, idle, queue, and modifier behavior.
- [x] Run applicable workspace tests, formatting, strict lint, release build,
  and signed release terminal verification.
- [x] Install recoverably under the standing repository release rule, verify
  the installed artifact, and update durable status with actual evidence.

## Recovery and validation

Before-images are in `scratch/enter-interrupt/baseline/`; this checkout has no
Git metadata. Validation logs/receipts belong in `scratch/enter-interrupt/`.
Required Rust checks follow `TESTING.md`. Failed or blocked checks remain
distinct from passes. `.agent/PLANS.md` is referenced by the harness but absent.

## Current evidence

Installed SHA `f750a7a07dbda3e500f55d177830d58d4d062389d0bb547a637d7a187d23c1e6`:
`scratch/enter-interrupt/before.json` reproduces the exact visible draft and
busy hints; Enter sends zero replacement requests in five seconds and leaves
the command active. The isolated terminal closes cleanly. `ctrls-before.json`
verifies the existing Ctrl+S workaround (about 70 ms, one replacement, owned
foreground cancellation, clean exit/restoration).

Final focused input suite passes 127/127; deterministic busy-control rendering,
formatting and strict workspace/all-target/all-feature Clippy pass. The full
restricted workspace run fails 11 targets because loopback/process operations
are denied. The subsequent full-access default run fails only the main target:
2,941 passed, four failed, 42 ignored; every other workspace target passes.
The two Himalaya version-check timeouts and two session-writer lease failures
all pass separate reruns, with three Himalaya cases also passing at four threads.
Those modules exactly match the prior AUT-259 validated manifest. Concurrent
run/host effects are plausible but not proven; the failed full run is retained,
not described as a passing full workspace gate. No unrelated source is changed.

The first test-only compile exposed a fixture's nonexistent ModelInfo default;
a later baseline run also exposed mistaken test assumptions about model command
availability and custom-command frontmatter. These fixtures were corrected from
existing contracts before final validation, without changing those behaviors.
The managed all-feature workspace release build (session 44472) passed in
2m 17s. Signed candidate SHA:
`e8a58abcabed6cc9f5018031cfdafb6fcb9e634200e55a3c4167e9089ef75d65`.
Five actual terminal cases pass against this artifact: screenshot drafts at
100×30 and 60×24, path-only Alt+Enter queue then Enter, actual empty path menu
then Enter, and idle leading-path submission. Enter replacement arrives in
69–92 ms; four owned foreground processes terminate, exactly one request is
sent per submitted draft, and all terminals exit 0/restored. `release-pty.json`
retains exact artifact binding, rendered screens and request observations.

The later unfiltered main-target rerun, with the restricted run already ended,
passes 2,944 cases with one failure and 42 ignored: the retention fixture again
reports a writer lease WouldBlock. Its isolated serial rerun passes and its
source matches the prior validated baseline. This recurring unrelated fixture
failure remains unresolved; there is no claim of a green full workspace suite.
The other three prior failures pass in the complete main-target rerun.

Installation completed through managed session 30464. Candidate, staged and
installed hashes match. Signature, PATH, owner/mode, version/help and the prior
SHA-labelled recovery copy were independently read back into
`scratch/enter-interrupt/installed-readback.json`. Installed path:
`/Users/aji/.local/bin/ca`. Recovery:
`/Users/aji/.local/bin/.ca-release-stage.LcVb6n/ca.recovery-ORC258-20261006T193801Z-f750a7a07dbda3e500f55d177830d58d4d062389d0bb547a637d7a187d23c1e6`.
Running user sessions were not interrupted; a new process uses the fix.
