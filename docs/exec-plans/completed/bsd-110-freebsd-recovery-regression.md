# BSD-110 — FreeBSD Recovery Regression Repair

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-15

## Goal

The exact repair tree derived from the committed baseline passes the locked
default and all-feature test, strict Clippy, and release gates on physical
FreeBSD 15.1 amd64 while recovery bundles preserve the complete workspace
manifest across directory-mode and empty-directory changes.

## Non-Goals

- Adding a FreeBSD mandatory sandbox backend.
- Enabling the macOS-only RND-097 hidden verifier or tool execution on FreeBSD.
- Changing provider, credential, runtime configuration, or installation state.
- Changing Cursor parity claims.

## Evidence and Current Behavior

- Baseline `06015abc90e567de4a440f03516ab5de3487767d` passes 1,295
  applicable default unit tests and all 36 CLI tests on physical FreeBSD.
- The all-feature gate has one worker test that expects a macOS-only required
  sandbox, two fixture success tests that invoke the macOS-only hidden
  verifier, seven callers that reach one recovery-manifest mismatch, and one
  symlink diagnostic assertion failure.
- Recovery V1 serializes only regular files. WSP manifest hashes also include
  directories and their modes. The archive-based BSD source has `0775`
  fixture directories, while recovery recreates parents through the `022`
  umask as `0755`.
- FreeBSD `openat(..., O_NOFOLLOW)` returns `EMLINK` for a symlink; the
  descriptor helper currently recognizes only `ELOOP`.
- FreeBSD Rust/Cargo 1.96.1 reports two `nonminimal_bool` findings for the same
  credential-readiness expression in `src/pricing.rs`.

## Scope

