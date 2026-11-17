# FLT-310 — Whole-site privacy for HTML development

Status: COMPLETE — signed ca 0.1.69 installed and qualified (2026-10-28).

## Outcome

Normal `ca --filter`, combined with the existing filename-alias option, supports
native multi-file website editing without manually selecting paths or values.
The user's installed/enabled 0.1.68 filename feature was first reconciled from its
isolated checkout, with exact baseline matches and before-images preserved in
`scratch/FLT-310/reconcile-0168.json`. Its source remains unchanged there.

HTML/SVG, standalone CSS, JavaScript/TypeScript and JSX/TSX share structural data
masking. Styles, executable logic, static DOM selectors and JSX class/ID values
remain readable. Anonymous directory/file references retain useful asset types.
Source-link records retain original spelling, origin and local target: native
reads open the target, same-source edits preserve exact bytes, copied relative
links rebase to the same target, and directory trailing slashes survive. Newly
inserted discovered references become target-relative URLs. New files can use a
plain model-chosen leaf under an issued directory. Existing real-path permission,
approval, syntax, source-drift and private-registry publication checks remain.
The tool schemas and production settings are unchanged. Filename-only content
behavior remains compatible. Old snapshots load with empty source-link records.

## Verification

Final reviewed Rust file hashes are in `scratch/FLT-310/independent-review.json`.
Source digest `ae651ef20bf0633f774f01e6493ca6c739780b3a868bdaf1bfb6688ddeb616d6` stays unchanged through the
complete gate; the release bump is the only source change after focused checks.

- Focused final: 185 passed, two pre-existing opt-in measurements ignored;
  formatting and strict all-feature binary lint pass. Receipt:
  `scratch/check-change/runs/20261028T223616Z-96213/report.json`.
- Unchanged complete gate/evaluator `scratch/FLT-310/full-1`: PASS, no issues.
  Default: 4660 passed/45 existing ignored;
  all features: 4816 passed/46 ignored.
  Formatting, strict workspace/all-target/all-feature lint, the release build,
  signing and native runtime checks pass. Actual cross-filesystem/fault fixtures
  remain mandatory in both matrices; no criterion or baseline was weakened.
- Exact release CLI, wide 140/narrow 60 TUI and installed-path wide TUI pass.
  Each performs 15 physical loopback requests, anonymous discovery, direct
  source-link reads, exact HTML/CSS/JS edits, unchanged SVG and new-file readback.
  All 60 captured requests exclude checked raw/encoded fixture spellings.
- Both candidate widths and the installed TUI visibly replace CSS with JS while
  HTTP is pending, with 192 history messages. The normal sidepane at width 140
  and request manager at both widths are checked from decoded real terminal
  frames; normal exits restore the terminal. Fixtures/settings remain intact.

## Repairs and retained evidence

The original two acceptance cases fail on 0.1.68. Independent review found and
verified repairs for root-creation snapshot reload, copied-relative targets,
JSX/TSX page text, deleted-alias creation and directory-link trailing slashes.
Every finding has focused regression coverage. Failed baseline/helper/driver
attempts and intermediate source receipts remain under `scratch/FLT-310/`.
The initial helper needed its ordinary isolated temporary-directory access;
the initial TUI driver used an invalid execution-mode flag and sent no requests.
No failed or intermediate-source attempt is labelled final-release proof.
Chronological plan: `scratch/FLT-310/plan-progress-before-completion.md`.

## Release and boundaries

One patch bump, 0.1.68 → 0.1.69; one intended release artifact reused for all final
checks and installation. Candidate/staged/installed SHA-256:
`c7d7ed5efc789312e9c7b9e776d07960e4f0435bb34799fd555a21a9ee3d7a53`. Installed version/help, valid-on-disk signature, mode/owner and exact
hash checks pass. Recovery: `/Users/aji/.local/bin/.ca-release-stage.TLc4uU/ca.recovery-ORC258-20261028T231557Z-de2dccad1258a0e474f02ca517a6dd8b7e834911bded713493e349b883a7860a`.
Background gate session 42918 and the installation job completed; logs and
receipts are in `scratch/FLT-310/`. Only ca was installed. Runtime metadata
unchanged: True (see before/after records).

This qualifies native local source editing through synthetic loopback providers,
not new external-model reasoning. Existing opaque shell/custom/MCP and
alias-bearing-pattern restrictions remain. Binary asset bytes stay original;
image pixels and semantic clues are not concealed by filename aliases. Local
assistant text restores references; tool transcript cards may retain model
aliases. Newly saved source-link records need a version that understands them.
No oracle/reference implementation was copied or changed.

Changed files: `Cargo.toml`, `Cargo.lock`, `src/privacy/mod.rs`, `src/privacy/filenames.rs`, `src/privacy/projection.rs`, `src/privacy/projection/syntax.rs`, `src/privacy/site_tests.rs`, `docs/CONFIGURATION.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`, `ARCHITECTURE.md`, `PARITY.md`, `docs/.INDEX.md`, `docs/exec-plans/completed/filename-alias-prototype.md`, `docs/exec-plans/completed/FLT-310-whole-site-privacy.md`.
Final evidence index: `scratch/FLT-310/final-assessment.json`.

Subsequent real-provider qualification is complete in [FLT-311](FLT-311-real-provider-site.md): 14 native provider requests, exact multi-file edits and link/asset preservation, with encrypted-reasoning capture limits recorded.
