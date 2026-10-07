# CON-153 — One-Project `ca` Monorepo Consolidation

Status: COMPLETE
Owner/session: Codex / current workspace
Last updated: 2026-08-22

## Goal

Make `/Users/aji/project/ca` the single canonical project directory while
preserving the current `ca` working tree, the completed `ca` integration
history, and the latest `codex-team` orchestrator as a second Rust crate in one
monorepo.

## User Authorization

The owner explicitly approved preserving `codex-team` inside the consolidated
`/Users/aji/project/ca` repository and retiring the four redundant folders and
their generated build output on 2026-08-22.

## Evidence

- `/Users/aji/project/ca` has working files through 2026-08-22 14:18 EDT and
  615 pending Git entries. Its committed HEAD is the shared `ca` base
  `855ca635a4d8`.
- `ca-tui-polish` is a clean linked worktree at `28ffaf776c9f` from 2026-08-22
  09:20 EDT. Its six commits descend linearly from the same `ca` base.
- `ca-role-run-integration` is the common checkout for that worktree and has 21
  pending entries. Those bytes must be proven present in the committed
  integration history or preserved separately before retirement.
- `codex_team_orchestrator-ca-integration` is a clean linked worktree at
  `dd7007c4eee8` from 2026-08-22 09:26 EDT, three commits ahead of its main
  branch. `codex_team_orchestrator` has one untracked historical plan.
- The five directories consume about 370 GiB. About 369 GiB is generated Rust
  `target/` output: 280, 31, 36, 18, and 3.9 GiB respectively.

## Design

- Keep the current root `ca` package in place; do not relocate its heavily
  modified working tree.
- Add a root Cargo workspace with members `.` and `crates/codex-team`.
- Import all committed side-repository tips as local archive branches in the
  canonical `.git` before any checkout is retired.
- Copy the latest orchestrator integration tree, excluding `.git`, `target`,
  and its standalone lockfile, into `crates/codex-team` and preserve its lone
  untracked historical plan there.
- Reconcile the latest `ca role-run`/orchestration boundary into current `ca`
  rather than replacing current files with the older integration snapshot.
- Preserve the existing authority split: `codex-team` is scheduler/governor;
  `ca` is the restricted role engine and operator cockpit.
- Permanently remove only generated `target/` directories after exact path and
  manifest checks. Move redundant source repositories to a timestamped folder
  in the user's Trash after histories, pending bytes, and validation are
  accounted for.

## Scope

Allowed: root workspace metadata/lockfile, current `ca` role-integration files
and tests, `crates/codex-team/**`, source-of-truth documentation, local Git
archive refs, the five exact generated target directories, and the four exact
redundant project directories named by the owner.

Forbidden: credentials, runtime profiles/sessions, installed binaries, live
provider traffic, `oracles/`, `reference/`, unrelated `/Users/aji/project`
directories, destructive Git history rewriting, or staging/committing current
work.

## Milestones

### M1 — Preserve and prove

- Import exact committed tips as archive branches into `/ca/.git`.
- Prove whether every dirty `ca-role-run-integration` file is represented by a
  historical blob in the clean integration branch; preserve exceptions.
- Preserve the orchestrator main checkout's untracked plan.

### M2 — Build the monorepo

- Add `crates/codex-team` from the latest clean integration tip.
- Add root workspace membership and reconcile the shared lockfile.
- Update active path and repository-root assumptions for the monorepo layout.

### M3 — Reconcile `ca` integration

- Port the latest scoped role-run protocol and required operator boundary onto
  the newer canonical `ca` source without overwriting subsequent work.
- Retain current prompt-cache, learning, reliability, and TUI behavior.

### M4 — Validate

- Run focused role-run and orchestrator integration suites.
- Run complete workspace tests, formatting, strict Clippy, and release builds.
- Run built help/version/role-run/MCP smokes and diff/snapshot hygiene.
- Do not claim live-provider or production-isolation validation.

### M5 — Retire redundant folders

- Remove the five exact generated `target/` trees only after validation.
- Remove the two clean linked worktrees through Git.
- Move the two former common checkouts and any remaining clean worktree shells
  into a timestamped Trash recovery folder.
- Verify `/Users/aji/project` contains only `/Users/aji/project/ca` for this
  product family and record recovered space.

## Stop Conditions

- Stop if a dirty side-checkout byte is not preserved by an imported commit or
  an explicit recovery artifact.
- Stop if current `ca` behavior would need wholesale replacement by the older
  integration snapshot.
- Stop if the two crates cannot coexist without weakening the documented role
  authority boundary.
- Stop before deleting any path not explicitly named in this plan.

## Validation

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed. `ca` completed 1,797 unit tests, 38 CLI
  tests, and 17 role-run CLI tests; `codex-team` completed 540 library tests
  and 10 integration tests. Three documented manual/pilot tests were ignored.
- `cargo build --workspace --release`: passed.
- Release help/version, role-run and learning-bridge command discovery,
  `git diff --check`, and snapshot-residue checks: passed.
- No live provider, credential, runtime-state, installation, oracle, or
  reference-tree check ran.

## Progress

- [x] Repository/worktree ancestry, dates, dirtiness, and disk usage audited.
- [x] Side histories and dirty bytes preserved.
- [x] Monorepo workspace assembled.
- [x] `ca` role-run boundary reconciled.
- [x] Complete validation passed.
- [x] Redundant folders retired and final inventory verified.

## Completion Record

The root workspace now contains `ca` and `crates/codex-team` with a shared
lockfile. Four archive branches preserve the former committed tips, the
pre-merge canonical tree has its own archive ref, and three verified
source-only tarballs preserve unique pending bytes.

All six exact generated build trees, including the isolated merge worktree,
were cleaned after validation. The two clean linked worktrees were removed
through Git. The two common source checkouts were moved intact to
`/Users/aji/.Trash/ca-consolidation-20260822-145511` for recoverability. The
four redundant paths are absent from `/Users/aji/project`; unrelated
`/Users/aji/project/firefox-yandex` was not touched. The canonical repository
occupies about 1.6 GiB after cleanup, and the Trash recovery folder about
406 MiB.
