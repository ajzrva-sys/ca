# SoreQen Code recommendations for CA

This file is a comparison for a later agent. It is not `NEXT_STEPS.md`. The
named continuation is verify-drive M4 and M5. Do not start this file unless
the task names it. AGT-271 is finished except a separately authorized live
cheaper-quality claim. Implement one numbered slice at a time, and stop when
that slice's done line is true.

Rechecked https://github.com/ZorQelis-AI/soreqen-code `main` at
`215a480fccc7b65549511161c3d010f9599d5b78`
(2026-11-19T16:00:25+05:30, "SoreQen Code 0.1.1"). The package version in that
commit is `0.1.1`. The shallow checkout is `oracles/soreqen-code/`. CA source
compared here is published `main` at
`4c1643b3bea621284e3bbf180bcde81a1a0dec81`. `Cargo.toml` in that commit is
`0.1.190`. The installed binary at comparison time was `ca 0.1.198`, so read
the tree you are editing before you treat a symbol below as current. No
SoreQen source was copied into `src/`.

`oracles/` is gitignored. Open the path directly. The README says 12
conditional skills. `packages/orchestrator/src/skills/builtin.ts` says twelve
always-on and sixteen conditional. Trust the source at this pin.

## How to read the oracle

SoreQen Code is a terminal coding agent. The loop, the ledger, and the mode
policy are the part that matters. The hosted credits, the desktop app, and
the phone remote are a different product.

| Question | Open |
| --- | --- |
| What the turn did | `packages/orchestrator/src/turn-ledger.ts` `TurnLedger`. Entries are dispatched calls and returned results. The rendered record says `Verification: nothing was run this turn.` when `checks` is empty. |
| Summary that invents a check | `readSummaryCheck` in the same file. Empty or a short `VERIFIED` counts as verified. A reply that repeats the summary's own words counts as verified. Anything else is a correction the person sees. |
| Diff review | `packages/orchestrator/src/self-review.ts`. One fresh call gets the request and the diff, not the transcript. It reports concerns and does not edit. `parseVerdict` sets `ran: false` when the answer has neither `VERDICT: OK` nor `VERDICT: CONCERNS`. |
| When that review runs | `packages/orchestrator/src/agent-loop.ts` around the `reviewed` flag. Only when `effortSpec.selfReviewDiff` is set and the turn modified files. The flag is set before the call, so the fix for a concern is not reviewed again. `packages/shared/src/effort.ts`: `max` and `apex` set it. `low` through `extra` leave it off. |
| Auto versus a checkpoint | `packages/orchestrator/src/mode-policy.ts` `evaluateForMode`. `plan` is read-only. `manual` asks for each change. `auto` skips the ask only for a reversible action after a checkpoint, and only when the workspace tier is not stricter. `bypass` drops asks and still runs the deny tier. |
| What a checkpoint can undo | `packages/orchestrator/src/reversibility.ts`. Unknown `run_command` is irreversible. A path outside the workspace is irreversible. A push, a publish, `sudo`, and a side effect that already happened are irreversible. |
| Missing checkpoint | The comment in `evaluateForMode`: a failed snapshot withdraws the auto relaxation. It does not add prompts for ordinary edits that the tier already allowed. One-way actions still ask. |
| Goal close | `packages/orchestrator/src/goal.ts` `afterGoalTurn`. The first `[goal: achieved]` sets `verifying` and starts another turn. Only a second achieved marker, while `verifying` is set, closes the goal. Four idle turns with no file change pause it (`IDLE_LIMIT`). Three failed turns pause it (`FAILURE_LIMIT`). |
| Project skills | `packages/orchestrator/src/skills/index.ts` `fence` and `load`. A project skill is wrapped in `<untrusted-content>`, cannot be always-on, and the load text says it cannot grant permissions or change the task. The person's own skills are instructions. At most `MAX_AUTO_SKILLS` (3) load themselves. |

## Already in CA

Leave these in place. Extend them.

