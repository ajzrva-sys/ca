# WT-071 — Worktree Lifecycle

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-11

## Goal

Add an explicit profile-scoped `/worktree` lifecycle for isolated Git branches beneath the `ca` data root. A user can create, inspect, diff, attach to, and confirm cleanup of one owned worktree without `ca` ever merging commits, deleting branches, force-removing a dirty tree, or cleaning resources automatically at shutdown. Once attached, ordinary workflows and newly spawned subagents inherit the isolated workspace through their existing authority boundaries.

## Non-Goals

- Automatic worktree creation for every workflow/subagent, worktree pools, leases, merges, cherry-picks, rebases, pushes, commits, stashes, branch deletion, conflict resolution, remote fetches, or destructive recovery.
- Concurrent workflow steps or dependency-aware agent scheduling; those remain AGT-072 work.
- Treating a worktree as an authority bypass. Tool selection, approvals, canonical path checks, sandboxing, network grants, hooks, and model limits remain unchanged.
- Cursor parity. MiMo/Kun/ECC documentation informed isolation and preservation rules, but no clean Cursor worktree fixture or oracle runtime observation establishes this behavior.

## Evidence and Current Behavior

- `WorkspaceContext` owns canonical session roots/cwd and every main tool invocation is prepared against a clone of `state.workspace` (`src/workspace.rs`, `src/main.rs`). Workflows already reuse that exact main tool loop, so an explicit workspace attachment naturally isolates later workflow file operations.
- `ProviderAgentRunner` currently captures one startup workspace (`src/agent/subagent.rs`), which would make subagents stale after an in-process attach. Spawn requests need an immutable workspace snapshot so existing children keep their original root and later children inherit the attached root.
- `RuntimePaths::profile_data_dir` already isolates default/named profile data; it has no worktree path policy (`src/runtime.rs`). Bare shares default data but disables extension mutation.
- Existing Git helpers use argument arrays and `--` boundaries, but there is no owned worktree registry, no `/worktree`, and no safe cleanup boundary (`src/changes.rs`).
- MiMo documents isolated child trees under an app data root, while also warning that deleting a worktree/branch can destroy unmerged work (`oracles/MiMo-Code-main/docs/harness/MiMo Orchestrator Mode.en.md:65-72`). Kun documents preserving unaccepted/conflicted/orphaned worktrees and never automatically deleting them (`oracles/Kun-master/docs/graph-mode.en.md:263-267,442-443`). These are design evidence only.

## Scope

Allowed files: a new worktree module; runtime path/capability policy; defaulted Session V4 attachment metadata; app/command/input/render/orchestration state; immutable subagent workspace snapshots; focused real-Git temporary-root tests and snapshots; required harness documents.

Forbidden: oracle/reference edits, real repositories/config/Keychain/sessions, branch deletion, merge/rebase/cherry-pick/stash/commit/push/fetch, `git worktree prune`, force removal, hidden cleanup at shutdown, remote I/O, installed-binary mutation, or worktree authority wider than its originating repository.

Runtime state affected: only explicitly created profile-owned worktree directories/branches and a bounded profile manifest under the injected data root. Attach changes the current process/session primary workspace after an explicit command. Cleanup permanently removes only one clean, unattached owned worktree directory after Cancel-first confirmation and leaves its branch intact.

## Design and Invariants

### Ownership and persistence

- `RuntimePaths::worktrees_dir(profile)` is `<profile-data>/worktrees`; bare reads the default-profile registry but cannot create/attach/cleanup. One bounded atomic `manifest.json` stores versioned records: UUID, validated label, canonical source repository, exact owned path, branch, base commit, and creation time. No prompt, diff, tool output, credential, endpoint, or session content enters it.
- Each tree path is derived, never supplied: `<worktrees>/<repo-digest>/<uuid>/<label>`. The canonical source repository identity and exact record/path containment are revalidated before every Git mutation. Symlinked manifests/parents/targets and paths outside the injected root fail closed.
- Labels are lowercase ASCII slugs (1–48; letters, digits, `-`, `_`). Branches are collision-resistant `ca/<label>-<uuid-prefix>`. Base is a validated local ref/commit and resolves before mutation. Creation uses argument-array `git worktree add -b`; failure never deletes a branch or another path. A post-create manifest failure preserves the new worktree and reports its exact recovery path.

### Status, diff, attach, cleanup

