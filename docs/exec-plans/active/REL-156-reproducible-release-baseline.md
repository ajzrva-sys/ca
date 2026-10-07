# REL-156 — Reproducible Release Baseline

Status: ACTIVE
Owner/session: current Codex session
Last updated: 2026-08-23

## Goal

Make the locally healthy `ca`/`codex-team` workspace reproducible from a clean
checkout, with one pinned root validation workflow and one deterministic,
machine-readable release-readiness receipt.

## Non-Goals

- No commit, tag, push, release publication, binary installation, or runtime
  config/session mutation without separate user authorization.
- No feature work, parity claim, provider traffic, credential access, or
  oracle/reference mutation.
- No claim that CI is green until a remote workflow actually runs.
- No destructive cleanup or reset of the existing working tree.

## Evidence and Current Behavior

- The workspace currently passes its local test, format, strict-Clippy, and
  release gates, including the SEC-154/155 privilege suites.
- `Cargo.toml` declares `crates/codex-team` as a workspace member, while
  `git ls-files crates/codex-team` currently reports zero tracked paths.
- The current working tree has 508 tracked changes and 355 exact untracked
  paths. Those bytes predate this plan and must be preserved; exact-path
  inventory uses `--untracked-files=all` rather than Git's collapsed summary.
- No root `.github/workflows` file or Rust toolchain pin exists. The only CI
  file is nested under `crates/codex-team`, so GitHub does not use it for the
  parent repository.
- The local host uses Rust/Cargo 1.97.1.
- `ROADMAP.md`, `TODO.md`, `ENVIRONMENT.md`, and `docs/QUALITY.md` contain
  stale status statements that conflict with the current source/harness.

## Scope

Allowed files:

- `.github/workflows/`, `rust-toolchain.toml`, root and package Cargo manifests
- release-readiness source/tests under `crates/codex-team`
- release validation scripts that contain no secrets or runtime mutation
- `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`,
  `docs/.INDEX.md`, and this plan
- stale status documents only where needed to remove a verified contradiction

Forbidden files/areas:

- `oracles/`, `reference/`, provider credentials, user configuration,
  sessions, installed binaries, tags/remotes, and unrelated dirty changes

Runtime state affected: build output and disposable local test directories
only.

## Design and Invariants

- The repository root owns the validation workflow for both workspace members.
- Exact validation toolchain, lockfile, workspace membership, workflow, release binaries,
  snapshots, protected trees, active plans, and working-tree state are explicit
  readiness inputs.
- The receipt schema is `codex-team.release-readiness.v1`. Its canonical digest
  excludes timestamps, durations, random IDs, and absolute checkout paths.
- Readiness checks use closed argv execution rather than shell interpolation.
- Exit status is 0 for ready, 1 for a completed audit with failed gates, and 2
  for an audit that could not be completed safely.
- A dirty checkout may be audited but cannot report `ready: true`.
- Existing untracked/tracked product bytes are never deleted or silently
  omitted to make the gate pass.

## Milestones

### M1 — Root toolchain and CI contract

- Add a Rust 1.97.1 pin with rustfmt and Clippy.
- Add a root, SHA-pinned, read-only GitHub workflow for formatting, locked
  dependency fetch, offline default/all-feature lint and tests, release builds,
  binary smokes, snapshot/protected-tree hygiene, and final status checks.
- Characterize the workflow locally without claiming a remote pass.

### M2 — Deterministic release-readiness receipt

- Add `codex-team verify release-readiness --repo . --json`.
- Add deterministic receipt/digest tests and mutations for every declared
  readiness input.
- Keep diagnostics bounded and free of absolute paths and raw command output.

### M3 — Clean integration and release-candidate proof

- Classify every pre-existing changed path and ensure all workspace members are
  tracked in a reviewable integration.
- Validate from a fresh clean checkout, then run the root workflow remotely.
- Build twice from a fixed-epoch source archive and compare both binary hashes.
- This milestone remains blocked on explicit Git-history/release authorization.

## Acceptance Criteria

- `rustc --version` and `cargo --version` match the exact validation toolchain.
- The project does not claim an MSRV until a lower-bound matrix establishes it.
- The root workflow validates both binaries and contains no floating action
  references or write permission.
- A fresh checkout contains every Cargo workspace member and passes the exact
  root validation matrix with `--locked` and offline execution after fetch.
- Two unchanged readiness runs have the same canonical receipt digest.
- Dirty state, untracked workspace members, lock/workflow/snapshot drift,
  active-plan inconsistency, or failed release smoke makes `ready` false.
- No validation reads credentials, calls a provider, invokes real sudo, mutates
  user runtime state, or changes protected evidence.

## Validation

Focused:

- workflow/toolchain static checks
- focused `codex-team` readiness tests
- two same-tree receipt comparisons and declared mutation fixtures

Completion:

- `cargo test --workspace --no-fail-fast`
- `cargo test --workspace --all-targets --all-features --no-fail-fast`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo build --workspace --release --all-features`
- release help/version smokes, `git diff --check`, snapshot hygiene, and
  protected-tree status

Manual/visual: none.

Blocked checks: remote CI, clean-clone proof, commit/tag, signing, publication,
and binary installation until separately authorized.

## Progress

- [x] Current repository and privilege-verification state characterized.
- [x] Active ExecPlan and matching task record created.
- [x] Root toolchain and CI contract implemented and validated locally.
- [x] Deterministic release-readiness command implemented and validated.
- [x] Stale status documents reconciled.
- [ ] Clean tracked integration and reproducible RC proof completed.

## Decisions

- Decision: keep `master` and validate pushes to both `master` and `main`.
  Evidence: the local branch is `master`; the nested historical workflow uses
  `main`.
  Consequence: current and future branch naming remain covered without a
  history mutation.
- Decision: treat clean-history construction as a separate authorization gate.
  Evidence: repository instructions forbid commits unless explicitly asked.
  Consequence: this plan can prepare and validate release infrastructure now,
  but cannot honestly mark the clean-integration milestone complete.

## Surprises and Discoveries

- SEC-154 and SEC-155 completed after the initial project review. Interactive
  sudo password handoff is now implemented and hermetically tested on supported
  Unix TUI hosts; service secret handoff remains a separate protocol task.

## Recovery and Rollback

Remove only REL-156-created files and reverse only its narrow manifest/doc
hunks with `apply_patch`. Do not reset, clean, or overwrite the pre-existing
working tree.

## Completion Record

Changed files: `.github/workflows/ci.yml`, `rust-toolchain.toml`, `.gitignore`,
the narrow `crates/codex-team` release-readiness command wiring and tests, and
the root/nested status harness named in Scope.

Commands and results: `cargo test -p codex-team release_readiness
--no-fail-fast` passed 14/14 including same-size artifact mutation and process-
group timeout coverage. Strict workspace all-target/all-feature Clippy,
formatting, default workspace tests, all-feature release build, and both binary
smokes passed. Two unchanged dirty-tree audits exited 1 with the same receipt
digest and exactly three blockers: active-plan consistency, working-tree
cleanliness, and untracked workspace/release inputs. The final all-target/all-
feature run had one unrelated flaky `codex-team` clean-filter exact-count
assertion; its focused rerun passed and REL-160 tracks it. The pinned checkout
SHA was verified as the actions/checkout v4.2.2 release commit.

Remaining risks: untracked workspace member, dirty integration, no remote CI,
and no clean-build reproducibility receipt.

Harness updates: active plan and task/index pointers created.
