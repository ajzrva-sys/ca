# GIT-111 — Approval-Safe `/commit` Workflow

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-15

## Goal

Turn the hidden disabled `/commit` command into an idle-only TUI workflow that
lets the user inspect the current repository, stage or unstage exact whole
files, enter one manual commit subject, confirm the exact staged snapshot, and
create one ordinary local commit without a shell, provider call, push, branch
change, or implicit staging.

## Non-Goals

- Hunk staging, generated messages, multiline bodies, amend, empty commits,
  signoff, author/date overrides, push, pull, PR creation, branch management,
  detached-HEAD commits, or merge/rebase/cherry-pick/revert/sequencer commits.
- Changing `/diff`, tool approval policy, file undo/redo, sessions, config,
  provider behavior, runtime state, or any oracle/reference content.
- Claiming Cursor or Hermes parity. The Hermes surface is engineering evidence
  only.

## Evidence and Current Behavior

- `src/tui/commands.rs` registers `/commit` as an idle-only workspace mutation
  but hides it and describes the unsafe legacy behavior as staging everything.
- `src/main.rs` handles that command only by reporting that it is disabled
  pending approval-safe redesign.
- `GitChangeService`, `ChangeReviewState`, and the `/diff` overlay already load
  porcelain status and tracked diffs asynchronously. They have no mutation,
  repository-state, commit-subject, or confirmation boundary.
- Baseline HEAD is `b426f5ca7a951f61ba1d78c20049fd0f80719e8f` on clean local `master`.

## Scope

Allowed files: narrow changes to `src/changes.rs`, `src/app.rs`, `src/main.rs`,
`src/tui/{commands,input,render,render_tests}.rs`, focused snapshots, this plan,
and required harness documents.

Forbidden: `oracles/`, `reference/`, config/session/provider/tool schemas,
runtime config/credentials, installed binaries, network/provider traffic, and
unrelated formatting or refactors.

Runtime state affected: the repository index and one local commit, only after
explicit stage/unstage keys and a default-cancel final confirmation. Workflow
state is process-only and is never serialized.

## Design and Invariants

- `/commit` accepts no arguments and opens the existing change-review surface
  in commit mode. `/diff` remains read-only and behaviorally unchanged.
- Commit mode resolves the repository root and named branch, allows an unborn
  named branch, and refuses detached HEAD, conflicts, and active special Git
  operations before offering mutations.
- `s` immediately stages the selected whole path; `u` immediately unstages it;
  `r` refreshes; Enter opens a tracked diff; `c` opens the subject editor;
  Esc closes without rolling back intentional index changes.
- Selected Git paths retain exact raw identity distinct from terminal-sanitized
  labels. Every Git command uses direct arguments and literal pathspecs, never
  a shell or a lossy display path.
- The manual subject is one trimmed nonempty line of at most 256 Unicode scalar
  values with no control characters. It is passed as the value after `-m`.
- Confirmation defaults to Cancel and owns a snapshot of the named branch,
  optional parent HEAD, index tree hash, and complete staged manifest. Commit
  execution revalidates branch, HEAD, and tree immediately before invoking Git
  and verifies the resulting tree and parent afterwards.
- Git hooks, filters, signing, and author configuration remain active. Commands
  use the configured finite timeout, bounded sanitized output, an owned process
  group, and post-cancellation/timeout HEAD reconciliation. Unknown outcomes
  never retry or reset automatically.
- No mutation enters file undo/redo, canonical messages, provider history,
  session persistence, or config.

## Milestones

### M1 — Characterize the disabled command and Git safety boundary

- Add failing command, service, input, and rendering tests for the approved
  contract before enabling behavior.
- Cover ordinary, initial, detached, conflict, special-operation, stale, and
  exact-path cases in disposable repositories.

### M2 — Implement bounded Git mutations and commit verification

- Add typed repository/commit snapshots and stage, unstage, subject validation,
  commit, timeout, cancellation, and postcondition behavior to the Git service.
- Prove only the confirmed index tree is committed and unrelated working/index
  state is preserved.

### M3 — Integrate command, TUI state, rendering, and orchestration

- Add commit-mode state and async input actions/events without blocking the TUI.
- Add list/diff/editor/confirmation/busy/error wide and narrow coverage.

### M4 — Complete local and FreeBSD validation

- Run focused suites, default and all-feature suites, formatting, strict
  all-feature Clippy, release build, binary smokes, diff checks, and snapshot
  hygiene.
- Run the exact tree in a disposable physical-FreeBSD checkout when available
  and perform a disposable no-network TUI walkthrough.
- Reconcile the harness and archive this plan only after all required evidence
  is recorded.

## Acceptance Criteria

- `/commit` is visible, idle-only, argument-free, and never stages implicitly.
- Stage/unstage affects only the selected literal whole path and preserves
  unrelated index and all working-tree bytes.
- The final confirmation truthfully names every staged path and defaults to
  cancellation. Drift, detached/special/conflicted state, invalid subjects,
  empty indexes, and Git failures refuse without an automatic retry or reset.
- A successful commit has exactly the confirmed tree and expected parent;
  unstaged changes remain; no push, model call, session/config mutation, or
  file-undo entry occurs.
- `/diff` and all existing completion gates remain green on macOS and, when
  available, physical FreeBSD.

