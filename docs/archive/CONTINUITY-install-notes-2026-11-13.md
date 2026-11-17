# Archived continuity install notes

Moved from `CONTINUITY.md` on 2026-11-13. The short current page replaced it.

# Continuity

Installed CA is **0.1.125**. Historical continuity is archived at
[docs/archive/CONTINUITY-before-iso337-summary.md](docs/archive/CONTINUITY-before-iso337-summary.md).

## Harness update rules

Record the installed version, open risks, and unresolved doc or source conflicts here. Move a superseded install narrative to `docs/archive/` when it no longer changes the next edit. Leave API keys, session contents, and command transcripts out.

2026-11-13: matching Cursor Agent CLI/TUI behavior is not the product goal. `ca` is a local agent harness. `PARITY.md` remains earlier oracle notes.

2026-11-13 docs-only pass: root `.agent/PLANS.md` was required by `AGENTS.md` and missing. It is restored from the section list already used by LRN-150, SEC-158, and `ca-hrs-252/.agent/PLANS.md`. `REVIEW_CHECKLIST.md` is new. Product source is unchanged. Release skipped. `TESTING.md` now points the current install here; the BUG-340 install section remains the ca 0.1.98 record.

Open harness gap: `docs/.INDEX.md` still mixes the navigation map with completed-plan narration. See NEXT_STEPS DOC-001. GRD-010 is proposed and has no script.

The conversation screen has no title row. Two rows sit under the composer:
directory, model, context, and approval mode, then the keys for the current
focus. Tab selects the transcript. Up and Down move the block, Left and Right
fold a tool run or thought, Enter opens it, and Alt+Q quotes it. Questions and
approvals pin above the composer. Esc parks a choice; a second Esc cancels it.
Errors and scroll notices stay off the key row. The Terminal theme does not
fill the background. `NO_COLOR` still selects Mono. Installed signed ca
0.1.125 readback SHA-256
`9052baf9c6f99ce4cc239338d90d30328901188c34264a07f8fb267d8fe37db3`. The
installer printed candidate
`ca7ebfb29a211ace60ac865644de0944de36ac29c5f8bcba8e7064ec42599c7e`.
`codesign -vvv` reports valid on disk. Recovery of 0.1.124:
`/Users/aji/.local/bin/.ca-release-stage.3WhwG5/ca.recovery-ORC258-20261113T194349Z-7b21f1570ea0932d021032fd668ff2669065318b7765a354b013633f095102c4`.
Focused tests and fmt passed in
`scratch/check-change/runs/20261113T193412Z-10686/report.json`. Clippy then
rejected a hand-rolled clamp; the clamp fix passed
`cargo clippy --locked --offline --bin ca -- -D warnings`. Full `verify.sh`
was not run.

`browser_exec` runs a persistent Chromium session in Rust. The first `#`
comment is the step the terminal shows. One call can `goto`, `wait`, `page`,
`controls`, `html`, `click`, `click text`, `fill`, `scroll`, `press`, `js`,
`click_at`, and `shot`. `shot` attaches the screenshot to the tool result.
A `cdp_url` in `~/.config/ca/browser.toml` connects to that endpoint instead
of launching Chromium. `browser_vault` lists and fills saved logins without
returning the password. `save_login` reads only
`~/.config/ca/browser-vault.pending.json` and deletes it. Installed signed
ca 0.1.124 readback SHA-256
`7b21f1570ea0932d021032fd668ff2669065318b7765a354b013633f095102c4`. The
installer verified candidate
`bce72797bdb396a933551ac04cc169ada86c466d4c868813de20d4f469c43ecc`. Recovery
of 0.1.123:
`/Users/aji/.local/bin/.ca-release-stage.1w6X9R/ca.recovery-ORC258-20261113T162313Z-6b50e51cf1584ceed23548e3a234222ccbfda15c57c5dc74e75cd5318ea01d19`.
Focused check `scratch/check-change/runs/20261113T161704Z-71364/report.json`
passed. Full `verify.sh` was not run.



The `browser` tool is on for a normal main session and off for bare, scheduled,
and role runs. Exclude still removes it. It can list page controls and form
options, read one element's HTML, scroll an element into view or by a page,
and press a key, in addition to navigate, snapshot, click, type, screenshot,
download, and close. Navigate still needs network approval. Installed signed
ca 0.1.123 readback SHA-256
`6b50e51cf1584ceed23548e3a234222ccbfda15c57c5dc74e75cd5318ea01d19`. The
installer verified candidate
`4fbef53530df8093045a39c0415a847faed03b585b7b04345c4aa7816490fbe3`. Recovery
of 0.1.122:
`/Users/aji/.local/bin/.ca-release-stage.1jA9OR/ca.recovery-ORC258-20261113T155830Z-149473241c9792039375e72c8674191fa4fb671388c12bcd748478e7deeec47f`.
Focused check `scratch/check-change/runs/20261113T155207Z-17634/report.json`
passed. Full `verify.sh` was not run.



