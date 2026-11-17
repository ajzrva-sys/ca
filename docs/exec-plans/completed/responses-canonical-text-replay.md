# Responses canonical text — plan stage dies after list_dir

Status: COMPLETE
Owner/session: 01a1e53e-6216-7522-8637-294fcb6fb334
Last updated: 2026-11-16

## Goal

A plan author on `xai-oauth` / `grok-4.7` can continue after a successful `list_dir`. The status line must not become:

`Plan stage ended Failed: provider protocol violation: response output message does not match canonical assistant text`

A genuinely different assistant sentence must still fail that same check.

## Non-Goals

- Do not start held exec plans. AGT-271 M5 stays finished.
- Do not change filename privacy, approval, or tool schemas except where the red fixture proves this check is the failure.
- Do not mutate `~/.config/ca`, session JSON, or installed recovery binaries while diagnosing.
- Do not copy oracle source. Do not edit `oracles/`, `reference/`, or `ca-hrs-252/`.
- Do not weaken the check so that any two sentences pass.

## Evidence and Current Behavior

The notice is built in `src/plan_workflow_runtime.rs` when the author child is not `Completed`. It prints `snapshot.result_summary`. That summary is `ProviderError::Protocol` from `validate_response_output_replay` in `src/agent/provider.rs`.

The check concatenates `output_text` parts of same-route `ResponsesOutput` message items and compares them with `message.content`. Reasoning items are ignored. Function calls are compared only after the text matches, and that failure uses a different sentence (`response output function calls do not match canonical assistant tool calls`). One `restore_view_in_place` runs on the item text when the strings differ. A second mismatch returns the sentence above.

What the live runs show:

- Two parent sessions on 2026-11-16, both `xai-oauth`, `grok-4.7`, reasoning high. Session `c245ec6e-a4db-4f53-b007-a17c9e9931d2` (plan `01c25e62-…`, author agent `14c1bdde-9869-47a9-99fc-a4d38569a467`, build `74f7fa17…`, about 17s). Session `5a806e5f-4f3f-451e-b571-8e5eb6a398c7` (author agent `cac66121-c228-4544-b26a-e49946f6521b`, build `6ecc9a43…`, about 20s).
- Each outcome is `delegated_task` → harness epoch → `list_dir` success → tool evidence of `__CA_PATH_…` rows → this provider failure.
- Parent sessions store the user message only (`message_count` 1). The child transcript, the two compared strings, and the provider body are not in those files.
- `~/.local/share/ca/traces/` from this investigation is October 2026 DeepSeek traffic. Runtime logs do not contain this sentence or the 16:43 timestamp. Provider trace is off unless the process is started with `--trace` (`src/main.rs` `cli.trace`).

Installed binary is 0.1.144. CONTINUITY already records the earlier fix: a Responses continuation restores filename aliases inside the privacy scope on the blocking prepare worker. `responses_replay_restores_filename_alias_inside_private_prepare` passes. That test still requires a different sentence to fail. These plan-author failures happened on that build.

Code facts that bound the bug:

- The stream completed and `list_dir` ran. A completed message whose text is not a prefix of the deltas for the same `(output_index, content_index)` fails earlier with `conflicting completed response text`. A conflicting completed item fails with `conflicting completed response output item`. Neither string was shown.
- `reconcile_completed_output_item` checks only text parts that exist on a completed message item. `response.output_text.delta` events for any other `(output_index, content_index)` are still emitted as `ProviderEvent::Text`. Nothing at `response.completed` requires every streamed text key to appear in a completed message part.
- `AssistantTurnAssembler::finish` stores restored delta text as `content` and stores the raw provider items as `response_items`. The subagent keeps that pair (`src/agent/subagent.rs`).
- Equal raw strings passed through `restore_view_with_spans` once under the same privacy context cannot diverge. `project_tool_output` rewrites the tool message only. Harness guidance, evidence guidance, host contexts, context pruning, context observation, span annotation, DSML (inactive on Responses), and the stream anomaly inspector do not rewrite assistant text. The contained planner is skipped when filename mode or tools are present, which is this path.
- xAI's current function-calling docs say a streamed function call arrives as one chunk. That shape is already accepted on `response.output_item.added` when `arguments` is non-empty. It changes the tool-call compare, which is a later and different error.