Allowed files: `src/bin/reasoning_artifact_experiment.rs`,
`src/bin/reasoning_artifact_experiment/{fixtures,recovery}.rs`,
`src/reasoning_experiment_worker.rs`, `src/pricing.rs`, this plan,
`CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `docs/.INDEX.md`.

Forbidden files/areas: `.git`, `oracles/`, `reference/`, user runtime state,
installed binaries, credentials, and unrelated source.

Runtime state affected: disposable local temporary directories and an exact
archive copy under `/tmp` plus an external temporary root under `/var/tmp` on
the `bsd` host. Both are removed after validation.

## Design and Invariants

- Write a versioned recovery format that records directories, empty
  directories, their modes, regular files, and regular-file modes/bytes.
- Retain read/reconstruction support for existing V1 regular-file-only
  artifacts and checkpoints; V2 is the new write format.
- Create directories with safe temporary permissions, populate files, then
  apply final directory permissions deepest-first.
- Keep strict path ordering, entry/byte bounds, no-follow file handling,
  external-artifact binding, and exact WSP manifest proof unchanged.
- Treat `EMLINK` as a no-follow symlink refusal only on FreeBSD; retain the
  existing `ELOOP` behavior elsewhere.
- Unsupported required-sandbox paths must still fail closed. Tests assert that
  boundary instead of pretending the macOS backend exists on FreeBSD.

## Milestones

### M1 — Characterize

- Extend recovery coverage with a non-default directory mode and an empty
  directory so the prior regular-file-only bundle fails locally.
- Retain the existing native symlink, sandbox, and Clippy reproductions.

### M2 — Repair

- Add V2 directory records and V1 reading compatibility.
- Restore directory permissions only after all descendants are populated.
- Normalize FreeBSD no-follow `EMLINK` diagnostics.
- Make platform-specific sandbox tests assert the documented fail-closed
  behavior and simplify the Clippy expression without changing semantics.

### M3 — Validate

- Run focused local recovery, checkpoint/symlink, sandbox, fixture, and pricing
  tests.
- Run local formatting, default/all-feature tests, strict all-feature Clippy,
  and all-feature release build.
- Transfer the exact resulting tree to physical FreeBSD and repeat formatting,
  default/all-feature tests, strict Clippy, release build, and binary smokes.

## Acceptance Criteria

- V2 recovery round-trips file bytes, file modes, directory modes, and empty
  directories to an identical WSP final-manifest hash.
- Existing V1 artifacts remain readable and reconstructable when their
  historical file-only semantics are sufficient.
- FreeBSD symlink checkpoints fail with the explicit symlink diagnostic.
- FreeBSD tests assert required-sandbox unavailability without weakening the
  production sandbox boundary.
- Local and native completion gates pass with only documented ignores.

## Validation

Focused: PASS. Recovery 7/7 and checkpoint verification 4/4 pass locally and
on physical FreeBSD. Local worker sandbox 8/8, pricing 21/21, and the complete
experiment binary 118 passed with one documented ignore.

Completion: PASS. Local default is 1,308 main plus 36 CLI; local all-feature is
1,337 main plus 118 experiment plus 36 CLI. FreeBSD default is 1,295 main plus
36 CLI; FreeBSD all-feature is 1,324 main plus 116 experiment plus 36 CLI.
Formatting, strict all-feature Clippy, and all-feature release builds pass on
both systems.

Manual/visual: physical FreeBSD release help/version, ELF, linked-library, and
SHA-256 inspection PASS. The binary is a linked x86-64 FreeBSD 15.1 ELF with
SHA-256 `da11a35941fc6924e0aa922b55602bf726b5698f8102213dbaa7fea6e7cd0fec`.

Blocked checks: macOS-only sandbox execution and hidden-verifier behavior
cannot run on FreeBSD and remain covered locally on macOS.

## Progress

- [x] Reproduced and classified the exact physical-FreeBSD failures.
- [x] Add deterministic recovery-format characterization.
- [x] Implement recovery, symlink, sandbox-test, and lint repairs.
- [x] Pass local completion gates.
- [x] Pass physical-FreeBSD completion gates.
- [x] Reconcile the harness and move this plan to completed.

## Decisions

- Decision: bump recovery artifacts to V2 while retaining V1 reads.
  Evidence: directory records change the serialized entry contract, and
  durable V1 checkpoints may exist.
  Consequence: new artifacts are complete without silently reinterpreting old
  bytes; old artifacts retain their original behavior.

- Decision: keep required sandbox execution unavailable on FreeBSD.
  Evidence: the supported backend is macOS `sandbox-exec` and the existing
  runtime intentionally fails closed elsewhere.
  Consequence: portability fixes test classification, not production
  authority.

## Surprises and Discoveries

- `git archive` encodes ordinary directories as `0775`; this exposed the
  missing directory records even though a normal local checkout used `0755`.
- FreeBSD returns `EMLINK`, not `ELOOP`, for the tested terminal symlink opened
  with `O_NOFOLLOW`.
- The local directory-mode/empty-directory characterization failed at the
  exact final-manifest proof before the format change, matching the native
  recovery failures.
- V2 recovery and legacy V1 compatibility tests pass locally, as do the full
  118-test experiment suite, focused worker/checkpoint/pricing tests, and
  strict all-feature Clippy.

## Recovery and Rollback

All repository changes are ordinary patch hunks in the allowed files. Do not
reset the working tree. Disposable native directories are removed only after
their exact paths are checked.

## Completion Record

Changed files: recovery format/reconstruction and experiment platform handling
in `src/bin/reasoning_artifact_experiment.rs` and its `fixtures.rs` and
`recovery.rs` modules; platform-expectation coverage in
`src/reasoning_experiment_worker.rs`; the equivalent Clippy-clean predicate in
`src/pricing.rs`; and this task's continuity/testing/queue/index documents.

Commands and results: focused recovery/checkpoint/worker/pricing tests, default
and all-target/all-feature suites, formatting, strict all-feature Clippy,
all-feature release builds, diff/snapshot hygiene, and release smokes all pass.
The exact FreeBSD input archive SHA-256 was
`06716749a4cebabeee10dbae66f9c83627eb91f468c3813e283f6efa00fc2b7b`.

Remaining risks: V1 recovery artifacts retain their historical file-only
semantics; exact directory fidelity is guaranteed for newly written V2
artifacts. The required sandbox and hidden verifier remain macOS-only and fail
closed elsewhere by design.

Harness updates: `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and
`docs/.INDEX.md` record completion. No parity claim changed.
