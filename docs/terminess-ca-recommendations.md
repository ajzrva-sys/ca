# Terminess recommendations for CA

This file is a comparison for a later agent. It is not `NEXT_STEPS.md`. The
named continuation is verify-drive M4 and M5. Do not start this file unless
the task names it. AGT-271 is finished except a separately authorized live
cheaper-quality claim. Implement one numbered slice at a time, and stop when
that slice's done line is true.

Rechecked https://github.com/Terminess/terminess `main` at
`e70cff04b1f09af1c89e9f7fbd10ebc6030dd3ec`
(2026-11-19T21:23:31Z, "No flash of the system tab bar when a tab opens").
`VERSION` in that commit is `0.1.0`. The shallow checkout is
`oracles/terminess/`. CA source compared here is published `main` at
`4c1643b3bea621284e3bbf180bcde81a1a0dec81`. `Cargo.toml` in that commit is
`0.1.190`. The installed binary at comparison time was `ca 0.1.198`, so read
the tree you are editing before you treat a symbol below as current. No
Terminess source was copied into `src/`.

`oracles/` is gitignored. Open the path directly.

## How to read the oracle

Terminess is a Mac and iPhone terminal that runs the official `claude` and
`codex` binaries and reads the transcripts they already write. CA is the
agent. Read the pair, finding, and approval paths. Leave the app shell.

| Question | Open |
| --- | --- |
| The four ways to use another agent | `Sources/TermApp/PairSkill.swift` `text(for:)`. `ask` and `review` are read-only. `review --fix` and `handoff` write. `parallel` writes in its own worktree and returns a job id. The brief is Goal, Agreed so far, Files, Done when. The other agent has not seen the parent conversation. |
| At most two review rounds | The same skill: do the work, then `pair review`, at most two rounds. |
| One finding applied alone | `Sources/TermApp/PairRuns.swift` `FindingsView.apply`. The brief names one file, the problem, the fix, and "nothing unrelated is changed, and the relevant tests pass." It starts a new handoff. It does not edit the file itself. |
| Finding shape | `ReviewFindings.Finding`: `severity`, `file`, `line`, `title`, `detail`, `fix`. |
| Parallel apply and discard | `PairSkill.swift` parallel section. `pair apply` lands the job as uncommitted changes in the main copy. Exit 3 means conflict markers are left for the parent. `pair discard` drops the job. The worktree is created with uncommitted changes copied in. |
| First approval wins | `Sources/TermHostServer/Server.swift` `decide` and `answerPermission`. The answer must be `allow`, `always`, or `deny`, and `PermissionInfo.id` must match the current permission. A mismatched id is ignored. `answerPermission` clears the current permission before `showNextPermission` shows the next one. |
| Facts versus behavior | `Sources/TermTranscript/OperationsGuide.swift`. Setup facts belong in `OPERATIONS.md`. Behavior rules belong in `ORCHESTRATE.md`. Secrets are locations, never values. An unchecked fact is marked `(unverified YYYY-MM-DD)`. Correct an entry in place. |
| Quiet alerts | The README and the iPhone section: a notification says a session finished, stopped, or needs you. It does not include what the session said. |

## Already in CA

Leave these in place. Extend them.

- `src/review.rs` `ReviewFinding` already has `severity`, `title`, `rationale`, `file`, `line_start`, `line_end`, `confidence`, and `suggested_validation`. A review run is read-only (`REVIEW_READ_TOOLS`). There is no per-finding `fix` string and no action that starts a child for one finding.
- `AgentSpawnRequest.reads_only` in `src/agent/subagent.rs` drops mutating tools. Plan-mode children set it. A spawn is not named ask, review, handoff, or parallel.
- `src/worktree.rs` `WorktreeAction` is List, Create, Status, Diff, Attach, PrepareCleanup, and ConfirmCleanup. Cleanup asks before it removes the directory. There is no action that applies a worktree diff into the parent as uncommitted changes.
- Approval modes are Allowlist, Auto-review, and Yolo. A session grant can allow a later matching command (`src/approval.rs`). A Yolo run still loses to a persistent deny.
- `/goal`, the publish receipt, host evidence, and plan send-back are covered by the SoreQen and LoopForge comparisons. Do not reopen them from this file.
- Automation alerts in `src/automation_notifications.rs` are metadata: automation name, outcome, profile. The comment says they are metadata-only. The alert struct has no transcript field.
- Project instruction files are named once by `src/instruction_proximity.rs` when a tool first touches their directory. The body is not copied into that notice.

## Slices

Do slice 1 first. Each slice gets its own failing test, then one
`python3 scripts/check-change.py --selector <name>`. A Rust change follows
`AGENTS.md`: that one check, then `bash tools/bug257_install.sh` on the same
artifact.

### 1. Apply one review finding as its own child

Terminess does not patch the file from the finding card. `apply` writes a
brief and starts a handoff whose done line is that one fix, nothing else, and
the relevant tests.

When a `ReviewResult` is on screen, each finding grows one action, Apply:

- The child prompt is only that finding: file, line range, title, rationale, and a fix line. Add `fix` to `ReviewFinding` as a required string on new results. An old stored result with no fix shows the finding and hides Apply.
- The child does not receive the parent transcript or the other findings.
- The child is a normal write spawn in the same workspace.
- The child is finished only when the named file differs and every other path in the turn's edit set is unchanged, and `suggested_validation` ran with success. A wider diff is a failed child, not a silent success.
- The parent stays put. Applying a second finding starts a second child after the first has finished.

Done when a two-finding fixture applies the first, the child prompt contains that file and fix and does not contain the second title, an edit outside the named file fails the child, and a successful validation on only the named file completes it. A finding decoded without `fix` still renders and offers no Apply.

`apply_child_uses_the_first_finding_and_fails_an_outside_edit` passes. The spawned child is in the parent workspace. Its prompt contains the first file and fix and omits the second title. An edit outside the named file fails the child. The child completes after that file is edited and the named validation succeeds. `a_finding_without_a_fix_renders_without_apply` passes. A stored finding with no fix renders and has no Apply line.

### 2. Name the child mode, and let a parallel child land or be dropped

`pair ask` and `pair review` cannot write. `pair handoff` writes in the main copy from a brief the parent composes. `pair parallel` returns before the child finishes. The parent later applies the diff as uncommitted changes, or discards the worktree. Apply exit 3 leaves conflict markers and does not commit.

Add a mode on `AgentSpawnRequest`:

- `ask` and `review` set `reads_only`. `review` must end in `ReportReviewFindings`. A review child that writes is a spawn error.
- `handoff` may write. Its prompt is the brief only: goal, decisions already made, files, and done-when. The parent transcript is not copied in.
- `parallel` creates a worktree with `WorktreeAction::Create`, spawns the child there, and returns the worktree id to the parent without waiting. The parent session can keep taking input. Further children use remaining `available_graph_slots`. Ask, review, and handoff are unchanged.
- Two new worktree actions: Apply and Discard. Apply calls the host merge in `docs/parallel-subagents-ca-recommendations.md` for this one worktree. A clean diff lands as uncommitted files and does not commit. An overlap writes conflict markers naming the parent and this part. Both sides stay in the markers. `/undo` of that batch is the same merge batch as that plan. Discard is the existing cleanup confirmation, and it refuses while the child is running.

Done when an ask child calling `write_file` fails closed, a handoff prompt fixture contains the brief and not a parent user sentence that was only in the transcript, a parallel spawn returns while the child is running, Apply of a one-file diff changes that file in the parent and does not commit, Apply of an overlapping parent edit leaves markers naming both sides and does not commit, and Discard of a running child is refused. The merge and `/undo` are the function in `docs/parallel-subagents-ca-recommendations.md`.

`child_modes_keep_ask_from_writing_and_handoff_off_the_transcript` passes. An ask child that calls `write_file` fails closed. A handoff prompt is the brief. A parallel request returns while the child is marked running and uses worktree isolation.

### 3. A late approval cannot decide the next card

This slice is first when the transcript plan or SoreQen slice 3 is also in the task. It does not change which commands ask. The order and the invariants are in `docs/tui-glitch-and-approval-ca-recommendations.md` under Shared approval order.

`decide` applies `allow`, `always`, or `deny` only when the permission id is the one on screen. `answerPermission` clears that permission, then shows the next queued one. A second decide for the cleared id does not allow the next command.

CA can show one approval card. A queued follow-up can become the next card. An answer has to be bound to the card that was on screen.

Give each pending approval a host id:

- Allow once, always (a session grant for the host's command prefix, not a string the model sends), and deny are the three answers.
- The answer carries the id. The host applies it only when the id is the current card.
- Applying it clears the card before the next queued approval is shown.
- A repeated answer with the old id is ignored. The next card stays pending.
- Yolo and a persistent deny still follow their current rules. This slice does not add a second device.

Done when a test approves card A, the harness queues card B, and a second approve carrying A's id leaves B pending. An approve carrying B's id runs B. An always answer stores a session grant for A's prefix and does not grant B's command.

`a_late_approval_for_card_a_does_not_run_card_b` passes. Approving A, then approving again with A's id, leaves B pending. B's id runs B. Always stores A's command prefix and does not grant B.

## Leave out of CA

- Swift, the Mac app, the iPhone app, `terminessd`, the relay, and any file from `oracles/terminess/` copied into `src/`.
- Wrapping the `claude` or `codex` CLIs, reading their transcript formats, or installing a skill into `~/.claude` or `~/.codex`.
- A phone approval surface, Face ID, SSH pairing, and lock-screen actions.
- `OPERATIONS.md` and `ORCHESTRATE.md` as files CA writes. Record a setup fact through the existing memory path, which already refuses secret values. Mark an unchecked fact unverified inside that record when a task asks for it. Do not add a second documentation pair.
- A native tab-bar flash fix, a Chat-versus-terminal split view, and a GitHub issue board.
- A notification body that quotes the session. Automation alerts already omit it. Do not add session text to them.
- A review loop of its own. Two rounds is their cap. Send-back stays the LoopForge comparison's slice.

## Suggested skills

Use `tdd` when a task names a slice and asks for the test first. Use the
repository `AGENTS.md` release rule after a Rust change. Leave `implement-spec`
and `to-tickets` unused: this file is the comparison, and it is not a spec or
a ticket split.