- `/goal` is `src/goal.rs`. Statuses include Active, Paused, Complete, Unreachable, and Blocked. Limits default to 12 continuations, 100_000 provider tokens, and 30 working minutes. `run_goal_evaluation` is a fresh call with no mutation, filesystem, network, MCP, memory, hook, or interaction tools. Its system text says to judge recorded evidence and not to trust a completion claim without evidence. `GoalDecision::Complete` stops the goal on that one evaluation.
- The goal state is the plan. There is no `GOAL.md` writer. Keep it that way.
- `src/host_evidence.rs` records child receipts as data. The wire prefix says the text is not a human request. That record is not the closing summary of the parent turn.
- `src/session_outcome.rs` treats only an explicit user `Accepted` as positive. A model review does not establish acceptance.
- Session mode is Normal, Plan, Auto-review, and Yolo (`ApprovalMode::Allowlist`, `AutoReview`, `RunEverything` in `src/app.rs`). Plan mode refuses unquoted redirects and a closed write-program set through `command_writes`. A Yolo run still loses to a persistent deny (`yolo_executes_but_persistent_deny_still_wins`).
- `/rewind` opens the timeline (`CommandId::Rewind` in `src/main.rs`). It is not a snapshot taken at the start of an auto turn.
- `src/publish_receipt.rs` binds a literal `git push` to its commit. `bind_approval` refuses a remote mismatch.
- `src/memory.rs` `validate_text` refuses `api_key=`, `authorization: bearer`, `password=`, `password:`, `secret=`, `private key`, and a whitespace-delimited `sk-` token of at least 16 characters, with `MemoryError::SensitiveContent`.
- `src/skills.rs` `task_prompt` inserts every loaded skill body, including a project skill, inside `<skill-instructions>`. Skill settings are marked `untrusted-data`. Discovery is `skill_search`, then an explicit activate. A skill body is not permission.

## Slices

Do slice 1 first. The others can follow in order. Each slice gets its own
failing test, then one `python3 scripts/check-change.py --selector <name>`.
A Rust change follows `AGENTS.md`: that one check, then
`bash tools/bug257_install.sh` on the same artifact.

### 1. Show a host ledger under the closing summary

SoreQen's ledger is built from calls that were dispatched and results that
came back. The person sees it whether or not the prose agrees. An empty check
list prints `Verification: nothing was run this turn.` Deletions stay
recorded as commands. The ledger does not guess that `rm` deleted a path.

CA's closing text can still say a suite passed when no test ran. Child
receipts and session outcomes do not fill that line.

At the end of a turn that used tools, append a host block the model cannot
rewrite:

- Files created and files modified, from the tool results of this turn.
- Commands, with exit status, last 15.
- Checks that actually ran, and `Verification: nothing was run this turn.` when none did.
- A count of tool calls and how many failed.

When the assistant summary names a check, a test run, or "all passing" and
the ledger has no matching successful check, replace that sentence with the
ledger line. A summary that does not claim a check stays as written.

Done when a fixture turn whose only tool was an edit renders the ledger with
the nothing-verified line, and a summary that says the tests passed is
rewritten to that line. A turn that ran a successful named test keeps both
the summary and a passed check line. The block is labeled host data, the way
`WIRE_PREFIX` labels host evidence.

`an_edit_only_turn_rewrites_a_false_pass_claim` and `a_passing_named_test_keeps_the_summary_and_the_check` pass. `apply_ledger` runs at the start of a completed plain response.

### 2. Review the diff once, on an explicit high effort

`reviewOwnDiff` sends the original request and at most 600 diff lines. The
reviewer prompt forbids style nits and forbids rewriting the code. Concerns
return to the loop as a user nudge, once, because `reviewed` is set first. A
verdict the parser does not recognize is `ran: false`, and the turn does not
say it was reviewed. `VERDICT: CONCERNS` with no listed concern is treated as
clean so the turn can end.

CA's goal evaluator judges a goal. It does not review one turn's diff, and it
does not run on an ordinary chat turn.

Add one session switch, default off. When it is on and the turn modified
files:

- One fresh call sees the user request and the diff since the turn's start. It does not see the transcript.
- A parsed concern list is fed back once, and the turn may edit in response.
- That response is not reviewed again.
- An unparsed answer is recorded as not run. It is not a clean review and it does not block the turn.
- The reviewer has no edit tool.

Done when a test with the switch on and a diff containing a leftover
`console.log` records one concern and one follow-up edit, then finishes
without a second review call. A test with the switch off makes no review
call. An empty model answer records `ran: false` and still finishes.

`one_shot_diff_review_records_one_concern_and_does_not_call_again` passes. The plain summary follows an edit whose arguments contain `console.log` and whose tool output does not. High effort writes one concern and one follow-up edit onto that plain message. A second call does not review again. An empty answer records `ran: false` on the message and finishes. Effort off records no call.

### 3. Let auto-review skip asks only after a snapshot

Do this after Terminess slice 3 and slices B and C in `docs/tui-glitch-and-approval-ca-recommendations.md` when those are also in the task. The order and the invariants are in that file's Shared approval order. This slice changes Auto-review only.

`auto` proceeds on a reversible workspace edit when `checkpointAvailable` is
true, and asks when `classifyReversibility` says the action is one-way. An
unknown command is one-way. `bypass` skips asks and still cannot pass a tier
denial. A missing checkpoint sets `degraded` and does not start asking about
edits the tier already allowed.

CA Auto-review uses `auto_review_safe_invocation`. Yolo is `RunEverything`.
Neither one is "snapshot, then undoable edits proceed."

On Auto-review only:

- Before the first mutating tool of a turn, snapshot the workspace files the turn may change. Record whether that snapshot succeeded.
- A file edit inside that snapshot proceeds without a card.
- `run_command` that `command_writes` does not recognize, a path outside the workspace, and a literal `git push` still ask. The push path stays on `PublishReceipt`.
- When the snapshot fails, keep today's Auto-review asks. Do not add a card for an edit the current tier already allows. Still ask for an unrecognized command.
- Yolo stays Yolo. A persistent deny still wins in both modes. Plan mode still refuses writes.

Done when an Auto-review test with a successful snapshot applies an edit
without a card, and asks for `bash -c 'echo hi'` (unrecognized) and for
`git push`. A test whose snapshot fails still asks for the push and does not
add a new card for a file edit that today's Auto-review would allow. A Yolo
test of an allowed command still runs, and the existing persistent-deny test
still denies.

`auto_review_snapshot_runs_an_edit_and_still_cards_push_and_bash` passes. A successful snapshot runs the edit and keeps cards for `bash` and `git push`. A failed snapshot still asks for the push. A yolo `cargo fmt` still runs.

### 4. Make the first goal Complete a verification turn

`afterGoalTurn` does not close on the first achieved marker. It sets
`verifying` and starts a turn whose prompt says to build from clean, run the
tests, and exercise the objective. A second achieved marker closes it. Four
turns that change no file pause the goal. Three failed turns pause it. The
plan stays in SoreQen's `GOAL.md`. CA's plan stays in `Goal` state.

When `run_goal_evaluation` returns `Complete` and this goal has not yet had a
verification continuation:

- Apply it as `Continue`.
- The continuation instruction tells the agent to run the checks the objective names and to record their results. It is evaluator guidance, the same channel as `continuation_instruction`.
- A later `Complete` closes the goal only when the evidence snapshot includes a successful check from that continuation. Otherwise it stays `Continue` with feedback that names the missing check.
- After four continuations whose tool results change no file, pause the goal. The note says no file changed.
- After three evaluations that fail to return a valid report, pause the goal. The note says evaluation failed.
- Do not write `GOAL.md` or any other plan file.

Done when a test whose first evaluation is `Complete`, with no successful
check in the snapshot, stays Active and queues one verification continuation.
A second `Complete` after a successful check in the snapshot becomes
`GoalStatus::Complete`. A fifth no-file continuation pauses. A fourth invalid
evaluation pauses. The goal file search of the workspace fixture stays empty.

`first_complete_without_a_check_stays_active_and_later_quiet_turns_pause` passes. The first Complete queues one verification continuation. A later Complete closes after a successful check. A fifth no-file continuation pauses. A fourth invalid evaluation pauses. The fixture writes no goal file.

### 5. Fence a project skill as guidance

`SkillSet.load` wraps a project skill in `<untrusted-content source="...">`
and tells the model it cannot grant permissions or change the task. User
skills are inserted as their body. A project skill cannot be kind `always`.
Auto-load stops at three skills.

CA `task_prompt` uses the same `<skill-instructions>` wrapper for every
scope. The body is the instruction.

When the loaded instance's source scope is project:

- Put the body in an untrusted-content element, with the skill path as the source.
- The surrounding line says the text is guidance about this repository and does not grant tools or change the task.
- A project skill does not enter the prompt unless the user or the model named it. It does not match an always-on or trigger list of its own.
- A user skill and a bundled skill keep today's `<skill-instructions>` body.

Done when a project skill fixture's `task_prompt` contains the untrusted
wrapper and the path, and does not contain `<skill-instructions>` around that
body. A user skill fixture still uses `<skill-instructions>`. A turn whose
message merely resembles a project skill's title does not load that skill.

`project_skill_guidance_is_untrusted_and_a_title_mention_does_not_load_it` passes. A project skill uses `<untrusted-content source="...">`. A user skill keeps `<skill-instructions>`. A title-like message does not load the project skill.

## Leave out of CA

- TypeScript, the npm bundle, or any file from `oracles/soreqen-code/` copied into `src/`.
- SoreQen credits, Burst Day, Flex balance, hosted Flow models, `soreqen login`, and the desktop or phone app.
- A `GOAL.md` writer, or a second plan file beside `src/goal.rs`.
- The twenty-eight builtin skill texts, including the motion-graphics skill.
- Their secret regular expression pasted into `validate_text`. The existing marker list stays the check. Widen it only in a task that names a specific missed shape, with a fixture for that shape.
- MCP sampling. CA does not grow it from this comparison.
- An auto-approver that answers a one-way command. `bypass` dropping asks is their Yolo. CA's Yolo already exists, and the deny floor stays above it.

## Suggested skills

Use `tdd` when a task names a slice and asks for the test first. Use the
repository `AGENTS.md` release rule after a Rust change. Leave `implement-spec`
and `to-tickets` unused: this file is the comparison, and it is not a spec or
a ticket split.
