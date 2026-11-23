# Parallel worktree subagents for CA

This file is a comparison for a later agent. It is not `NEXT_STEPS.md`. The
named continuation is verify-drive M4 and M5. Do not start this file unless
the task names it. AGT-271 is finished except a separately authorized live
cheaper-quality claim. The behavior below is not in the tree. Implement one
numbered slice at a time, and stop when that slice's done line is true.

Checked published `main` at `4c1643b3bea621284e3bbf180bcde81a1a0dec81`.
`Cargo.toml` in that commit is `0.1.190`. The installed binary at comparison
time was `ca 0.1.198`, so read the tree you are editing before you treat a
symbol below as current.

The requested behavior is: split independent parts of one job into subagents,
give each its own git worktree, let them edit and test at the same time, then
merge the worktrees back with a three-way merge that writes conflict markers
where needed. `/undo` covers that merge.

## What already exists

These pieces are real. They are not that behavior.

- `src/agent/subagent.rs` defaults to 3 concurrent children and 8 children per parent turn. `available_graph_slots` is that cap minus children already running.
- `src/agent_graph.rs` `ensure_workspace_lanes` refuses two writers on the same lane unless one depends on the other (`UnserializedWorkspaceLane`). Two writers on different `worktree_id`s may both be ready. The graph tick in `src/main.rs` starts as many ready tasks as `available_graph_slots` allows. It attaches a worktree that already has that id. It does not create one.
- `AgentMergePolicy::ThreeWayText` adds one sentence to the merge lane's prompt: "three-way text merge against the common base commit." No host function runs `git merge-file` or writes `<<<<<<<` markers. `RejectConflict` and `LastWriteByTaskOrder` are prompt sentences too.
- `/worktree` creates, inspects, diffs, attaches, and cleans up a worktree. `WorktreeAction` has no merge. Create is the slash command and the launch flag, plus multi-run.
- `/multi-run` (`src/multi_run.rs`) runs the same request in 2 to 5 worktrees, collects diffs, and asks for a winner. Fusion stores a plan and a source worktree id. It does not split the request into different parts, and it does not copy those parts into the parent tree.
- A child `write_file` emits `RunnerEvent::Mutation`. The agent manager keeps that receipt for the parent turn. `automatic_review` snapshots it. `/undo` restores batches recorded by `retain_file_mutation` on the session stack. Child worktree writes are not that stack. `/undo` refuses while work in the workspace is still running.

## Slices

Do slice 1 first. Slice 3 is the user-visible close. Each slice gets its own
failing test, then one `python3 scripts/check-change.py --selector <name>`.
A Rust change follows `AGENTS.md`: that one check, then
`bash tools/bug257_install.sh` on the same artifact.

Leave `/multi-run` as best-of-N of one request.

## Merge contract

This file owns the host merge. Terminess Apply is the one-worktree call of
that function. Terminess does not define another conflict rule.

- N independent parts, disjoint file lists, the same base commit, started together. The cap is `available_graph_slots`.
- Overlap writes conflict markers naming the parent and the part. Neither side is dropped. The merge does not commit.
- `/undo` restores that one batch. Worktree cleanup stays `/worktree cleanup`.

### 1. One worktree per independent part, started together

A parallel run names 2 to `available_graph_slots` parts. Each part has its
own task text. The host creates one worktree per part from the same base
commit, the parent `HEAD` at start, and records that commit on the run. The
children start without waiting for one another. Each child's workspace is its
worktree. Each may `run_command` there, including tests. The parent session
stays usable.

A part that shares a file list with another part is rejected before any
worktree is created. The file lists are the host's split, not a guess after
the children edit.

Done when a two-part fixture creates two worktrees at the same base commit,
both children are non-terminal at the same time, and a fixture whose file
lists overlap creates no worktree.

`two_parts_share_head_and_overlap_creates_no_worktree` passes. Both children are running at the parent HEAD. An overlapping list creates no worktree.

### 2. The host merges three ways and writes markers

When every part has finished, the host merges each worktree into the parent
working tree against the recorded base. For each path:

- Only the worktree changed it since the base, and the parent bytes still match the base: take the worktree bytes.
- Only the parent changed it: leave the parent bytes.
- Neither changed it: leave it.
- Both changed it and the changed regions do not overlap: keep both edits.
- Both changed the same region, or two worktrees changed the same region: write conflict markers in the parent file. The markers name the parent and the part. The base text sits between them.

The merge does not commit. A clean path is a normal file edit. A conflicted
path is still a file edit, and the run status is conflicted until the markers
are gone. No part is dropped to make the tree clean.

`AgentMergePolicy::ThreeWayText` on a merge lane calls this function for the
lane's declared worktrees. The prompt sentence is no longer the merge.

Done when a fixture with disjoint edits lands both files and commits nothing,
a fixture that edits the same line in the parent and in one worktree leaves
markers naming both sides and does not lose either side, and a second
worktree that edits that same line is included in the markers. A path only
the parent changed stays the parent's bytes.

`merge_lands_disjoint_edits_and_keeps_both_sides_of_a_conflict` passes. Markers name `parent`, `part-1`, and `part-2` on the shared line and keep each side. A path only the parent changed stays the parent's bytes. The merge commits nothing.

### 3. `/undo` restores the pre-merge tree

The merge is one `FileMutationBatchReceipt` recorded through
`retain_file_mutation`. `/undo` restores every parent path in that batch,
including deleting a file the merge created and removing conflict markers.
`/redo` applies the same batch again. Undo does not delete the worktrees.
Cleanup of those worktrees stays `/worktree cleanup`.

`/undo` still refuses while a part is running.

Done when a test merges two parts, `/undo` returns the parent bytes to the
pre-merge snapshot and clears the markers, `/redo` restores the merged bytes
and the markers, and the worktree directories still exist after the undo.

That same merge test calls `restore_before` and `restore_after`. The parent bytes return, the markers come back on redo, and the worktrees remain. `discard_part` refuses a running child. `/undo` and `/redo` call those restores when `ReviewTracker.merge_batch` is set.

`parallel_spawn_returns_while_the_child_is_running` passes. Both children keep the user task, the parent bytes stay unchanged while they run, and `finish` merges only after each worktree path is idle. `/undo` then restores the parent.

## Leave in place

- `/multi-run` winner selection and its fusion plan.
- The lane rule that two writers cannot share one worktree without a dependency.
- Session-stack undo of a parent `write_file`. Slice 3 adds the merge batch to that stack. It does not replace hunk undo.
- A model-written merge. The host writes the markers. The model can edit a conflicted file afterward, and that later edit is its own undo batch.