- `/worktree [list|create <label> [base]|status <id>|diff <id>|attach <id>|cleanup <id>]` is visible. List/status/diff are read-only; create/attach/cleanup are idle-only and use bounded background orchestration so Git/filesystem work never blocks the TUI. A mutation is locked once started rather than presenting a misleading mid-`git worktree add/remove` cancellation promise.
- Status verifies the worktree's Git top-level, branch, HEAD, bounded porcelain status, base divergence, missing/orphaned state, and whether it is attached. Diff combines bounded base-to-HEAD, staged, and unstaged tracked patches plus untracked names only. Every external string is terminal-sanitized and output is UTF-8-safe with honest omission metadata.
- Attach requires an owned present worktree and no active provider/tool/approval/goal/workflow, foreground/background job, or child agent. It swaps the primary `WorkspaceContext`/cwd, reloads project skills and workspace memory, clears derived request/context state, updates dynamic managers, records a defaulted Session V4 attachment reference, and preserves canonical conversation/history. Existing child agents retain immutable prior workspace snapshots; later spawns receive the attached snapshot.
- Workflows need no new definition authority: after explicit attach, `/workflow run` uses the attached workspace exactly as ordinary main requests do. No workflow file may attach/create/cleanup a tree or name an external path.
- Cleanup is available only for a non-attached owned record. A fresh status check must prove zero staged, unstaged, or untracked changes. The confirmation is Cancel-first and shows branch preservation. Confirmation rechecks identity and cleanliness, runs plain `git worktree remove` without `--force`, removes only the manifest record, and never deletes the branch. Missing registered directories can be forgotten only through the same confirmation.
- `ca` never automatically removes a worktree on workflow/subagent completion, cancellation, startup, shutdown, retention, or failure.

### Compatibility and safety

- Session V4 gains a defaulted optional attachment `{id}`. V1–V3 and existing V4 files load unchanged. If an attachment is absent/missing/mismatched at restore, the launch workspace remains active and a visible warning is emitted; no Git repair runs.
- Bare mode can list/status/diff default-profile records but refuses create/attach/cleanup. Named profiles cannot see or mutate another profile's manifest even when repository/labels match.
- Worktree Git commands have no remote options or inherited credential prompts, use `GIT_TERMINAL_PROMPT=0`, bounded output/time, and argument arrays. No command accepts free-form shell text.

## Milestones

### M1 — Core registry and real-Git characterization

- Add failing tests for path/profile isolation, label/ref validation, atomic manifests, create/list/status/diff, collisions, missing/orphaned records, symlink/traversal rejection, clean cleanup, dirty/untracked cleanup refusal, branch preservation, and no automatic cleanup.
- Add runtime path/capability and Session V4 compatibility coverage.

### M2 — Attachment and inherited authority

- Add immutable workspace snapshots to subagent spawn requests and regressions proving pre-attach children retain their root while later children inherit the new one.
- Add attachment orchestration, workspace memory/skill refresh, dynamic manager updates, session persistence/restore warnings, and workflow inheritance tests.

### M3 — Commands and visible surfaces

- Add command discovery/parsing, background action generations, busy locking, status/diff pagers, Cancel-first cleanup, bare guidance, and wide/narrow list/status/diff/confirmation/busy/error snapshots.

### M4 — Gates and isolated walkthrough

- Run focused/full/format/snapshot/strict-Clippy/release/smoke gates.
- Exercise the released TUI in a disposable Git repository/home: create from HEAD, inspect clean status/diff, attach, run one loopback workflow plus one subagent rooted in the tree, make an untracked change and prove cleanup refusal, clean it externally, confirm cleanup, and prove the branch remains.

## Acceptance Criteria

- Creation produces exactly one profile-owned path and one collision-resistant branch from a verified local base. Invalid repo/ref/label/path/symlink/collision fails without a registered tree or provider traffic.
- List/status/diff never mutate Git. Diff is bounded and names untracked files without reading their content.
- Attach changes future main workflow and subagent filesystem authority to the exact owned worktree; existing running children are unaffected. Restore is explicit/safe when metadata is missing.
- Cleanup defaults to Cancel, rechecks exact ownership and status, refuses every dirty state, uses no force, removes exactly one owned directory/record, and leaves its branch reachable.
- No normal completion/cancellation/shutdown path removes worktrees or branches. Default/named profiles are isolated; bare cannot mutate.
- Existing chat, workflow, goal, session, workspace-root, approval, sandbox, MCP/LSP/skills/memory, headless/scripted, and Git change-review behavior is unchanged when no worktree is attached.

## Validation

Focused:

- `cargo test worktree --no-fail-fast`: PASS, 19 focused module/input/orchestration/render tests when first run; the final suite includes the immutable child-workspace and attached-workflow regressions below.
- `cargo test agent_spawns_capture_immutable_workspace_snapshots --no-fail-fast`: PASS.
- `cargo test attached_worktree_becomes_the_exact_workflow_definition_root --no-fail-fast`: PASS.
- Fourteen reviewed snapshots cover list, status, diff, attached title, Cancel-first cleanup, cleanup busy, and cleanup failure at 100x30 and 60x24.

Completion:

- `cargo test --no-fail-fast`: PASS, 855 unit tests plus 28 CLI integration tests.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- Released `--help`, `--version`, named-profile, and bare-profile temporary-home smoke checks: PASS.

Manual/visual:

- Released TUI in a disposable home/repository: PASS for paced `/worktree` command dispatch, profile-owned create/list/status/diff, exact attach persistence, an attached-root loopback workflow reading `WT071_ATTACHED_ROOT`, dirty untracked-file cleanup refusal, default-Cancel preservation, confirmed clean removal, empty manifest, and branch preservation.
- Child workspace inheritance is proved by the hermetic orchestration regression rather than provider prose: spawn requests capture immutable workspace snapshots, so pre-attach children retain their root and later children inherit the attached root.
- The disposable repository, config, sessions, server, and worktree registry were moved to Trash after validation. No user runtime state or installed binary changed.

Blocked checks: none.

## Progress

- [x] Runtime/workspace/Git/workflow/subagent seams and oracle preservation lessons characterized.
- [x] Profile-owned registry, create/status/diff/cleanup core complete.
- [x] Attachment/session/subagent/workflow inheritance complete.
- [x] Command/orchestration/render surfaces complete.
- [x] Repository-wide gates and isolated released-TUI walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: isolate by explicit attachment rather than executable workflow syntax.
  Evidence: workflows already use the canonical main workspace/tool loop; adding path authority to untrusted DAG data would violate WFL-070's definition boundary.
  Consequence: the user creates and attaches an owned tree first, then workflows and future subagents naturally inherit it.
- Decision: preserve branches on every cleanup.
  Evidence: a clean worktree can still contain unmerged commits, and oracle documentation explicitly warns against losing them.
  Consequence: cleanup reclaims only the directory/manifest; branch integration/deletion remains a manual Git decision.
- Decision: durable manifest plus repeated Git identity checks.
  Evidence: filesystem containment alone cannot prove that a directory remains the registered linked worktree.
  Consequence: stale/orphaned records are visible and fail closed instead of being silently repaired or pruned.
- Decision: subagents receive immutable workspace snapshots per spawn.
  Evidence: the runner currently captures startup state, while an attachment can occur later and children may overlap in time.
  Consequence: attachment never moves a running child underneath it, and new children get exact current authority.

## Surprises and Discoveries

- `McpManager` already receives current workspace updates every TUI loop and `LspManager` updates at execution, but the subagent provider runner captures a startup clone. Only the subagent boundary needs a request-local workspace refactor for safe attach inheritance.
- Session V4 persists additional roots but not its primary root. A small defaulted owned-worktree reference is safer than persisting an arbitrary absolute primary path.
- The existing change service provides useful porcelain/diff parsing, but worktree cleanup needs stricter identity/cleanliness checks and must never use its ordinary current-workspace assumptions.
- Real terminal automation must pace slash-command keystrokes. Bracketed or burst paste is intentionally treated as composer input rather than command execution, so the final walkthrough used the same palette path as a person typing `/worktree`.
- A worktree can be clean while its preserved branch contains commits not present on the source branch. The core cleanup regression therefore records a unique commit, removes only the linked directory, and proves the branch still resolves to that commit.

## Recovery and Rollback

Remove the `/worktree` surfaces and ignore the defaulted Session V4 attachment. Existing worktree directories and their branches are intentionally left intact for manual Git recovery; deleting code must never attempt to clean them automatically. The profile manifest is non-authoritative and can be inspected manually without touching repository data.

## Completion Record

Changed files: `src/worktree.rs`; `src/runtime.rs`; `src/session.rs`; `src/app.rs`; `src/main.rs`; `src/agent/subagent.rs`; `src/tui/commands.rs`; `src/tui/input.rs`; `src/tui/render.rs`; `src/tui/render_tests.rs`; fourteen worktree snapshots; and the required harness documents.

Commands and results: all focused, repository-wide, formatting, strict-lint, release, smoke, snapshot, real-Git, and disposable released-TUI checks listed above passed.

Remaining risks: worktree branches and any recovery paths intentionally remain user-owned; `ca` does not integrate or delete them. AGT-072 may consume attachment metadata but must not weaken this ownership boundary.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, and `docs/.INDEX.md` now record the module, invariants, validation, extension status, and AGT-072 handoff.