`/config` → Personality edits two sections of `personality.md`: Personality and
Pre-prompt. Guided questions still opens the questionnaire. The seed is the
Hermes technical voice with the Hermes and Nous identity removed, plus the
short technical pre-prompt. Joke built-ins, gateway text, and tool-enforcement
text are not imported. A heading that is already present is left alone,
including an empty body. Bare mode can show the text and cannot save it. A
running turn keeps the steering it already loaded. Saving the questionnaire
can fold those two sections into Additional preferences. Installed signed ca
0.1.122 readback SHA-256
`149473241c9792039375e72c8674191fa4fb671388c12bcd748478e7deeec47f`. The
installer verified candidate
`67750d1184ad61ed3dcd771f80149d6f4ce34430701623d5eb3a03ac9600cfa8`. Recovery
of 0.1.121:
`/Users/aji/.local/bin/.ca-release-stage.JVfGxm/ca.recovery-ORC258-20261113T145911Z-8e4d61188dcca05ed98fe42743639731ad85563c3c7d7f7c2ec4f0887c0450f7`.
Focused check `scratch/check-change/runs/20261113T145251Z-89687/report.json`
passed. Full `verify.sh` was not run.

The local `chatgpt-shim` (`~/.config/ca/chatgpt-shim/shim.ts`, not shipped in
this release) drives a private second copy of `/Applications/ChatGPT.app`.
The profile is `/tmp/chatgpt-shim-desktop-profile`, the Codex home is
`/tmp/chatgpt-shim-desktop-codex`, and the debugging port is `127.0.0.1:9335`.
That copy does not use `~/Library/Application Support/Codex`. The production
app was not running. The private window stays open. Its first screen is a
browser sign-in whose return is `http://localhost:<port>/auth/callback` on
that process only. Moving the window to `chatgpt.com` ends that wait, so a
later browser callback has nowhere to land. Chat does not need that sign-in.
Google Chrome `--headless=new` on `9333` remains the fallback if the private
app does not open the debugging port. A stale interstitial is reloaded once.
A prepare 401 refreshes the Codex token once and retries. Upstream failure is
an HTTP error, not a normal answer. ca treats a leftover
`[chatgpt-shim upstream error` stream as a failed turn, and on HTTP 401 from
that provider refreshes the Codex token and retries once. A live `gpt-5.6`
completion through the private app returned `pong` on 2026-11-13.

`/config` now includes the saved appearance and agent preferences that previously
lived only in the config file or in slash commands: transcript timestamps,
assistant footers, scrollbar, tool and thinking and edit block switches, tool
bullet, automatic dark and light themes, line numbers, reasoning details, Vim
mode, approval mode, task breakdown, proactive delegation, review changes, keep
awake, and tool discovery. Tool schema is a session choice. Reasoning effort
lists the current model's efforts. Sync theme, filter rules, prompt cache,
route presets, personality, memory, plugins, rules, custom commands, workspaces,
and direnv open the screens those commands already use. Numeric agent limits
stay in the config file. Installed signed ca 0.1.121 readback SHA-256
`8e4d61188dcca05ed98fe42743639731ad85563c3c7d7f7c2ec4f0887c0450f7`. The
installer verified candidate
`3ca992159a8f55a3c05505cc79ea6499c01b53bb6b772e0b7e93c67e7a9d1b55`. Recovery
of 0.1.120:
`/Users/aji/.local/bin/.ca-release-stage.r5jBIt/ca.recovery-ORC258-20261113T141757Z-d92d53fc571436e9ab1eee5101d871b9d231e953c95e572a339878b563346bc7`.
Focused check `scratch/check-change/runs/20261113T141124Z-99219/report.json`
passed. Full `verify.sh` was not run.

The pre-send filter review starts off. `[ui] filter_preview` saves the choice.
`/config` lists it as Filter, with on and off. `/filter on` and `/filter off`
save the same choice and still leave an armed stop filter running until
restart. Installed signed ca 0.1.120 readback SHA-256
`d92d53fc571436e9ab1eee5101d871b9d231e953c95e572a339878b563346bc7`. The
installer verified candidate
`243c88762bb08c18172d066914f85a0b163abecb75d2d99c1c2978e682ccdbff`. Recovery
of 0.1.119:
`/Users/aji/.local/bin/.ca-release-stage.O1SbZc/ca.recovery-ORC258-20261113T134437Z-f39f083a33e303e31cacf5df7ce8df5f8bac6a0c99611a0f3474ab489ab5603b`.
Focused check `scratch/check-change/runs/20261113T133848Z-28560/report.json`
passed. Full `verify.sh` was not run.

