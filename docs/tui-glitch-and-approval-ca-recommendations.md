# Transcript overlap and yolo approvals

This file is a plan for a later agent. It is not `NEXT_STEPS.md`. The named
continuation is verify-drive M4 and M5. Do not start this file unless the
task names it. AGT-271 is finished except a separately authorized live
cheaper-quality claim. Do not start the edits from this file alone. Implement
one numbered slice at a time, and stop when that slice's done line is true.

The screenshots are a live CA session on `freebsdosx`, model grok-4.7, status
line `yolo`. The diagnosis below is the published tree at
`4c1643b3bea621284e3bbf180bcde81a1a0dec81` (`Cargo.toml` `0.1.190`). The
installed binary at the earlier comparison was `ca 0.1.198`. Re-read the
named functions before editing.

Two separate faults are in the screenshots.

## 1. Tool rows and diff tails share a screen row

The transcript shows a `search_replace` header, then the diff of
`seat-files.c` and `seat-wallpaper.c`, with the tail of a C line sitting on
the right of the next row. Examples of the tail: `d ? &hi : &fg, font,` and
`}{ 52, 88, 62 }, t);`. Headers are also smashed into the source, as in
`+ ✓ search_replace {"filXftDrawStringUtf8`. `↓1 more` at the top is the
capped user-prompt card (`long_user_prompt_card_is_capped_with_scroll_affordance`).
Leave that card. It is not the overlap.

`sanitize_terminal_text` in `src/terminal_text.rs` keeps `\t` and `\n` and
drops other controls. Diff lines from C sources are full of tabs.
`src/tui/transcript_cache.rs` `Layout::build` counts rows with
`Paragraph::line_count` and `Wrap { trim: false }`. A tab's Unicode width is
not the columns a terminal uses when the cell's symbol is a tab. The layout
reserves one row. The terminal advances the cursor by a tab stop, the rest of
that row wraps, and the next transcript line is painted from column 0 of the
wrapped row. The right side of that row is the previous line's tail.

`render_diff_lines_expanded` in `src/tui/render.rs` truncates with
`chars().count()`, not display columns. A tab counts as one character there
too.

### Slice A. A transcript row occupies the columns it paints

Before a string becomes a `Line` in the transcript, expand each tab to
spaces at a width of 8, and drop any other character that moves the cursor.
Measure the row with that expanded string. `line_count` for the block must
equal the number of rows the buffer paints.

Done when a fixture diff line of tabs, wider than the viewport, is followed
by a `search_replace` header: in a buffer of that viewport width, the header's
first cell is on the next row, and no cell of the header's row contains the
tail of the diff line. A line with no tab renders as it does today.

Landed. `tabbed_diff_line_does_not_share_a_row_with_the_following_header` passes. Transcript layout expands each tab to an 8-column stop and drops other cursor-moving characters before it measures the row.

## 2. Yolo still stops on the approval card

The second screenshot is an approval card while the status line already says
`yolo`. The card is `Approve command [2/2] – run_command`, reason
`compound shell syntax`, scope `this exact command for this run`, command
`rm -f` of several files under `/Volumes/projects/freebsdosx`, truncated.
The activity line says `Running 2 commands`. The card keys are
`[y] run [n] deny [a] yolo [e] edit [s] tool always [g] this run [p] path always [x] always deny`.
The footer repeats `[y] run · [n] deny · [a] yolo · [e] edit`.

`tool_needs_approval` in `src/main.rs` returns false for
`ApprovalMode::RunEverything`. That only clears `BrokerDecision::Prompt`.
`reclassify_pending_approval` still keeps the card for `ExplicitPrompt` and
`NetworkPrompt`.