Definitions:

- Canonical assistant text: `ChatMessage.content` stored for the turn. For Responses this is the local restored view.
- Replay text: concatenation of completed message `output_text` parts, restored at most once during prepare.
- Uncovered delta: a `response.output_text.delta` whose `(output_index, content_index)` is not a content part of a completed message item.

## Scope

Allowed files:

- `src/agent/provider.rs` and, only if the red fixture requires it, `src/agent/stream.rs`
- `src/privacy/filename_tests.rs` for the list_dir continuation fixture
- `CONTINUITY.md` with the outcome and the install readback
- This plan, copied to `docs/exec-plans/active/responses-canonical-text-replay.md` at the start of implementation

Forbidden files/areas:

- `oracles/`, `reference/`, `ca-hrs-252/`
- `~/.config/ca`, session files, provider traces the user did not just create
- Held plans under `docs/exec-plans/active/` other than the new file above

Runtime state affected: one replace of the installed `ca` through the standing post-task rule, after the focused check passes. Leave running sessions alone.

Interfaces: OpenAI Responses decode, assistant-turn assembly, and `validate_response_output_replay`. No schema or session-format change. Stored `response_items` stay the provider's completed items.

## Design and Invariants

The completed response output is the history that will be replayed. Canonical assistant text for a Responses turn must be the concatenation of those message `output_text` parts, then one restore. Uncovered deltas may be shown while tokens arrive. They must not become the stored assistant message.

Ranked predictions. Run them in order. Stop at the first red fixture and fix that fixture. Do not apply a later fix while an earlier fixture is still green.

1. Uncovered output-text deltas. If grok-4.7 emits assistant prose as `response.output_text.delta` on an index the completed output does not store as message text (reasoning slot, extra `content_index`, or no message item), the stream finishes, the tool runs, and the next prepare returns this exact sentence. Changing stored `content` to the completed message text, before the single restore, makes the continuation succeed. A different sentence still fails.
2. Filename-map change after `list_dir`. If prediction 1 is green, registering child `__CA_PATH_` aliases in `project_tool_output` makes a later `project_private_request` fail for an assistant turn whose raw text already equals the item text. The fix then belongs in that restore, and the uncovered-delta overwrite must not land.
3. Missing bytes. If predictions 1 and 2 are green, the live strings are not in the tree. Add lengths to the protocol error and stop. Ask for one `ca --trace` plan attempt. Do not guess a third rewrite.

Invariant to keep: `responses_replay_restores_filename_alias_inside_private_prepare` stays green, including the different-sentence failure. Replay items stay the provider JSON. Tool-call mismatch remains a separate error. `conflicting completed response text` still fires when a completed part disagrees with the deltas for its own key.

On the red uncovered-delta fixture, the edit is in `AssistantTurnAssembler::finish` (or the decoder, if the test shows the events are already wrong before assembly): when Responses message items are present, set the pre-restore content from `response_message_text_parts` in output order. Then run the existing `restore_view_with_spans` once. No message item means empty canonical content; the function-call items and tool calls stay. Do not invent or edit a response item.

Also include `canonical_len` and `item_len` in the protocol error. Lengths only. No path text, alias, or sentence body. The status line can then distinguish a short gap from a different sentence without a saved body.

## Milestones

### M1 — Red fixture before any production edit

Add one test next to `responses_decoder_maps_text_reasoning_tools_usage_finish_and_done` in `src/agent/provider.rs`.

Feed an SSE body shaped like a grok-4.7 tool turn:

- reasoning item at output index 0 with `encrypted_content` and an empty or summary part
- `response.output_text.delta` prose on output index 0 (the reasoning slot), and a second delta on content index 1 of the message slot
- one `response.output_item.added` function call whose `arguments` are the whole `list_dir` JSON (`{"path":"."}`), matching the xAI one-chunk tool shape
- `response.completed` output: reasoning, one message part that does not contain the uncovered prose, and that function call

Decode, assemble, build the assistant `ChatMessage` the subagent stores, and call `response_output_items_for_replay`.

Expected red result today: the stream finishes with `FinishReason::ToolCalls`, and replay returns `response output message does not match canonical assistant text`.