Learning and recall now enroll Grok sessions beside Codex, Hermes, and MiMo.
The root is `$GROK_HOME/sessions` when `GROK_HOME` is an absolute directory,
otherwise `~/.grok/sessions`. A missing tree stays unenrolled. Each session
contributes `summary.json` title and timestamps plus `user_message_chunk` and
`agent_message_chunk` text from `updates.jsonl`. Thoughts, tool calls, plans,
command lists, rewind, feedback, and `subagents/` stay out of the corpus.
Recall accepts source `grok`. Installed signed ca 0.1.119 readback SHA-256
`f39f083a33e303e31cacf5df7ce8df5f8bac6a0c99611a0f3474ab489ab5603b`. The
installer verified candidate
`d8044c711efffd105d4c3e2c2143c5174c34cd9bf6bfb98e3651acf46e9cfc37`. Recovery
of 0.1.118:
`/Users/aji/.local/bin/.ca-release-stage.CeJS0D/ca.recovery-ORC258-20261113T125835Z-b6ec9a26872176ec78bc015752fc5214ea32ed263de1397a80b89231ea223b85`.
Focused check `scratch/check-change/runs/20261113T125333Z-19718/report.json`
passed. Full `verify.sh` was not run.

Enter on an unfiltered `/config` row uses the same section order as the highlight.
Compact mode and Status indicators show the live session values, so a slash
toggle is not overwritten by the stale in-memory config. Installed signed ca
0.1.118 readback SHA-256
`b6ec9a26872176ec78bc015752fc5214ea32ed263de1397a80b89231ea223b85`. The
installer verified candidate
`2ca201e21277e36eb52bd362ed07fab6cbada3d417a38cd1b7f81bc378539d47`. Recovery
of 0.1.117:
`/Users/aji/.local/bin/.ca-release-stage.7GyxAV/ca.recovery-ORC258-20261113T104231Z-e20f4744b621962bc91c88b4767292e490101ab6caf6104447224b41a5c75ae0`.
Full `verify.sh` was not run.

`/config` is a near-full-screen settings sheet. Appearance now includes Theme
(Default, Ocean, Amber, Mimocode, Mono, Rose Pine), Compact mode, and Status
indicators, each saved as the explicit choice. Status line, Keymap, and
Permissions open the same screens as their slash commands. A weaker sandbox
choice still uses the sandbox downgrade confirmation. Notification, mouse,
command output, delegation, model, provider, MCP, skills, and hooks keep
their actions. Catalog tests
`settings_sheet_catalog_saves_theme_compact_mode_and_status_indicators` and
`settings_sheet_catalog_renders_theme_and_saved_rows` passed. Installed signed
ca 0.1.117 readback SHA-256
`e20f4744b621962bc91c88b4767292e490101ab6caf6104447224b41a5c75ae0`. The
installer verified candidate
`e4bd28517e7af266a7a2ef2faf1d528d745652c815981c93035eeaea49d64a30`. Recovery
of 0.1.116:
`/Users/aji/.local/bin/.ca-release-stage.E09Xc6/ca.recovery-ORC258-20261113T102029Z-cd5e2bb65bf61deaa31afb0c7caa2d71603629282396e183b2436671e4657ea0`.
Full `verify.sh` was not run.

`/config` is a near-full-screen settings sheet: section rules, `/` search,
in-sheet radios for notification, mouse, command output, delegation policy,
and sandbox, with a downgrade confirmation still owned by the sandbox hub.
Clipboard and image-file pastes insert `[Image #N]` chips. A trailing
`[sent-images:N]` record is hidden in the user row; chips beyond that count
are struck through. Focused check
`scratch/check-change/runs/20261113T094448Z-63281/report.json`. Installed
signed ca 0.1.116 readback SHA-256
`cd5e2bb65bf61deaa31afb0c7caa2d71603629282396e183b2436671e4657ea0`. The
installer verified candidate
`64fced7dfbd16b16ca3ea4c5317fac0a0fb07399ac2d5a6d6a1d310d874aeab4`. Recovery
of the previous binary:
`/Users/aji/.local/bin/.ca-release-stage.m2If8s/ca.recovery-ORC258-20261113T095132Z-797534e57f02d7a0651bc6049158a7c1b92b62e863f703d50e8ce423f6fb8091`.
Full `verify.sh` was not run.

## Hermes comparison, 2026-11-12

