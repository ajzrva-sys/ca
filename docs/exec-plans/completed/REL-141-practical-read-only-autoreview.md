# REL-141 — Practical Read-Only AutoReview

Status: COMPLETE
Owner/session: Codex / current user-requested approval repair
Last updated: 2026-08-18

## Goal

Make the ordinary read-only discovery pipeline shown by the user (`find src
-maxdepth 3 -type f | sort`) run under AutoReview without an approval prompt,
while retaining approval for every `find`/`sort` mode that can delete, execute
another program, or write to a caller-selected file.

## Non-Goals

- Do not auto-approve unknown programs, arbitrary shell syntax, background
  commands, network commands, Git/Cargo, or general executable wrappers.
- Do not weaken explicit Ask/Deny rules, unsafe-shell precedence, sandboxing,
  workspace containment, service authority, or outbound approval.
- Do not mutate runtime configuration, install a binary, modify oracle or
  reference evidence, or claim Cursor parity.

## Evidence and Current Behavior

- The user supplied a TUI screenshot in which AutoReview prompts for
  `find src -maxdepth 3 -type f | sort` in `/Users/aji/project/ca`.
- `src/permission.rs::auto_review_safe_stage` uses an exact utility allowlist.
  It recognizes neither `find` nor `sort`, so both stages fail classification.
- The REL-135 classifier already parses a bounded closed pipeline and rejects
  redirects, sequencing, substitution, backgrounding, glob syntax, malformed
  quotes, and unknown utilities before stage-specific checks.
- Inferred: recognizing the utility names without checking their action flags
  would be too broad because `find` can execute/delete/write and `sort` can
  write an output file or execute a compression helper.

## Scope

Allowed files:

- `src/permission.rs` and its focused tests
- `docs/.INDEX.md`, `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`,
  `NEXT_STEPS.md`, and this plan

Forbidden files/areas:

- `oracles/`, `reference/`, runtime config/session/credential files
- unrelated source, snapshots, installed binaries, Git history

Runtime state affected:

- Focused and completion tests may create their existing disposable fixtures
  and bounded local child processes. No user runtime state is changed.

## Design and Invariants

- Keep the closed pipeline parser and exact-program classification boundary.
- Add stage-specific classifiers rather than unconditional utility names.
- `find` is safe only without `-delete`, `-exec`, `-execdir`, `-ok`, `-okdir`,
  `-fls`, `-fprint`, `-fprint0`, or `-fprintf` actions.
- `sort` is safe only without `-o`/`--output` or
  `--compress-program` modes. Ordinary stdout sorting remains safe.
- Existing explicit rules, background rejection, and unsafe shell/network
  boundaries remain unchanged.

## Milestones

### M1 — Characterization

- Add the exact screenshot pipeline and representative single-stage forms to
  the safe classifier fixture.
- Add delete/exec/file-output/helper-program counterexamples.
- Run the focused test and record the expected pre-fix failure.

### M2 — Implementation

- Add small stage-specific argument classifiers for `find` and `sort`.
- Run permission, interactive AutoReview, and headless AutoReview tests.

### M3 — Validation and Harness Reconciliation

- Run the minimum Rust completion gates in `TESTING.md`.
- Reconcile architecture, continuity, testing, next-step, and index records.
- Move this plan to completed only after required checks pass.

## Acceptance Criteria

- `find src -maxdepth 3 -type f | sort` is AutoReview-safe.
- Ordinary `find` predicates/printing and stdout-only `sort` are safe.
- `find` execution, deletion, and file-output actions remain approval-gated.
- `sort` output-file and helper-program modes remain approval-gated.
- Existing rejection fixtures and explicit rule-precedence tests still pass.
- Focused tests and required completion gates report actual results.

## Validation

Focused: PASS. The exact reported pipeline failed the initial characterization
before implementation. After the repair, `cargo test auto_review --
--nocapture` passed five focused main tests and one CLI policy test, and `cargo
test permission::tests -- --nocapture` passed all 10 permission tests.

Completion: PASS. Default tests passed 1,601 main and 37 CLI tests with one
ignore. All-target/all-feature tests passed 1,630 main, 118 experiment, and 37
CLI tests with two documented ignores. Format, strict all-target/all-feature
Clippy, and the all-feature release build pass. Released help/version, diff
check, snapshot hygiene, and oracle/reference diff checks pass.

Manual/visual: the reported command executes in the deterministic headless
fixture; no layout changed, so a native TUI walkthrough was not needed.

Blocked checks: none currently.

## Progress

- [x] Traced the screenshot to the exact closed utility allowlist.
- [x] Defined safe and unsafe `find`/`sort` modes.
- [x] Added failing characterization coverage.
- [x] Implemented the classifier repair.
- [x] Passed focused and completion checks.
- [x] Reconciled harness records and completed the plan.

## Decisions

- Decision: classify command modes, not just executable names.
  Evidence: both utilities have ordinary read modes and explicit modes that
  mutate or execute.
  Consequence: common discovery is frictionless without silently authorizing
  materially different behavior.

## Surprises and Discoveries

- The one displayed pipeline accounts for two consecutive approvals because
  both stages are absent from the current utility policy.

## Recovery and Rollback

All changes are local source/test/document hunks. Revert only the REL-141
hunks; do not reset, checkout, or disturb the broader dirty worktree.

## Completion Record

Changed files: `src/{permission,main,headless}.rs`, `docs/.INDEX.md`,
`ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and this
completed plan.

Commands and results: focused AutoReview and permission suites PASS; default
and all-feature suites PASS; format, strict Clippy, all-feature release,
help/version, diff/snapshot hygiene, and oracle/reference checks PASS.

Remaining risks: AutoReview remains an explicit utility/mode classifier; newly
encountered observational utilities still require classification rather than
silently receiving arbitrary shell authority.

Harness updates: architecture, continuity, testing, next-step, and knowledge
index records reconciled. This is a source-derived `ca` repair, not a Cursor
parity claim.
