# LoopForge recommendations for CA

This file is a comparison for a later agent. It is not `NEXT_STEPS.md`. The
named continuation is verify-drive M4 and M5. Do not start this file unless
the task names it. AGT-271 is finished except a separately authorized live
cheaper-quality claim. Implement one numbered slice at a time, and stop when
that slice's done line is true.

Rechecked https://github.com/Tencent/LoopForge `main` at
`eb2e368f14d004e6a22ee25940f5a03c0b5d9f38`
(2026-11-22T22:53:37+08:00, "Fix LoopForge coding-agent handoffs and onboarding
(#1217)"). The shallow checkout is `oracles/loopforge/`. CA source compared
here is published `main` at `4c1643b3bea621284e3bbf180bcde81a1a0dec81`.
`Cargo.toml` in that commit is `0.1.190`. The installed binary at comparison
time was `ca 0.1.198`, so read the tree you are editing before you treat a
line number below as current. No LoopForge source was copied into `src/`.

`oracles/` is gitignored. Open the path directly. `docs/validation.md` in the
oracle is a 2026-10-16 record and says later revisions are not certified by
it. Use the pin's source and tests.

## How to read the oracle

LoopForge is a self-hosted team product. A web workbench, MySQL, and Docker
sandboxes surround other coding CLIs. CA is the agent. Read the engine, then
stop.

| Question | Open |
| --- | --- |
| Node shape | `internal/domain/workflow.go` `WorkflowNode`. `Type` is `agent`, `script`, `approval`, or `sop`. |
| Script body | Same struct: `ScriptLanguage` is `shell` or `python`. `Script` is the program. |
| Send back | `RejectTo`, `MaxReworks`, `OnFailure` (`fail` or `reject`). |
| Agent terminal decision | `internal/engine/gate_submit.go` `applyGateResult`. Decisions are `pass`, `rework`, `fail`, `human`. |
| Rework budget | `internal/engine/node_outcome.go`. A `rework` with no `RejectTo` becomes a terminal fail. When `ReworkCount` has reached `MaxReworks`, the run waits for a person. |
| Human pass | `internal/engine/run_ops.go` `Pass`. The node is accepted and downstream continues. The node is not executed again. |
| Human send-back | `run_ops.go` `Reject`. The chosen target and its descendants return to `pending`. The target's `ReworkCount` increments. Its `Feedback` keeps the reason. At the limit, `Reject` returns `rework limit reached`. |
| Human rerun | `run_ops.go` `Retry`. One node reopens. Upstream successes and their artifacts stay. |
| Nested SOP | `internal/engine/sop_node.go`. The child uses the parent checkout. Nesting stops at 6. |
| Delivery hash | `e2e/api/45-delivery-model.test.mjs`. A script writes `sha256sum` of the changed file. After approval, a later script runs `sha256sum -c`. The test also requires the stored artifact hash to be the same. |
| External write | `internal/runtime/conversation_approval.go`. A provider hook can block `git push` and other writes that leave the sandbox until the current speaker answers. |

The README's delivery picture is that same test: fixture script, plan agent,
implement agent, review agent, verify script (`unittest` plus `grep APPROVED`
plus the hash), approval node whose `RejectTo` is `implement`, summary agent,
confirm script.

## Already in CA

Leave these in place. Extend them.

- Plan roles are `Author`, `Review`, `Revise`, and `Implement` in `src/plan_workflow.rs` `WorkflowRole`. A run can `Pause`, `Resume`, and `Cancel`. A review loop has `max_review_passes`.
- `fail_stage` sets the current stage and the whole workflow to `Failed`. A failed plan stays failed.
- `src/workflow.rs` steps are prompts with `depends_on`, grants, a retry policy, and evidence. Default evidence is `WorkflowEvidenceGate::AssistantOutput` (nonempty text). `ToolSuccess` records that a named tool succeeded. `Artifact` checks a SHA-256 that the definition already contains. `artifact_proof_ok` reads that digest back. It does not store a new one.
- A `.script` workflow is parsed by `parse_workflow_script` in `src/parity_backend.rs`. Legal lines are `budget`, `phase`, `agent`, `pause`, `when`, `loop`, `parallel`, and `join`. Each `agent` line becomes a prompt. The parser rejects a file with no agent line.
- Builtin workflow ids are `deep-research`, `fact-check`, and `research-experiment` (`BUILTIN_WORKFLOW_IDS`). Those ids are reserved.
- `src/publish_receipt.rs` is the host record of a literal `git push`: HEAD, parent, remote SHA, path counts, and lockfile-dependency flag. The model does not compute it. `bind_approval` clears approval when the remote SHA differs from HEAD, or when the review missed the pushed commit. `note_command` in `src/agent/tools.rs` records the command for that probe.
- The question form in `src/interaction.rs` is a model tool. The words "Push this commit?" appear in `a_question_form_keeps_the_recommendation_and_the_custom_answer`. That test checks the form widget. It does not bind the answer to a `PublishReceipt`.
- Session mode cycles Normal, Plan, Auto-review, and Yolo. The status toast is `Yolo` (`status_message` in `src/tui/input.rs`). Plan mode refuses unquoted redirects and a closed write-program set through `command_writes` in `src/parity_backend.rs`.

## Slices

Do slice 1 first. Slice 5 uses it. Slices 2, 3, and 4 can follow in that
order. Each slice gets its own failing test, then one
`python3 scripts/check-change.py --selector <name>`. Docs-only edits skip the
release. A Rust change follows `AGENTS.md`: that one check, then
`bash tools/bug257_install.sh` on the same artifact.

### 1. Send a failed stage back to a named earlier stage

LoopForge `Reject` keeps the run `running`. It resets the named target and
every descendant, stores the human reason on the target, and stops at
`MaxReworks`.

CA `fail_stage` ends the workflow. `WorkflowRetryPolicy` in `src/workflow.rs`
repeats the same step, and only for `ProviderError` or `EvidenceFailed`, at
most three times (`MAX_RETRIES`).

Add one plan action beside `Pause`, `Resume`, and `Cancel`:

- The caller names an earlier stage and a reason.
- That stage and every stage after it return to pending.
- The run stays able to continue. Upstream stages before the target stay completed.
- The reason is the only new text the target sees.
- A second send-back to a stage that already received one is refused. Count on the target, the way `ReworkCount` does.
- `fail_stage` still ends the run for a cancelled run, a missing stage, or an exhausted send-back.

Done when a test starts a two-stage plan, fails the second stage by this
action, observes the first stage pending again with the reason attached, the
workflow still running, and a second send-back to that same stage refused.
`fail_stage` still produces `WorkflowStatus::Failed` when the test calls it
directly.

Landed. `send_back_reopens_an_earlier_stage_once_and_fail_stage_still_ends_the_run` passes. `/plan send-back <stage number> <reason>` queues the action. The stage number is the one shown in plan status. A second send-back to that stage is refused.

### 2. A script step that never calls a model

LoopForge `Type: "script"` runs `Script` as shell or python. The delivery
test's `verify` and `confirm` nodes are scripts. Their success is the
command's exit status and the files they write. Token use is asserted only on
the four agent nodes.

CA has no step kind whose success is a host command. A `.script` file still
requires an `agent` line. `Artifact` evidence wants the digest inside the
definition before the run, so a step cannot record a transcript it just
produced.

Do not start this slice until verify-drive M4 and M5 are recorded done. Both
edit `src/workflow.rs`, and verify-drive is the named continuation. The new
step kind stores its own digest on step state. It must not change
`WorkflowEvidenceGate::Artifact` or the proof JSON.

Add one step kind on `WorkflowStep`:

- The host runs one fixed command in the workspace. The provider attempt counter stays at zero.
- Success is exit 0. The host writes the transcript and stores its SHA-256 on the step state at completion.
- A later step can require that stored digest. Changing the file after the hash fails that later step.
- Assistant text does not satisfy the step.
- Keep the existing prompt step. A definition that omits the new kind keeps today's behavior, including the `AssistantOutput` default.

Done when a fixture definition runs a command that writes a file, records the
digest, and a following check step fails after the test mutates the file. A
prompt-only definition still completes on nonempty assistant text. The test
asserts zero provider attempts for the command step.

The copy-once blueprint belongs in a test fixture, with a new id. Leave
`deep-research`, `fact-check`, and `research-experiment` reserved. Shape it
like `e2e/api/45-delivery-model.test.mjs`: fixture command, implement prompt,
review prompt, verify command, human gate from slice 1, confirm command that
checks the hash. Ship the fixture when slices 1 and 3 are both done. Until
then, the single command step above is enough.

### 3. The reviewer is a different participant

The delivery agents are separate records: `delivery-plan`,
`delivery-implement`, `delivery-review`, `delivery-summary`. The verify script
requires the review file to contain `APPROVED` and requires the tests to pass.
The review prompt says to leave the code unchanged.

CA can store a route on each `WorkflowParticipantInput`. No check requires the
review participant to differ from the implement participant. Default evidence
accepts the implementer's own nonempty text.

When a plan has both an `Implement` stage and a `Review` stage:

- The review participant id differs from the implement participant id. The same id is a start error.
- Review evidence is a host check the implementer's prose cannot satisfy. Use the script step from slice 2, or `ToolSuccess` of a named check, once that step exists.
- Until slice 2 exists, reject a review stage whose only evidence gate is `AssistantOutput`.

Done when a plan whose review participant equals its implement participant
fails at start, and a review stage with only `AssistantOutput` fails at start.
A plan with two participant ids and a non-text gate still starts.

The participant half is landed. `review_participant_must_differ_from_the_implement_participant` passes. A review stage and an implement stage with the same participant id fail at start. `assistant_output_review_fails_at_start_and_a_paused_note_reaches_the_next_stage` passes. A review stage whose only evidence gate is `AssistantOutput` fails at start when an implement stage is present. A plan with two participant ids and a tool-success gate still starts. Pausing, recording a workspace note, and resuming puts that note in the next stage input. A failed workflow that was not sent back stays failed. Slice 2 stays unstarted.

### 4. Ask the push question from the receipt

`PublishReceipt` and `bind_approval` already bind a literal `git push` to its
commit. The card a person sees can still be any question the model composes.
The form test's "Push this commit?" choices are model-supplied labels.

When a turn has a non-empty publish receipt:

- The host opens the question. The choices are fixed: push this commit, or leave it local.
- The card text comes from `format_blocks`: HEAD, remote SHA, match flag, path counts, lockfile flag, and the pushed command.
- The model cannot add a third choice or rewrite the command line.
- Only the turn that produced the receipt can answer. A later turn sees a fresh receipt or no card.
- Answering "push" still has to pass `bind_approval`. A remote mismatch stays refused.

Done when a test with a receipt opens that host card, a model-supplied
`RequestUserInput` during the same turn does not replace it, and a receipt
whose remote SHA differs from HEAD stays unapproved after the push choice.

Leave `a_question_form_keeps_the_recommendation_and_the_custom_answer` green.
That test is the free-form widget, not the push gate.

`host_publish_card_keeps_the_receipt_text_and_refuses_a_remote_mismatch` passes. The host card keeps the receipt text, and push refuses a remote SHA that differs from HEAD.

### 5. Continue a paused plan in the same workspace

LoopForge can bind a conversation to a paused SOP workspace, edit there, and
`Retry` a downstream node. `sop_node.go` keeps the child in the parent's
checkout. Files are not rolled back on send-back. `Reject` resets node state.
It does not restore an older tree.

After slice 1, a person can type in the parent session while the plan is
paused or has just been sent back, and the next dispatch uses that same
workspace. Starting a second plan is a different action.

Done when a test pauses a plan, records a workspace note from the parent
session, resumes, and the next stage's input contains that note. The completed
upstream stage stays completed. A failed workflow that was not sent back stays
failed until slice 1's action is used.

`assistant_output_review_fails_at_start_and_a_paused_note_reaches_the_next_stage` covers this note. Completed upstream stays completed.

## Leave out of CA

- Go, SQL, or web files copied from `oracles/loopforge/` into `src/`.
- A web workbench, accounts, MySQL, a visual SOP canvas, WeCom, or a Chinese UI.
- Wrapping CodeBuddy, Codex, Claude Code, Cursor CLI, or the other executors. CA is the agent those products call.
- Docker-socket sandboxes and a per-chat empty directory the agent clones into. CA keeps the checkout the user opened, and CA's own sandbox.
- Requirement trackers, TAPD import, token-price dashboards, and an `/insights` rollup.
- An auto-approver that answers an external write without the current turn. LoopForge has that mode. CA's receipt stays a question.
- `docs/validation.md` treated as a current pass. It is historical at this pin.

## Suggested skills

Use `tdd` when a task names a slice and asks for the test first. Use the
repository `AGENTS.md` release rule after a Rust change. Leave `implement-spec`
and `to-tickets` unused: this file is the comparison, and it is not a spec or
a ticket split.