`oracles/hermes-agent` is `2597dc6b7e876e57f47fabeb1c4bc4efc1e12e85`
(2026-11-13T00:01:55+01:00), refreshed from `4eba3eb` (2026-10-11).
Recommendations are in
[docs/hermes-ca-recommendations.md](docs/hermes-ca-recommendations.md).

The recommended UI and backend items are in the source. The live child dock
sits in the activity strip above the composer, and Shift+X stops the selected
subtree. Alt+S cycles queue, steer, and interrupt while a turn is running.
Steer is injected with `wrap_steer` at the next tool boundary. The transcript
draws a window of messages and replaces the streaming reply in place. The goal
condition is on the composer footer. Plain `/command` and `@` tokens use the
accent style. Clipboard copy falls back to OSC 52 when the local clipboard
fails.

`execute_code` compiles one std-only Rust program and runs that binary under the `run_command` sandbox. Stub
calls use the existing tool handler and approval classification, and a call
that needs a prompt returns an error to the script. The precise edit chain
reports the matching strategy. Outbound tool images are retired in a batch
only when the request would cross the image ceiling. A streaming repetition
loop is stored as a marker. One empty reply is retried after a wedged-tail
repair. A code edit with auto-review off asks for one direct check. A git
workspace omits `email_*` and `generate_image` until `/tools full`. Tool reads
and writes refuse credential files and `.env`. Session titles use
`agent.compaction_model` when it is set. `execute_code` now compiles a
std-only Rust program instead of running Python. Focused receipt
`scratch/check-change/runs/20261113T013129Z-2084/report.json`. Installed
signed ca 0.1.114 SHA-256
`0e5e734ac46d708178b5554a6667c497340d2c5ebc7e384c8fb834075309236c`. Recovery of
0.1.113:
`/Users/aji/.local/bin/.ca-release-stage.w88aOx/ca.recovery-ORC258-20261113T013724Z-b3e857e152b0d28fc0d095376cbd2b073068e7ac038bab050cf5e0b223383e72`.
Full `verify.sh` was not run.

## Grok-gap wiring, installed 0.1.112

Oversized complete fetches are written under the session fetch directory and are not cached. Image reads over 32 KiB are re-encoded before normalize. The next successful tool result names a nearby unloaded skill directory once and names finished background jobs once, including "do not restart it." for a user kill. `claim_due` and `claim_immediate` write a fire receipt before the run; the same occurrence is suppressed, a bad receipt is quarantined, and one-shots stop at the cap. `call_tool` refuses an over-cap elicitation form and cuts the server off after three refusal notes. A repeated assistant reply with no tool call is retried once inside the prose caps. A successful edit appends a bounded diagnostic summary when a language server report exists. The pinned user prompt uses the user-card height, so the next prompt slides it off instead of being covered. `write_file` and native reads take the path lock on the canonical path; a queued writer is not skipped. Oversized `web_fetch` keeps the artifact directory in the fetch future across awaits, including on a multi-thread runtime. When `.git` has no readable `HEAD`, status and diff use the in-process scan. Receipt `scratch/check-change/runs/20261112T223813Z-14249/report.json`. SHA-256 `e9e5591ae1d644f1197eadef3e3a51d43f13fcfe76c2f281847526cec891cb76`. The package version was not bumped again. Recovery of the previous 0.1.112: `/Users/aji/.local/bin/.ca-release-stage.HjdtAR/ca.recovery-ORC258-20261112T224402Z-e498c891499efc3af76258efdc9ccbba46acda4ae9c0a588efa2a0ca11b40f93`. Earlier receipt `scratch/check-change/runs/20261112T220145Z-35005/report.json`. Full `verify.sh` was not run.

Folding replaces the rich tool cards for a fully foldable run. A repository with a readable `HEAD` still uses `git status` and `git diff`.

## Grok-gap wiring, installed 0.1.111

The transcript renderer folds a consecutive tool run (`Explored …`, `edited …`) and pins the current user prompt. Queue drain joins plain follow-ups with a blank line and leaves a `!` command queued. Undo of a drifted file restores agent lines through `perform_with_agent_hunks`. Bash commands go through `prepared_shell_command`. Search results from the file query become a Found card. Native read batches call `admit_batch`. Git status load asks `thread_cap(soft_nproc())`. Startup calls `announce_if_previous` before tokio. A mid-turn steer is wrapped by `steer_model_text`. Receipt `scratch/check-change/runs/20261112T210722Z-17126/report.json`. SHA-256 `8ef39776d9557b089dca12537b32f1c443832b81080560f52a1db915f7b3a341`. Recovery of 0.1.110: `/Users/aji/.local/bin/.ca-release-stage.TISzF4/ca.recovery-ORC258-20261112T211312Z-4318453ecb7e3aae68e26eda3b47e791f043648373750407c7959720bdfc17e6`. Focused log and install log are in the goal scratch directory. Full `verify.sh` was not run.