Control in the same test module, expected green today: the same body with every delta on the completed message part. Replay succeeds.

Run that test once and paste the failure into this plan before editing `finish`.

### M2 — Store the completed message text

Only if M1 is red. Set canonical content from the completed message parts before the one restore. Re-run M1. The uncovered body must replay. The matching-delta control must still replay. The different-sentence case in `responses_replay_restores_filename_alias_inside_private_prepare` must still fail.

### M3 — list_dir alias continuation, only if M1 is green

In `src/privacy/filename_tests.rs`, extend the existing filename fixture:

- assemble a turn whose raw prose equals the message item and mentions the fixture alias
- `project_tool_output` for a successful `list_dir`, which registers child aliases
- `project_private_request` on that assistant message plus the tool message

Expected if this is the bug: `project_private_request` returns the canonical-text sentence. Fix the restore so the same raw sentence still matches after the new aliases, and keep the different-sentence failure.

If this fixture is green, skip the content overwrite.

### M4 — Lengths, then stop if both fixtures are green

Change the protocol error to include `canonical_len` and `item_len`. Assert the error text does not contain the fixture sentence. Update CONTINUITY to say both structural fixtures pass and the live bytes were not captured. Ask for one plan author run under `ca --trace`. Read the redacted frames. Do not change restore or assembly on a green pair of fixtures.

### M5 — Release

One `python3 scripts/check-change.py` with `--release` and the selectors below. Then `bash tools/bug257_install.sh` on that artifact. Record the version, both hashes, and `codesign` in CONTINUITY. Move this plan to `docs/exec-plans/completed/` after those commands are in the Completion Record.

## Acceptance Criteria

- A decoded grok-shaped tool turn whose uncovered `output_text` deltas are absent from the completed message replays without the canonical-text error, and the stored content is the completed message text.
- The same turn with matching deltas still replays.
- `project_private_request` after `list_dir` alias registration still replays an assistant sentence that matches its item, and still rejects a different sentence.
- The user-visible protocol error includes `canonical_len` and `item_len` and does not include the assistant sentence.
- Plan-stage status text stays `Plan stage ended {:?}: {}`. No TUI copy change beyond the child error string.
- A failed workflow still does not resume.

## Validation

Focused, one Cargo stage:

```text
python3 scripts/check-change.py --release \
  --selector responses_tool_turn_uncovered_output_text_replays_completed_message \
  --selector responses_replay_restores_filename_alias_inside_private_prepare \
  --selector responses_list_dir_alias_registration_keeps_assistant_replay
```

Use the real test names if the implementer adjusts them. The selectors must be the new uncovered-delta test, the existing alias test, and the list_dir continuation test.

Completion is that focused check plus `bash tools/bug257_install.sh` on the same artifact. Do not start `./scripts/verify.sh` and the focused check together. Do not `cargo clean`.

Manual: one plan author on `xai-oauth` / `grok-4.7` that lists a directory and reaches a reviewable plan. This needs the user's SuperGrok session. Record it as blocked if it is not run. A green unit fixture does not by itself prove the live turn.

M1 went red on the uncovered-delta fixture, so that is the recorded cause. A live `xai-oauth` / `grok-4.7` plan author was not run after the install.

## Progress

- [x] Locate the status line, the replay check, and the two live sessions
- [x] Rule out guidance, pruning, observation, host context, DSML, and the contained planner as rewriters of assistant text
- [x] M1 red fixture recorded
- [x] M2 fix: stored content is the completed message text
- [x] Lengths on the protocol error
- [x] M3 list_dir alias continuation stays green, so restore was not changed
- [x] Focused check, install, CONTINUITY

### M1 red output, before the finish edit

`responses_tool_turn_matching_output_text_replays` passed. `responses_tool_turn_uncovered_output_text_replays_completed_message` failed:

```text
replay rejected completed message text: provider protocol violation: response output message does not match canonical assistant text; canonical=Some("uncovered reasoning proseextra partListing the workspace.")
```

The stream finished as a tool turn. Canonical text was the uncovered reasoning-slot delta, the extra content index, and the completed message, concatenated. After `AssistantTurnAssembler::finish` stores the completed message text, that test passes and the stored content is `Listing the workspace.`

## Decisions

