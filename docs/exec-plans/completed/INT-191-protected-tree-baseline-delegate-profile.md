# INT-191 — Protected-Tree Baseline and Delegate Profile Activation

Status: COMPLETE
Owner/session: current Codex implementation session
Last updated: 2026-08-26

## Goal

Make `codex-team verify protected-trees` trust an exact, tracked digest baseline
for the ignored `oracles/` and `reference/` trees, then install the validated CA
and codex-team releases and activate the existing delegate facade through one
isolated named CA profile. The release must also include the user-reported
composer correction so horizontal arrows work while inline slash or file
completion is open.

## Non-Goals

- Editing, tracking, copying, or deriving source from `oracles/` or `reference/`.
- Copying saved provider credentials or changing the default CA profile.
- Changing the 11/15/28 MCP catalogs, delegation authority, or provider traffic.
- Committing implementation or documentation changes.

## Evidence and Current Behavior

- `crates/codex-team/src/verification/ca.rs::protected_trees` always reports an
  ignored protected root as unverifiable because no trusted content baseline
  exists.
- Both roots are ignored and absent from `git ls-files`; direct Git status/diff
  reports no protected path change, but cannot establish content identity.
- The current CA configuration has no mechanistic MCP table. The existing
  runtime accepts exact local stdio, network-disabled delegate servers and
  lazily validates the 15-tool/zero-resource catalog.
- The installed CA and codex-team hashes predate the validated INT-190 release.

## Scope

Allowed files: codex-team CLI/application/verification modules and focused
tests; CA composer input and focused input tests; this plan; root and codex-team
continuity/testing/integration docs; `NEXT_STEPS.md`; one root protected-tree
manifest.

Forbidden files/areas: every byte under `oracles/` and `reference/`; unrelated
dirty source; default runtime configuration; provider credential stores;
sessions; services; oracle/reference Git metadata.

Runtime state affected after all release gates pass: recoverable replacements
of `/Users/aji/.local/bin/{ca,codex-team}` and creation of the private named
profile `/Users/aji/.config/ca/profiles/codex-team-delegate.toml`.

## Design and Invariants

- A versioned, bounded manifest at `.codex-team-protected-trees.json` stores
  only aggregate SHA-256 tree roots, counts, byte totals, and approval
  provenance. The verifier trusts only the blob committed at `HEAD`.
- The tree digest is domain-separated and byte-order deterministic. It includes
  directories, regular-file content/size/executable state, and symlink targets
  without following links. Special files, hardlinks, device changes, traversal,
  races, and configured bounds fail closed.
- Preview is read-only and performs two identical scans. Apply requires the
  exact preview digest and repository revision, recomputes, and atomically
  writes only the manifest. No automatic rebaseline exists.
- The initial manifest commit contains that path and no other path. Existing
  index/worktree state is measured before and after and never reset, stashed,
  or cleaned.
- Local activation is bound to validated artifact hashes, a passing protected-
  tree receipt, recoverable backups, and post-install compatibility. A dirty
  checkout remains visible in the publication audit but is not itself a local
  installation veto.
- The named profile copies only non-secret configuration. Saved DeepSeek and
  Mimo references become environment placeholders; no secret bytes are read or
  copied. MCP remains local stdio, network false, delegate-only, and repo-pinned.

## Milestones

### M1 — Characterization and protected-tree contract

- Preserve existing fail-closed coverage and add preview/manifest/hash tests.
- Implement the bounded scanner, HEAD-anchored manifest reader, verifier, and
  digest/revision-bound CLI lifecycle.
- Run focused codex-team verification tests and formatting check.

### M2 — Approved initial baseline

- Preview twice against the approved current trees, apply the exact digest, and
  create one manifest-only commit with hooks/signing disabled.
- Prove commit path isolation, unchanged protected roots, preserved unrelated
  index/worktree state, and a passing protected-tree receipt.

### M3 — Release and runtime activation