`rm` is `CommandFloor::Ask` in `score_words` (`src/parity_backend.rs`), with
`sudo`, `curl`, `wget`, `dd`, `chmod`, and `chown`. `tighten_shell` in
`src/permission.rs` turns an Allow into Ask when `command_requires_ask`.
`classify_decision` maps Ask to `ExplicitPrompt`. A compound command is any
string that `split_command_segments` divides, and the strictest segment wins.
The card label is a separate scan: `approval_reason_and_scope` calls the
command compound when it contains `;`, `|`, `&`, `$`, or a backtick. An `rm`
joined with `&&` gets that label even though the floor reason is `rm`.

The batch is one card at a time, so `[2/2]` waits even though the status line
says both commands are running.

Keep these stops in yolo. Tests already cover them:

- A persistent deny still denies (`yolo_executes_but_persistent_deny_still_wins`).
- An explicit user Ask rule still asks and runs nothing (`rich_ask_rule_blocks_yolo_without_side_effect`).
- Outbound email still does not run (`yolo_and_generic_allow_cannot_authorize_outbound_email`).
- `NetworkPrompt` stays a card. A literal `git push` stays on the publish receipt.

The shell floor itself stays. Plan mode and the allowlist still ask before
`rm`.

## Shared approval order

When a task names more than one of these, do them in this order:

1. Terminess slice 3 binds the answer to the card id. It does not change which commands ask.
2. Slices B and C of this file.
3. SoreQen slice 3 changes Auto-review only, and only after a snapshot.

These stay true after each slice. A persistent deny denies. An explicit user Ask asks and runs nothing. Outbound email does not run. `NetworkPrompt` stays a card. A literal `git push` stays on the publish receipt. Plan mode refuses writes. An answer applies only to the card on screen. Yolo and Auto-review stay different modes.

Slice B is an approval widening. It starts only when the task names this file.

### Slice B. Yolo runs a floor-Ask command inside the workspace

In `reclassify_pending_approval`, a yolo session executes the pending command
when the Ask came from `tighten_shell` (`PolicySource::NoMatch` and
`MatcherCategory::CommandPrefix`) and every path the command names is inside
the workspace. A compound command follows the strictest segment. If any
segment is a user Ask rule, a deny, a network prompt, or a path outside the
workspace, the card stays for that command.

A batch of commands that all pass this check runs without a card. The
`[2/2]` counter does not appear for them.

Done when a yolo fixture of `rm -f` on two workspace files, including one
command that also contains `&& rm -f` of another workspace file, finishes
both with no approval card. The same `rm` in allowlist mode still cards. An
explicit Ask rule on `run_command` still cards in yolo and does not delete
the file. `rm` of a path outside the workspace still cards in yolo. The
persistent-deny, email, and `git push` checks above still hold.

`yolo_runs_workspace_rm_and_still_cards_an_outside_path_and_an_ask_rule` passes. Yolo runs `rm -f` of workspace files, including an `&& rm -f` of another workspace file. Allowlist still cards that `rm`. An explicit Ask still cards in yolo. An outside path still cards.

### Slice C. The card that remains says why, once

When a card is still required in yolo, the reason is the floor or the rule
that kept it (`rm` outside the workspace, explicit Ask, network, deny). It
is not `compound shell syntax` unless the operator is the reason the card
stayed. The command wraps inside the card width instead of one truncated
line. The footer `key_line` does not repeat the card's keys while the card
is up. `[a] yolo` is omitted when the mode is already yolo.

Done when the outside-workspace `rm` card from slice B names the outside
path, shows the whole command across wrapped rows, and the footer is the
ordinary idle or running hint. A session that is not yolo still shows
`[a] yolo` on the card.

`outside_rm_card_names_the_path_wraps_the_command_and_hides_yolo_when_already_yolo` passes. The card names the outside path, wraps the command, and omits `[a] yolo` when the mode is already yolo. A session that is not yolo still shows `[a] yolo`.

## Suggested skills

Use `tdd` when a task names a slice and asks for the test first. Use the
repository `AGENTS.md` release rule after a Rust change. Leave
`implement-spec` and `to-tickets` unused.