## Validation

Focused:

- The command visibility/classification characterization failed while
  `/commit` was hidden, then passed after implementation.
- `cargo test changes::tests`: PASS, 20 tests.
- `cargo test commit_`: PASS, 19 matching tests.
- `cargo test working_tree_`: PASS, 3 unchanged `/diff` cases.
- Fourteen new wide/narrow snapshots were reviewed; no `.snap.new` remains.

Local completion:

- `cargo test --no-fail-fast`: PASS, 1,332 main and 36 CLI tests; one
  documented manual case ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,361 main,
  118 experiment, and 36 CLI tests; the two documented cases ignored.
- `cargo fmt --all -- --check`, strict all-feature Clippy, all-feature release
  build, release help/version, `git diff --check`, and snapshot hygiene: PASS.
- Release SHA-256:
  `c704629724be722749945968808248cb746324f1887d1e72e3c16560fd5060a2`.

Physical FreeBSD 15.1-RELEASE-p2 amd64, Rust/Cargo 1.96.1, unprivileged:

- Exact source archive SHA-256:
  `8642f812a140ee0476c3e51e08a6f3a18c8e6c8fab3976d133aaa4f1fcf8a03d`.
- Focused Git service: PASS, 20 tests.
- Locked default: PASS, 1,319 main and 36 CLI tests; one ignored.
- Locked all-feature targets: PASS, 1,348 main, 116 experiment, and 36 CLI
  tests; one main and one experiment case ignored. The experiment target was
  rerun after removing 772 untracked AppleDouble files generated from macOS
  archive metadata; its exact-source result was 116 passed, 1 ignored.
- Native formatting, strict all-feature Clippy, release build, help/version,
  ELF/linkage, and snapshot hygiene: PASS. Release SHA-256:
  `d1ff2b32e57515efb1aaf3dede9cbdc1cae3adcbecc3991eb48bcead05b3cdfa`.

Manual/visual:

- PASS in a disposable 80x24 PTY with the local release binary, bare profile,
  no session, empty temporary home, no network, and a disposable repository.
  The walkthrough staged two exact files, retained `Walkthrough café` after
  default cancellation, committed only after explicit confirmation, verified
  parent/tree/files, and restored the alternate screen. The temporary home
  remained empty.

Blocked checks: none.

## Progress

- [x] Approved behavior and safety decisions recorded.
- [x] Failing characterization tests added.
- [x] Git service implemented and focused tests passing.
- [x] TUI/orchestration integrated with reviewed snapshots.
- [x] Local completion gates passing.
- [x] Physical-FreeBSD gates and disposable walkthrough passing.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: use immediate whole-file index mutations and commit only the live
  staged snapshot.
  Evidence: the owner selected whole files and immediate staging; mixed
  staged/unstaged paths retain ordinary Git semantics.
  Consequence: Esc never pretends to roll back index changes.
- Decision: accept one manual subject and no provider-assisted generation.
  Evidence: the owner selected the smallest no-network message surface.
  Consequence: GIT-111 adds no request, usage, pricing, or credential path.
- Decision: allow named unborn branches but refuse detached and special states.
  Evidence: the owner selected normal commits only and explicit detached refusal.
  Consequence: complex history operations remain direct-Git work.

## Surprises and Discoveries

- A macOS-created transfer archive can materialize extended attributes as
  untracked AppleDouble `._*` files on FreeBSD. The experiment fixture loader
  correctly rejected the 772 binary extras as an inexact UTF-8 fixture. After
  deleting only those generated files from the disposable extraction, all 116
  applicable experiment tests passed. No source change was required.
- Staging the old side of an already-recognized rename with ordinary `git add`
  can report an absent path. Commit mode therefore uses literal pathspecs for
  both identities and an explicit safe index removal for the old side before
  staging the new side.

## Recovery and Rollback

Before a commit succeeds, remove commit-mode state/actions and the Git mutation
methods; intentional stage/unstage changes are ordinary user-owned index state
and are not automatically reverted. After a successful disposable test commit,
delete only the disposable repository. Never reset or rewrite the user's real
repository to recover this implementation.

## Completion Record

Changed files:

- Git state/service and orchestration: `src/changes.rs`, `src/main.rs`.
- Command/input/rendering: `src/tui/commands.rs`, `src/tui/input.rs`,
  `src/tui/render.rs`.
- Tests: `src/tui/input_tests.rs`, `src/tui/render_tests.rs`, and 14
  `commit_review_*` snapshots.
- Harness: `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and this completed
  ExecPlan. `PARITY.md` was reviewed and intentionally left unchanged because
  no real Cursor parity conflict was discovered.

Commands and results: all focused, local completion, physical-FreeBSD, binary,
and manual checks listed in Validation passed. `git diff --check` and snapshot
hygiene pass. No binary was installed and no implementation commit was made.

Remaining risks: ordinary user Git hooks, signing, filters, and author policy
can still reject a real commit by design; diagnostics remain sanitized and the
draft/index remain available for deliberate recovery. A stock versioned Cursor
runtime remains unavailable, so this feature makes no parity claim.

Harness updates: GIT-111 is recorded complete, the stale physical-FreeBSD gap
is closed, and CTX-112, SES-113, TUI-114, TUI-115, and MD-116 remain separate
unselected candidates.
