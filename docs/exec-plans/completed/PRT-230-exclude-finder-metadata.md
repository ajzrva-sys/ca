# PRT-230 — Exclude Finder Metadata from Protected Trees

Status: COMPLETE
Owner/session: resumed ORC-218 goal session
Last updated: 2026-08-29

## Goal

Exclude exact `.DS_Store` path components from codex-team protected-tree
baselines and verification so volatile Finder metadata cannot block otherwise
unchanged ignored evidence trees, then regenerate the trusted manifest and
finish REL-229 validation and installation.

## Scope and Invariants

- Change only the protected-tree scanner, focused tests, the tracked protected
  manifest, and ORC/codex-team reconciliation documents.
- Exclude an entry only when its exact basename is `.DS_Store`, at any depth
  below `oracles/` or `reference/`. Similar names remain protected.
- Do not read, rewrite, remove, or otherwise mutate any file under `oracles/`
  or `reference/`.
- Retain no-follow traversal, regular/symlink/hardlink/special-file checks,
  entry/byte bounds, double-scan stability, HEAD anchoring, and exact
  revision/digest-bound manifest publication.
- Regenerate the manifest only after the focused exclusion regression passes
  and the preview is stable. The user's explicit instruction to exclude the
  file authorizes this narrowly scoped manifest transition.

## Progress

- [x] Add failure-first unit and CLI coverage for mutable root/nested
  `.DS_Store` entries and protected similarly named files.
- [x] Implement the exact-component exclusion without weakening other scans.
- [x] Regenerate the versioned manifest through exact revision/digest-bound
  apply without touching either protected tree.
- [x] Commit only the manifest so HEAD can trust it, then verify protected
  trees.
- [x] Pass focused codex-team tests and both required workspace matrices,
  formatting, strict Clippy, and release build.
- [x] Reconcile PRT-230, REL-229, ORC-218, root/codex-team continuity, testing,
  queue, and completed-task records.
- [x] Complete the recoverable CA installation and post-install verification.

## Surprises & Discoveries

- The current oracle tree contains four `.DS_Store` files, so filtering the
  exact basename at traversal time removes four regular entries and 43,024
  bytes while leaving every evidence path covered.
- Protected verification deliberately reads the manifest from `HEAD`, not the
  worktree. The user explicitly authorized the exact manifest commit, which
  preserved the boundary without trusting dirty bytes.
- The first replacement workspace matrix reproduced the separately tracked
  REL-160 clean-filter assertion. Git validly called the filter once rather
  than twice. The test now checks the actual invariant—one or two staging-time
  calls, never a third—while retaining exact commit/index/HEAD checks.

## Decision Log

- Use algorithm identity `sha256-merkle-v2-ds-store-excluded` so changed hash
  semantics are never mislabeled as v1.
- Filter exact basename components with `WalkDir::filter_entry` before counts,
  type inspection, or hashing. This also prevents traversal into a directory
  named `.DS_Store`; similar names remain protected.
- Preserve HEAD-only manifest trust. Commit `25c936f37e545cb0b4ccfdbbcb397d216a497e83`
  contains only the exact regenerated manifest after separate user authority.

## Context and Orientation

- Scanner and unit coverage:
  `crates/codex-team/src/verification/protected_tree.rs`.
- Real CLI coverage: `crates/codex-team/tests/protected_baseline_cli.rs`.
- Trusted root manifest: `.codex-team-protected-trees.json`.
- Integrated release plan:
  `docs/exec-plans/completed/REL-229-orc218-audit-installation.md`.

## Plan of Work

Characterize the old failure, filter only the exact Finder filename, bind the
semantic change to v2, regenerate with the existing double-scan apply command,
run focused and integrated gates, commit only when explicitly authorized, then
verify and resume REL-229 installation.

## Concrete Steps

The protected-tree unit and CLI suites, both serial workspace matrices,
formatting, strict workspace all-feature Clippy, workspace all-feature release,
HEAD-trusted verifier, hygiene/process checks, and REL-229 installer all pass.

## Validation and Acceptance

- Failure-first regression: observed four entries instead of the expected two.
- Protected-tree unit suite: 6/6 pass.
- Protected-baseline CLI suite: 3/3 pass.
- Full codex-team package: 652 passed/1 documented ignore; MCP stdio 8/8.
- Corrected clean-filter test: ten consecutive focused passes.
- Replacement default matrix: pass; CA 2,371/4 ignored, codex-team 652/1
  ignored, MCP stdio 8/8.
- Replacement all-target/all-feature matrix: pass; CA 2,400/4 ignored,
  experiment 118/1 ignored, codex-team 652/1 ignored, MCP stdio 8/8.
- Formatting, strict all-target/all-feature Clippy, and all-feature release:
  pass.
- HEAD-trusted protected-tree verifier: pass; `oracles/` and `reference/` are
  unchanged.
- Candidate/staged/installed SHA-256, strict signature, ownership/mode, path,
  help/version, and representative command smokes: pass.

## Recovery

Reverse only PRT-230-owned source, test, manifest, and documentation hunks with
`apply_patch`. Never reset or clean the shared worktree. Installation recovery
uses REL-229's exact timestamped backup.

Preview is read-only and apply is safe to repeat only after restoring the
tracked manifest to its clean HEAD bytes. The current applied manifest is
revision/digest bound. No protected content was mutated.

## Artifacts and Notes

- Applied manifest digest:
  `eeb36e1d625884a375ffa48fe244491902d198b08a9054f7ce572a16ebceb4dc`.
- v2 oracle root: 42,161 entries, 36,748 regular files, 5,342 directories,
  71 symlinks, 1,401,108,346 bytes, SHA-256
  `bd0de373003f954cc1d3701765368f38810ad9ed3ad08059a76002ffb5d2189e`.
- CA release candidate remains SHA-256
  `3ed7b2347f22671d2d45e25fd3d640309ecf6a734378e14fbdbff610409d0392`.

## Interfaces and Dependencies

No public CLI or CA API changes. The protected manifest algorithm identifier
changes from v1 to v2; the existing preview/apply/verify command schemas remain
unchanged.

## Outcomes & Retrospective

Implementation, exact manifest trust, validation, plan reconciliation, and the
recoverable installation are complete. No protected-tree byte was changed.