## Grok-gap slice, installed 0.1.110

Image tokens follow the provider tile rule. Auto-compact uses 85% of the window unless `compact_at_tokens` is set, retries an empty or unstructured summary under 500 characters three times, and can use `agent.compaction_model`. macOS worktrees clone with `clonefile` after `git worktree add --no-checkout`. `web_fetch` uses one client, refuses a private-network redirect, and caches a complete inline body. A missed edit that looks like a smart quote, em dash, ellipsis, or non-breaking space names the line. The crash flag is set before the async runtime.

Transcript folding, sticky prompt rows, the warm file index, queue merge, hunk undo, and the other new modules are tested and are not all called from the turn loop. `render.rs` does not draw folded tool rows. Full `verify.sh` was not run. Receipt `scratch/check-change/runs/20261112T203556Z-49234/report.json`. SHA-256 `eb440b1b46f40edcff6179a5ec0f1527075de77e6fd0d6b15be78174dfdfc05e`. Recovery of 0.1.109: `/Users/aji/.local/bin/.ca-release-stage.BN0y5B/ca.recovery-ORC258-20261112T204021Z-b5bbebaeaba4fea29b4030709705ef73605685eb3febf35c9090dbf794eb4b18`.

## Session shortcut

Ctrl+R opens the previous-session list, the same list as `/sessions`. `/diff` and `/changes` open the working-tree diff. Shift+R renames the highlighted saved session.

## Queued follow-ups

While a turn is running, Enter queues the composer draft. The selected row shows [Send now] [edit] [cancel]. Send now, Enter on an empty composer, and Ctrl+S stop the turn and send. Cancel deletes that queued message. Esc still stops the turn. Shift+Enter and Ctrl+J insert a newline. On macOS a plain Enter does that too while the Shift key is held.

## CHG-343 ChatGPT plan (SIWC)

`openai-chatgpt` is a chat shim for `https://api.openai.com/v1/responses`. It is
not Codex. Its request skips the provider filter. An expired plan token is
refreshed before the chat call. A plan usage limit stays on this provider.
Codex remains `openai-codex` on `chatgpt.com/backend-api/codex`. Live browser
sign-in has not been run. Receipt
`scratch/check-change/runs/20261112T173103Z-40529/report.json`. Plan:
[CHG-343](docs/exec-plans/completed/CHG-343-chatgpt-plan-siwc.md).

## ChatGPT consumer shim

Provider `chatgpt-shim` is the local bridge into ChatGPT chat. A send on that
route does not start an automatic task map and does not open the filter
preview. `openai-chatgpt` also skips that preview. While either route is
selected, the filter status row stays hidden. An automatic map already aimed at
`chatgpt-shim` is cancelled before the author prompt is sent. The wire request
keeps filenames and URLs as written, and it omits tools and tool-call history
so the bridge does not inject `TOOL_CALL` text. Other providers still plan,
filter, and call tools. Those agents can call `ask_chatgpt` with one question.
The shim receives only that question. Focused check
`scratch/check-change/runs/20261112T183757Z-95358/report.json`.

## Installed version