- Preserve ordinary composer navigation while slash/file completion overlays
  are open, with focused Unicode/native-navigation regressions.
- Run the complete serial suite, formatting, strict Clippy, release build,
  diff/snapshot/readiness/isolation/real-client checks.
- Recoverably install both releases and create the sanitized named profile.
- Validate lazy profile load, exact installed MCP compatibility, `/delegate`,
  and `/jobs` without provider traffic or persistent background processes.

### M4 — Durable handoff

- Reconcile continuity, testing, integration documentation, and next steps.
- Move this plan to completed with exact commands, hashes, backups, and risks.

## Acceptance Criteria

- An absent, malformed, uncommitted, stale, or mismatched baseline fails closed.
- The committed day-zero baseline makes the unchanged ignored roots pass while
  direct Git checks remain active.
- All protected entry kinds, bounds, races, and digest-bound apply failures are
  covered without mutating the roots.
- The baseline commit contains exactly one path and unrelated dirty/staged
  bytes remain unchanged.
- Installed release hashes match `target/release`; prior binaries are retained.
- The private named profile loads, starts no MCP process during preparation,
  and validates the exact delegate catalog when explicitly inspected.
- Left/Right move the composer cursor under slash, file, and path completion;
  existing Unicode and native Option/Command motion remains intact.

## Validation

Focused: `cargo test -p codex-team protected_tree -- --test-threads=1`,
`cargo test -p codex-team verification -- --test-threads=1`, and exact CLI
integration filters.

Completion: `cargo test --workspace --no-fail-fast -- --test-threads=1`,
`cargo fmt --all -- --check`, strict all-target/all-feature Clippy,
all-feature release build, and `git diff --check`.

Manual/local: release help/version, snapshot hygiene, committed protected-tree
receipt, release readiness, production isolation probe, real 11/15/28 MCP
fixture, named-profile doctor, `/delegate`, and `/jobs`.

Blocked checks: live provider calls and service activation remain forbidden.

## Progress

- [x] Read governing repository and nested codex-team instructions.
- [x] Record initial HEAD, index/status, relevant file digests, runtime hashes,
  ignored-tree shape, and profile constraints.
- [x] Characterize and implement the protected-tree baseline contract.
- [x] Create and verify the authorized manifest-only baseline commit.
- [x] Reproduce and fix horizontal composer arrows swallowed by slash/file
  completion overlays.
- [x] Pass focused tests, the full workspace suite, format, strict lint,
  optimized build, hygiene, isolation, and both real compatibility fixtures.
- [x] Replace the clean-Git local activation veto with artifact-, receipt-, and
  backup-bound installation safeguards.
- [x] Recoverably install releases and create/validate the named profile.
- [x] Reconcile documentation with the completed implementation and runtime
  activation.

## Decisions

- Decision: accept the current ignored-tree bytes as the day-zero baseline.
  Evidence: explicit user authorization; Git has no historical content baseline.
  Consequence: future drift is detectable, but earlier provenance is not proven.
- Decision: trust the manifest from `HEAD`, not the worktree.
  Evidence: an uncommitted manifest must not grant itself authority.
  Consequence: the initial baseline requires one manifest-only commit.
- Decision: use environment placeholders for saved DeepSeek/Mimo credentials.
  Evidence: explicit user choice and named-profile credential isolation.
  Consequence: those routes require environment variables in this profile.
- Decision: treat the reported arrow failure as a release blocker in this run.
  Evidence: explicit user report; focused regressions failed because both
  overlays consumed Left/Right before the composer.
  Consequence: no CA installation occurs until the correction passes all gates.

## Surprises and Discoveries

- Named profiles are whole-file configurations. A byte-for-byte default-profile
  copy would fail because two saved credential references resolve against the
  named profile's separate store.
- The protected roots contain nested Git metadata, large binaries, and symlinks;
  the scanner must stream content and hash symlink targets without following.
