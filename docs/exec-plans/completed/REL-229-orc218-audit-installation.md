# REL-229 — ORC-218 Integrated Audit and Installation

Status: COMPLETE
Owner/session: ORC-218 goal session
Last updated: 2026-08-29

## Goal

Reconcile every ORC-218 child, run the complete repository validation and
protected-tree audit, compare the final checkout to the frozen baseline, and
perform the one authorized recoverable release installation.

## Non-Goals

- No feature expansion, provider request, hosted-service check, paid traffic,
  oracle/reference mutation, automatic commit/push/merge, or runtime profile,
  configuration, or session mutation.
- Do not claim runtime parity for oracle bundles that were not runnable.

## Baseline and Ownership

- Frozen status: 868 records in
  `docs/exec-plans/completed/ORC-218-dirty-baseline.status.gz.b64`.
- Frozen SHA-256:
  `2dcaf84dc86d3ae27bfea593bc811ce16f46f9782382780901fdcf890ff39978`.
- Existing unrelated and active-plan changes remain owned by their original
  work. `oracles/` and `reference/` remain read-only.

## Integrated Gates

Run both serial workspace matrices, formatting, strict workspace all-target
all-feature Clippy, workspace all-feature release, help/version and every new
command/schema smoke, strict code-signature verification, diff/snapshot
hygiene, direct protected-tree inspection and verifier, orphan-process checks,
and exact baseline comparison. Record actual results only.

## Installation

Only after every gate passes, preserve `/Users/aji/.local/bin/ca` as a
timestamped full-SHA recovery copy, stage the release candidate on the
destination filesystem, and atomically install. Candidate/staged/installed
SHA-256, regular-file ownership/mode, strict signature, path lookup,
help/version, and representative local commands must match. Restore the prior
binary if any integrity check fails.

## Progress

- [x] REL-229 child plan created after every stable child and PRV deferral.
- [x] Harness/capability audit reconciled with the implemented/deferred split.
- [x] Both serial workspace matrices, formatting, strict Clippy, release,
  signature, smokes, diff, and snapshot gates pass.
- [x] Baseline/protected/orphan audit passes after the separately authorized
  exact PRT-230 manifest commit.
- [x] Recoverable installation and post-install verification pass.
- [x] Umbrella plan and goal completed.

## Validation Record

- Default serial workspace matrix: pass; CA 2,371 passed/4 ignored,
  codex-team 652/1 ignored, MCP stdio 8/8, all other targets pass.
- All-target/all-feature serial matrix: pass; CA 2,400/4 ignored, reasoning
  experiment 118/1 ignored, codex-team 652/1 ignored, all other targets pass.
- Formatting, strict workspace all-target/all-feature Clippy, workspace
  all-feature release, help/version and new-command/schema smokes, strict
  candidate signature, `git diff --check`, snapshot hygiene, and exact-name
  orphan checks: pass.
- Candidate: regular owner-matching mode-0755 arm64 executable, SHA-256
  `3ed7b2347f22671d2d45e25fd3d640309ecf6a734378e14fbdbff610409d0392`.
- Final frozen-to-current inventory: 868 to 908 records; current sorted
  SHA-256
  `5b4b82652500f967716197626567f8ab3096d2053012cc5942f0d603db208f3a`.
  The only removed record is AGT-157's active-plan path, paired with its
  completed-plan addition. The 41 additions are ORC-218/PRT-230-owned.
- Protected verifier: pass after exact manifest-only commit
  `25c936f37e545cb0b4ccfdbbcb397d216a497e83`; direct Git status/diff for both
  protected roots remains empty.
- Read-only recovery audit: an independent implementation reproduced the exact
  current Merkle root, then virtually tested all three ordinary same-size local
  candidates; none matches the committed root. A 2026-08-28 22:28:51 local
  Time Machine snapshot predates the drift, but its protected backup volume was
  not mounted, extracted, or restored.
- PRT-230 replacement validation: protected unit 6/6, CLI 3/3, codex-team
  652/1 ignored, default and all-feature workspace matrices clean, strict
  Clippy and release clean. Applied v2 manifest digest:
  `eeb36e1d625884a375ffa48fe244491902d198b08a9054f7ce572a16ebceb4dc`.
- Installed candidate SHA-256:
  `3ed7b2347f22671d2d45e25fd3d640309ecf6a734378e14fbdbff610409d0392`.
  The timestamped recovery copy preserves prior SHA-256
  `30660c4e21598363feb3c99d11f271716f84e6f3e95907d07f633faa0af83111`.
  Candidate, staged, and installed bytes match; regular-file ownership/mode,
  strict signature, path lookup, help/version, repo JSON, and representative
  new-command smokes pass.

## Outcome

Oracle/reference bytes are unchanged, exact Finder metadata is excluded, the
HEAD-trusted verifier passes, and the recoverable release installation is
verified. No provider or hosted-service request ran.

## Recovery

Reverse only REL-229 documentation hunks with `apply_patch`. Installation
recovery uses only the exact recorded pre-install backup. Never reset, clean,
or overwrite unrelated checkout state.