Installed CA is 0.1.109 at `/Users/aji/.local/bin/ca`. SHA-256
`b5bbebaeaba4fea29b4030709705ef73605685eb3febf35c9090dbf794eb4b18`.
`codesign -vvv` reported valid on disk. Recovery of 0.1.108:
`/Users/aji/.local/bin/.ca-release-stage.qI6Y86/ca.recovery-ORC258-20261112T184233Z-dd133f59c208e5e4afd62b061f0c936d8cd252f32d91622453dbaf080dc3c8b7`.
A new process loads 0.1.109. Focused check `scratch/check-change/runs/20261112T183757Z-95358/report.json`.
Previous install was 0.1.108, SHA-256
`dd133f59c208e5e4afd62b061f0c936d8cd252f32d91622453dbaf080dc3c8b7`.
`codesign -vvv` reported valid on disk. Recovery of 0.1.107:
`/Users/aji/.local/bin/.ca-release-stage.VC5FSB/ca.recovery-ORC258-20261112T182636Z-418908793aed127207bceaa8a7b9ada1ab42a888f664067e67fbe09c67949a84`.
A new process loads 0.1.108. Focused check `scratch/check-change/runs/20261112T182209Z-61035/report.json`.
Previous install was 0.1.107, SHA-256
`418908793aed127207bceaa8a7b9ada1ab42a888f664067e67fbe09c67949a84`.
`codesign -vvv` reported valid on disk. Recovery of 0.1.106:
`/Users/aji/.local/bin/.ca-release-stage.hrd40f/ca.recovery-ORC258-20261112T175902Z-18012aa59faab97d249aab55a91fa682c814d28448d0ed099d2e245a45adfe05`.
A new process loads 0.1.107. Focused check `scratch/check-change/runs/20261112T175053Z-83324/report.json`.
Previous install was 0.1.106, SHA-256
`18012aa59faab97d249aab55a91fa682c814d28448d0ed099d2e245a45adfe05`.
`codesign -vvv` reported valid on disk. Recovery of 0.1.105:
`/Users/aji/.local/bin/.ca-release-stage.IcWaN6/ca.recovery-ORC258-20261112T173601Z-fa5d1521f22a8465a78adde379265bea850902fb71dbee329c4739a665d98640`.
A new process loads 0.1.106. Focused check `scratch/check-change/runs/20261112T173103Z-40529/report.json`.
Previous install was 0.1.105, SHA-256
`fa5d1521f22a8465a78adde379265bea850902fb71dbee329c4739a665d98640`.
`codesign -vvv` reported valid on disk. Recovery of 0.1.104:
`/Users/aji/.local/bin/.ca-release-stage.btBMwP/ca.recovery-ORC258-20261112T171140Z-1dd61d776c42f6a2f182ca9744c43854b47a2c6197132f3a30ebb4735263fa10`.
A new process loads 0.1.105. Focused check `scratch/check-change/runs/20261112T170533Z-84825/report.json`.
Previous install was 0.1.104, SHA-256
`1dd61d776c42f6a2f182ca9744c43854b47a2c6197132f3a30ebb4735263fa10`.
`codesign -vvv` reported valid on disk. Recovery of 0.1.103:
`/Users/aji/.local/bin/.ca-release-stage.miox4a/ca.recovery-ORC258-20261112T165302Z-ce74273a783200cf482720d5e76a89c0b3c6c3a8ada8fd01adc215b102878fdb`.
A new process loads 0.1.104. Focused check `scratch/check-change/runs/20261112T164410Z-28627/report.json`.
Previous install was 0.1.103, SHA-256
`ce74273a783200cf482720d5e76a89c0b3c6c3a8ada8fd01adc215b102878fdb`.
`codesign -vvv` reported valid on disk. Recovery of 0.1.102:
`/Users/aji/.local/bin/.ca-release-stage.lUVwiw/ca.recovery-ORC258-20261112T161207Z-042384eb6b2d02c7cb2f7799e85666578af29c6d9eb07a63183162cec8f10843`.
A new process loads 0.1.103. Focused check `scratch/check-change/runs/20261112T160134Z-12891/report.json`.
Previous install was 0.1.102, SHA-256
`2ab0f6b54a82fdc9087980493a7cbe67d4b9aeb69f2b629939c6703465cf2d9f`.
`codesign -vvv` reported valid on disk. Recovery of 0.1.101:
`/Users/aji/.local/bin/.ca-release-stage.LVK9Cw/ca.recovery-ORC258-20261110T230054Z-e55ba9f84e2e5ec3faa33c88343ac8cc2d46cbdc2743ef6afae857bfcaef3dd0`.
An automatic Dream whose supervised queue is already full is recorded as a skip and does not replace the status line. Eight unacknowledged intents from 2026-09-24 through 2026-10-23 remain queued. Focused check `scratch/check-change/runs/20261110T225627Z-85151/report.json`.
Previous 0.1.101 SHA-256 was
`bafec028e5b65237bb145d71b4975afb6396a6cdfc5f81067d8f01dc53b29ee5`. Recovery of that build:
`/Users/aji/.local/bin/.ca-release-stage.YNXuRs/ca.recovery-ORC258-20261110T224842Z-636528775baf1f4f322a0789110f17dcdfb35479fc2a0759c8ee5936fdd18260`.
A finished task map uses the restored summary, then lists each provider alias token. It no longer shows one raw progress line and a different markdown row. Focused check `scratch/check-change/runs/20261110T224339Z-57046/report.json`. A new process loads 0.1.101. Previous recorded 0.1.100 candidate SHA-256 was
`4fec43c00d271ed736f418ae231a4373d167b681cf6291467368f581a1b46307`. Recovery of 0.1.99:
`/Users/aji/.local/bin/.ca-release-stage.UCPDXp/ca.recovery-ORC258-20261110T223626Z-bae74cc8d904bdf07784a06ccd820bb112639dedad39165408f635e9419bd128`.
Previous install before that was 0.1.99, SHA-256
`bae74cc8d904bdf07784a06ccd820bb112639dedad39165408f635e9419bd128`.
The single signed release passed the complete workspace gate and real DeepSeek
acceptance. Installed signature, ownership/mode, version/help and exact binary
identity pass. Recovery of 0.1.98:
`/Users/aji/.local/bin/.ca-release-stage.7NmbDL/ca.recovery-ORC258-20261110T020743Z-1876595a1750676b08aa8bedb821a762beb0c2e354d6c63052d8b3c7857177b6`.
Installation: `scratch/BUG-341/installation.json`, `scratch/BUG-341/install.log`
(job 2987). A new process loads 0.1.99; existing sessions were left alone.