- Decision: completed message `output_text` is canonical history for a Responses turn.
  Evidence: replay sends those items unchanged, and the check exists to keep `message.content` aligned with them.
  Consequence: uncovered deltas are not stored. The live token stream can show them before finish replaces the stored message with the completed text.
- Decision: do not invent a message item to hold uncovered deltas.
  Evidence: xAI continuation replays provider items, including encrypted reasoning.
  Consequence: a turn with only reasoning plus a function call stores empty assistant text and still continues.
- Decision: no production edit until M1 has been run.
  Evidence: the live request body was not saved, and the alias restore fix already landed in 0.1.144.
  Consequence: a green M1 moves the work to M3, then to a trace, instead of shipping the content overwrite.

## Surprises and Discoveries

- The tool result has empty `response_items`, so its own replay check is `Ok(None)`. The failure is the previous assistant message on the continuation prepare.
- `restore_view_in_place` no-ops when `rewrites_outbound()` is false. `restore_view_with_spans` does not. Assembly uses the spans helper. Prepare uses the in-place helper. With an enabled privacy context both run. The 0.1.144 test covers that scope. It does not cover uncovered deltas or aliases registered after the assistant turn.
- Child messages never reached the parent session file, so the compared strings cannot be recovered from disk.

## Recovery and Rollback

M1 is a new test. Revert that test if it asserts the wrong symptom.

The content overwrite is safe to redo: it derives stored text from items already on the turn. It does not rewrite `response_items`.

The install follows `docs/VERIFICATION.md`. Recovery of 0.1.144 is `/Users/aji/.local/bin/.ca-release-stage.aMDTup/`. Keep that directory. A bad 0.1.145 is rolled back by restoring that recovery copy, not by editing sessions. The older 0.1.143 recovery remains at `/Users/aji/.local/bin/.ca-release-stage.U9rQRc/`.

Do not rebuild the shared Cargo target from scratch. One Cargo stage at a time.

## Completion Record

Changed files:
- `src/agent/stream.rs` — a Responses turn stores the completed message text before the one restore
- `src/agent/provider.rs` — uncovered-delta fixture, and the protocol error includes `canonical_len` and `item_len`
- `src/privacy/filename_tests.rs` — list_dir alias registration still replays a matching sentence and still rejects a different one
- `Cargo.toml`, `Cargo.lock` — 0.1.145
- `CONTINUITY.md`, `NEXT_STEPS.md`

Commands and results:
- M1, before the finish edit: `responses_tool_turn_matching_output_text_replays` passed. `responses_tool_turn_uncovered_output_text_replays_completed_message` failed with `response output message does not match canonical assistant text` and canonical `uncovered reasoning proseextra partListing the workspace.`
- After the edit, those two tests, `responses_replay_rejects_output_that_disagrees_with_canonical_history`, `responses_replay_restores_filename_alias_inside_private_prepare`, and `responses_list_dir_alias_registration_keeps_assistant_replay` passed.
- `python3 scripts/check-change.py --release` with those selectors passed: `scratch/check-change/runs/20261116T171841Z-70615/report.json`. Not a full gate. An earlier 0.1.144 run, `scratch/check-change/runs/20261116T171157Z-70393/report.json`, also passed and was not installed.
- `bash tools/bug257_install.sh` installed that 0.1.145 candidate.

Artifacts:
- Candidate SHA-256 `d27ec1aaa48d50c15fec3b46edf4a92bf4de1324a0899293444aea87c87b8ec7`
- After `ca --version`, on-disk readback `c766d784c6f6e8a6a370855d9fab6bcb514119be70cece8d3043f45e1534b9a8`
- `codesign -vvv` valid on disk
- Recovery of 0.1.144: `/Users/aji/.local/bin/.ca-release-stage.aMDTup/ca.recovery-ORC258-20261116T172444Z-0ed320d488ec9e32e5443ccd862c7007d61547a08c932430a01f38269ff65bfb`

Remaining risks:
- A live plan author on `xai-oauth` / `grok-4.7` was not run. The fixture covers uncovered deltas and a list_dir alias registration. It does not replay the unsaved provider body.
- `./scripts/verify.sh` was not run.

Harness updates:
- CONTINUITY records 0.1.145 and this cause.
- NEXT_STEPS points at this completed plan.