- The complete test run was initially launched with a pseudo-terminal for live
  progress. One interactive disposition test inherited that terminal and
  waited for input; closing stdin restored the normal non-interactive harness
  behavior and the suite passed. Future completion runs should use closed or
  piped stdin.
- The publication-oriented release-readiness audit is intentionally stricter
  than local activation. Treating its clean/tracked Git findings as a local
  installation veto was an unnecessary coupling; exact built-artifact hashes,
  the protected-tree receipt, reversible backups, and real compatibility are
  the local activation boundary.
- The first installed-profile smoke exposed two real boundary bugs. CA's
  mandatory macOS sandbox needed a literal read-only grant for the configured
  installed stdio executable, and codex-team `status` needed a mutation-free
  repository-only receipt before `.orchestrator` initialization. Both now have
  focused regressions.

## Recovery and Rollback

- Source/docs remain ordinary uncommitted changes and can be reverted only by
  exact reviewed patches; no destructive Git command is used.
- The manifest-only commit is retained as approved history. Future correction
  uses a newly digest-approved manifest commit, never an automatic rewrite.
- Binary installation keeps timestamped SHA-identified backups and restores
  them if post-install validation fails.
- If profile validation fails, retain or remove only the newly created profile;
  the default profile and credential stores remain unchanged.

## Completion Record

Changed files: protected-baseline CLI/application/verifier modules and focused
tests; CA inline-overlay input routing and tests; root/nested continuity,
testing, integration, index, queue, and this plan. Protected roots were not
changed. The only task commit is manifest-only
`1530d660916ecb8d6f5befa68d1a850e9912c674`.

Commands and results: focused protected and input suites PASS; full workspace
suite PASS (2,072 CA unit, 38 CLI, 17 role-run, 15+3 ca-watch, 585 codex-team
unit, 13 codex-team integration, three protocol); formatting PASS; strict
all-target/all-feature Clippy PASS; optimized all-feature workspace build PASS;
diff/snapshot/protected hygiene PASS; help/version PASS; production isolation
PASS; real 11/15/28 CA client and reverse CA role-run fixtures PASS.

Runtime activation is complete. Installed and release hashes match at
`ca=b966d7fd02a5f97a58f0a42ec99b08d98ec8dbd1a4d3054bdf7786810b1a98f9`
and
`codex-team=770a000fb296be68732278cc06fa1e3157491de7bf24e5396bd6bf07f6a502d9`.
The original recoverable backups are
`/Users/aji/.local/bin/ca.backup-int191-20260826-da46aaeb` and
`/Users/aji/.local/bin/codex-team.backup-int191-20260826-62578fd4`;
pre-fix candidate backups are retained beside them. The private profile is
`/Users/aji/.config/ca/profiles/codex-team-delegate.toml` with mode `0600`.
Installed help/version, signing validation, named-profile doctor, exact real
MCP compatibility, `/delegate`, `/jobs`, lazy startup, and clean shutdown pass.
The enabled facade automatically prefers codex-team for eligible mechanical
work without explicit user wording, before ordinary exploration or agent
spawning, and falls back only when its receipt allows it. No provider request
or service activation occurred. The previous installed CA remains recoverable
at `/Users/aji/.local/bin/ca.backup-int191-pre-autodelegate-f15a4b8f`.
The first natural-language call exposed a model-invented facade-version
failure; the installed fix hides that field from the model, accepts action-only
calls, and retains exact legacy compatibility. Its immediate predecessor is
recoverable at
`/Users/aji/.local/bin/ca.backup-int191-pre-host-schema-369e2912`.
The default profile hash remained
`75d077d966d0563fe476c63a1fd311b23c3a79aa5ff9fd4405843e9c85a7ea77`.

Harness updates: `CONTINUITY.md`, `TESTING.md`, nested codex-team continuity
and testing, `CA_INTEGRATION.md`, `docs/.INDEX.md`, and `NEXT_STEPS.md` record
the trusted baseline, arrow correction, exact validation, and completed
activation.