## FLT-342 filter proof

Filename mode now says `filenames on · N selected values` on the privacy
status line. A completed task keeps the unrestored assistant text on
`provider_summary`. When the TO PROVIDER pane is not open, the task map shows
one underlined `Provider listing:` alias. The pre-Send preview adds local
basename → alias rows for a dotfile and an empty file, and those rows stay out
of the provider copy. Successful `list_dir` and `glob` text is projected before
the next provider message and the outcome excerpt. Filename mode adds one fixed
child instruction, and filename restore returns one span per alias. Focused
check passed: `scratch/check-change/runs/20261110T222806Z-12882/report.json`.
Plan: [FLT-342](docs/exec-plans/completed/FLT-342-filter-proof.md).

## Latest repair: BUG-341

Both `testing filter. list ~/projects` and
`testing filter. list ~/projects/filtertest` complete through normal CA with the
real saved DeepSeek-v4-pro route. The signed candidate child run and installed
root run each use one Send, zero pre-Send traffic and no manual controls, then
plan and execute automatically. The root final listing contains 15 current
entries; the child has 2. `/filter` works in wide/narrow layouts without extra
requests. Preview coverage gaps remain explicit.

Closed host instructions, exact home references, native reasoning continuation,
failed listing history, task-issued URL authorities, literal-reference glob
queries and authenticated repeated-listing notes are repaired. Full default
and all-feature matrices, strict lint, formatting and runtime evaluation pass.
Earlier failed/interrupted attempts remain retained as nonpassing. Evidence:
`scratch/BUG-341/qualification.json`; plan:
[BUG-341](docs/exec-plans/completed/BUG-341-real-deepseek-filter-listing.md).
Production settings, credentials and sessions retain their metadata; only
recorded background-status/automation metadata changed. Native runs use
disposable application state and deny writes to production CA state/workspace.

## What the tree now does

- Filter preview works from the multi-million-entry projects volume. It lists
  root names, scans at most 25,000 descendant entries and discloses inaccessible
  folders or bounded coverage. Unscanned bare names may remain readable. Exact
  encountered paths retain masking/restoration; historical aliases stay stable.
- Automatic planning retains the exact host task-map schema even when discovered
  filenames collide with schema metadata. Caller schemas, returned values,
  execution preflight and explicit stop rules retain their checks.
- Composer path completion includes valid linked folders/files, classifies them
  by their targets and preserves the entered path. It still lists one directory
  on the existing background worker and skips broken or looping links.
- Every new interactive task opens a local filter preview before its first
  planner/provider request. Private values is the default; the second preset
  adds editable suggestions. Send and run saves the task policy, then plans and
  executes automatically with existing action permissions. Escape restores the
  draft. The review starts off. `/filter off` and `/filter on` save that choice
  in `[ui] filter_preview` and do not clear an already armed stop filter. The
  same switch is the Filter row in `/config`. Explicit
  Plan mode still requires plan review.
- Accepted filtering belongs to the task, including attachments and historical
  aliases. Steering and workers reuse it; a later task cannot change it.
  Remember for this project is explicit. `/filter` inspects the current task,
  with Files, Rules and Settings retained under Advanced.
- Graph progress saves share the privacy receipt commit ordering and persist
  terminal state before publishing completion, preventing stale session saves
  from leaving completed work recorded as running.
- Child admission creates an `AdmittedExecutionGrant`. Typed reads honor path
  scopes. Worktree preparation rebases only scopes under the source root.
- Admitted child commands use a sealed environment and do not receive `/` as a
  read-only sandbox root. A network allow for that child does not add the host
  Keychain, SSH agent, or git and `gh` credential paths. Ordinary jobs still do.
- A child proxy binds loopback, forwards one admitted HTTP request to the
  vetted address, and closes that route on revocation. The Seatbelt profile
  can allow only that localhost TCP port. `http_request`, `git_operation`,
  and `ssh_operation` pin their arguments and reject host credentials.
- On macOS, an absent `agent.child_execution` uses the confined worker. Explicit
  isolated launches `sandbox-exec` before the worker. A bad identity or a missing
  capability runs no command and does not fall back in-process. Linux `nono`
  0.81.0 at `e9796cca01eb863a9a83e96c18ae6120aa50beb1` is pinned for Linux only.
  Linux and FreeBSD absent settings stay in-process.
- A new cycle cannot decide, activate, promote, or enter guidance until
  confirmation succeeds. A one-hint removal is adopted only when exposure shows
  the baseline received the hint and the candidate did not.
- New skills from `ca skill new` include when-to-use, prerequisites, reference,
  command, and completion guidance. An installed skill test runs after the
  source checkout is removed. Missing prerequisites are reported and not installed.
- Provider catalog snapshots cover every `provider_templates::ALL` entry after
  adapter preparation. Synthetic credentials are not recorded.
- Destination admission rejects the wrong protocol, port, address class,
  generation, and another worker's reference. It connects to the vetted address.
  A changed address or a non-HTTP socket family fails before that connect.
- A restricted read holds the admission lock until it finishes, so a concurrent
  unapproved send fails. A policy-generation change closes the live proxy,
  cancels the owned job and worker, and blocks the old grant until cleanup is
  observed. After that cleanup, the old generation still cannot spawn. Cache
  headroom follows the active route for a switch, a fallback,
  and a restoration. A stale generation or owner does not activate a checkpoint.
- `--internal-isolated-worker` is recognized before Tokio. Without an inherited
  launch spec it exits 2 and starts no tool.
- Confirmation is a separate phase from `HarnessPlan::validate`. Hint edits
  allow one addition or one removal, and missing exposure blocks a match.
- Security journal records use Prepared, Running, Confirmed, and Unknown.
  Skill outcomes store `SkillLoad.content_digest`.

## Known gaps

The model loop stays on the host. Child `run_command` is the confined worker
path. Ordinary parent commands are not. Linux and FreeBSD isolated-by-default
are externally unverified. No real-provider or real-model benefit is claimed.
FLT-334 and earlier failed comparisons stay unqualified.
A default `/var/folders` temp path is a symlink and is not a valid private-storage
root; gates set `TMPDIR` under `/private/var/tmp`.

## Completed task

BUG-340 is installed as 0.1.98. Inaccessible volume-system folders and unbounded
filename discovery caused the reported preview failure; exact native schema
binding also fixes the discovered post-Send planning blocker. The stable final
source passed 116 focused all-feature tests, formatting, strict bin lint, release
and independent combined review. Candidate and installed actual-volume terminal
runs pass both modes, zero requests before Send, one planner, native listing,
protected result paths, exact workspace/grants and authoritative saved completion.
Previews take 454–491 ms in this fixture. Wide/narrow coverage and pending refresh
are visible. Only live dashboard-presence metadata changed among 4,459 protected
entries; saved settings/sessions were unchanged. All failed attempts remain in
the completed [BUG-340 plan](docs/exec-plans/completed/BUG-340-filter-discovery-volume-root.md).
No new full workspace gate or real external-provider run is claimed.

## Previous completed task

BUG-339 repaired the reported `~/projects` zero-match popup. The user's lowercase
`~/projects` is a link to `/Volumes/projects`; completion had explicitly excluded
every symlink. The source change is local to `src/tui/composer.rs`. Baseline
0.1.96 reproduced zero matches in wide/narrow terminals. Ten focused tests,
formatting, strict lint and independent review passed; the single signed 0.1.97
candidate and installed binary passed the exact draft, Tab completion, child
listing, linked-file and broken-link checks with zero provider requests. Saved
configuration/session metadata stayed unchanged; only the existing live
dashboard-presence directory and one presence record changed. Evidence:
`scratch/BUG-339/installation.json`. No full workspace rerun was needed for this
bounded completion-only change; FLT-338's broader evidence remains below.

## Earlier completed task

FLT-338 is installed and qualified. Plan:
[FLT-338](docs/exec-plans/completed/FLT-338-automatic-filter-preview.md).
Full workspace Rust checks and the single release build passed. The original
full-gate runtime failure remains recorded; its legacy raw-filename fixture was
corrected and all nine runtime cases plus independent evaluation passed on the
same binary. Reused stages and fresh evidence are linked in qualification.json.
Candidate and installed terminal acceptance passed with 196 history messages,
two automatic tasks and two visibly fresh pending updates in wide/narrow views.
All 4,459 protected config/session metadata entries remained unchanged. Real
external providers and ignored opt-in tests were not exercised.

## Prior installed task

ISO-337 was installed as 0.1.95 and remains included. A confirmed policy generation still rejects a
nested spawn of the old grant. Plan:
[docs/exec-plans/completed/ISO-337-execution-isolation.md](docs/exec-plans/completed/ISO-337-execution-isolation.md).
