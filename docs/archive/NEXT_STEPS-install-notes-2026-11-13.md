# Archived next-step install notes

Moved from `NEXT_STEPS.md` on 2026-11-13.

Plans moved to `docs/exec-plans/completed/` in that pass:

- `ISO-337-execution-isolation.md`
- `FLT-342-filter-proof.md`
- `CHG-343-chatgpt-plan-siwc.md`
- `FLT-328-desktop-host-runtime-integration.md`
- `FLT-326-same-client-cyber-comparison.md`
- `FLT-327-native-startup-repair-and-faithful-replay.md`
- `LRN-170-one-shot-learning-pass.md`

# Next steps

## Queue rules

Small local work uses the task template in this file. Risky, multi-file, protocol, event-loop, approval, session-format, or parity work gets an ExecPlan under `docs/exec-plans/active/` that follows `.agent/PLANS.md`. Link the plan here. The plan holds the steps.

Paragraphs under "Current task" and "Completed task" are recent install notes. Read `CONTINUITY.md` for the installed version before editing.

## DOC-001: Split completed-plan narration out of the knowledge index

Status: Ready
Priority: High

Allowed files:
- `docs/.INDEX.md`
- `docs/archive/INDEX-completed-plans.md` (create)

Forbidden files:
- `src/`, `crates/`, `oracles/`, `reference/`, `Cargo.toml`
- Bodies under `docs/exec-plans/`

Context:
- `docs/.INDEX.md` is the navigation map and also a long log of completed plans. `CONTINUITY.md` records this as the open harness gap after the 2026-11-13 docs pass.

Reference parity:
- Reference: N/A
- Match: N/A
- Do not match/copy: N/A
- Verification method: link check by reading the edited index

Instructions:
1. List headings in `docs/.INDEX.md` and mark each bullet as `exec-plans/active` or `exec-plans/completed`.
2. Move completed-plan narration into `docs/archive/INDEX-completed-plans.md`, preserving the links.
3. Leave Read First, the user-doc list, the oracle and reference map, conflict resolution, and every link to `docs/exec-plans/active/` in `docs/.INDEX.md`.
4. Add one archive link. Stop if an active plan would lose its only index link.

Acceptance criteria:
- `docs/.INDEX.md` still links `AGENTS.md`, `CONTINUITY.md`, `ARCHITECTURE.md`, `PARITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `.agent/PLANS.md`, `docs/QUALITY.md`, and `REVIEW_CHECKLIST.md`.
- Every `exec-plans/active/` link that was in the index remains there.
- Completed-plan paragraphs live in the archive file.

Tests:
- Manual: read both files and confirm the active-plan links. No Cargo command applies.

Discovery budget:
- `docs/.INDEX.md` and `ls docs/archive`. Stop after one pass over the index bullets.

Stop conditions:
- Stop if an active plan has no remaining link.
- Stop if the edit would change an ExecPlan body or source file.

## GRD-010: Check new ExecPlans for required sections

Status: Proposed
Priority: Medium

Allowed files:
- `scripts/check-exec-plan.py` (create)
- `docs/QUALITY.md` (status line for GRD-010 only)
- `TESTING.md` (one command row)

Forbidden files:
- Product source under `src/` and `crates/`
- Existing files in `docs/exec-plans/`

Context:
- `docs/QUALITY.md` GRD-010 asks that a new active plan contain Goal, Acceptance Criteria or Validation, and Recovery and Rollback. No script does that today.

Reference parity:
- Reference: N/A
- Match: the section names in `.agent/PLANS.md`
- Do not match/copy: `crates/codex-team/.agent/PLANS.md` headings
- Verification method: run the new script on a fixture plan and on one known-good plan such as `docs/exec-plans/active/SEC-158-privilege-supervisor-and-service-v2.md`

Instructions:
1. Read `.agent/PLANS.md` and the GRD-010 entry.
2. Add a script that reads one plan path and exits non-zero with `active ExecPlan missing required section: <name>` when a required heading is absent.
3. Point `TESTING.md` and GRD-010 at that command. Leave existing plans unchanged.

Acceptance criteria:
- A plan missing `## Recovery and Rollback` fails with that message.
- `docs/exec-plans/active/SEC-158-privilege-supervisor-and-service-v2.md` passes.

Tests:
- `python3 scripts/check-exec-plan.py docs/exec-plans/active/SEC-158-privilege-supervisor-and-service-v2.md`
- One temporary fixture with a section removed, then removed after the run.

Discovery budget:
- `.agent/PLANS.md`, `docs/QUALITY.md` GRD-010, and the SEC-158 headings. Stop if SEC-158 no longer has those headings.

Stop conditions:
- Stop if the check would need to rewrite historical plans to pass.
- Stop if the script would import product crates.

## Current task

The conversation screen drops the title row. Identity and keys are two rows under the composer. Tab selects a transcript block, Left and Right fold it, Enter opens it, and Alt+Q quotes it. Questions and approvals pin above the composer. The Terminal theme does not fill the background. Installed ca 0.1.125, readback SHA-256 `9052baf9c6f99ce4cc239338d90d30328901188c34264a07f8fb267d8fe37db3`. The installer printed candidate `ca7ebfb29a211ace60ac865644de0944de36ac29c5f8bcba8e7064ec42599c7e`. Recovery of 0.1.124: `/Users/aji/.local/bin/.ca-release-stage.3WhwG5/ca.recovery-ORC258-20261113T194349Z-7b21f1570ea0932d021032fd668ff2669065318b7765a354b013633f095102c4`. Focused tests and fmt passed in `scratch/check-change/runs/20261113T193412Z-10686/report.json`. Clippy passed after replacing a hand-rolled clamp. Full `verify.sh` was not run.

`browser_exec` is a Rust browser session. A leading `#` comment is the step label. Helpers cover navigation, controls, HTML, clicks by selector or visible text, fill, scroll, keys, page JavaScript, coordinates, and an attached screenshot. `~/.config/ca/browser.toml` `cdp_url` uses that endpoint. `browser_vault` lists and fills logins without returning the password. Installed ca 0.1.124, readback SHA-256 `7b21f1570ea0932d021032fd668ff2669065318b7765a354b013633f095102c4`. The installer verified candidate `bce72797bdb396a933551ac04cc169ada86c466d4c868813de20d4f469c43ecc`. Recovery of 0.1.123: `/Users/aji/.local/bin/.ca-release-stage.1w6X9R/ca.recovery-ORC258-20261113T162313Z-6b50e51cf1584ceed23548e3a234222ccbfda15c57c5dc74e75cd5318ea01d19`. Focused check `scratch/check-change/runs/20261113T161704Z-71364/report.json` passed. Full `verify.sh` was not run.

The `browser` tool is available on a normal main session. It can list controls and form options, read one element's HTML, scroll, and press a key, as well as navigate, snapshot, click, type, screenshot, download, and close. Bare, scheduled, and role runs leave it off. Installed ca 0.1.123, readback SHA-256 `6b50e51cf1584ceed23548e3a234222ccbfda15c57c5dc74e75cd5318ea01d19`. The installer verified candidate `4fbef53530df8093045a39c0415a847faed03b585b7b04345c4aa7816490fbe3`. Recovery of 0.1.122: `/Users/aji/.local/bin/.ca-release-stage.1jA9OR/ca.recovery-ORC258-20261113T155830Z-149473241c9792039375e72c8674191fa4fb671388c12bcd748478e7deeec47f`. Focused check `scratch/check-change/runs/20261113T155207Z-17634/report.json` passed. Full `verify.sh` was not run.

`/config` → Personality edits the Personality and Pre-prompt sections. The seed is the Hermes technical voice without the Hermes or Nous identity, plus the short technical pre-prompt. Empty headings stay empty. Bare mode cannot save. The local chatgpt-shim returns upstream failures as HTTP errors, drives a private ChatGPT.app on `127.0.0.1:9335` with its own profile, and falls back to Google Chrome `--headless=new` if that app does not open the port. The private window's browser sign-in returns to a localhost callback that exists only while that screen is waiting. A live `gpt-5.6` completion through the private app returned `pong` on 2026-11-13. ca fails a turn that still contains shim error text and retries one HTTP 401. Installed ca 0.1.122, readback SHA-256 `149473241c9792039375e72c8674191fa4fb671388c12bcd748478e7deeec47f`. The installer verified candidate `67750d1184ad61ed3dcd771f80149d6f4ce34430701623d5eb3a03ac9600cfa8`. Recovery of 0.1.121: `/Users/aji/.local/bin/.ca-release-stage.JVfGxm/ca.recovery-ORC258-20261113T145911Z-8e4d61188dcca05ed98fe42743639731ad85563c3c7d7f7c2ec4f0887c0450f7`. Focused check `scratch/check-change/runs/20261113T145251Z-89687/report.json` passed. Full `verify.sh` was not run. The shim was restarted and `GET /v1/models` returned 200. A live conversation was not sent.

`/config` includes the missing appearance and agent preferences, and opens the existing screens for sync theme, filter rules, prompt cache, presets, memory, plugins, rules, commands, workspaces, and direnv. Numeric agent limits stay in the config file. Installed ca 0.1.121, readback SHA-256 `8e4d61188dcca05ed98fe42743639731ad85563c3c7d7f7c2ec4f0887c0450f7`. The installer verified candidate `3ca992159a8f55a3c05505cc79ea6499c01b53bb6b772e0b7e93c67e7a9d1b55`. Recovery of 0.1.120: `/Users/aji/.local/bin/.ca-release-stage.r5jBIt/ca.recovery-ORC258-20261113T141757Z-d92d53fc571436e9ab1eee5101d871b9d231e953c95e572a339878b563346bc7`. Focused check `scratch/check-change/runs/20261113T141124Z-99219/report.json` passed. Full `verify.sh` was not run.

The pre-send filter review starts off. `/config` has a Filter row, and `/filter on|off` saves the same `[ui] filter_preview` choice without clearing an armed stop filter. Installed ca 0.1.120, readback SHA-256 `d92d53fc571436e9ab1eee5101d871b9d231e953c95e572a339878b563346bc7`. The installer verified candidate `243c88762bb08c18172d066914f85a0b163abecb75d2d99c1c2978e682ccdbff`. Recovery of 0.1.119: `/Users/aji/.local/bin/.ca-release-stage.O1SbZc/ca.recovery-ORC258-20261113T134437Z-f39f083a33e303e31cacf5df7ce8df5f8bac6a0c99611a0f3474ab489ab5603b`. Focused check `scratch/check-change/runs/20261113T133848Z-28560/report.json` passed. Full `verify.sh` was not run.

Learning and recall enroll `~/.grok/sessions` (or `$GROK_HOME/sessions` when `GROK_HOME` is an absolute directory). A missing tree stays unenrolled. Only user and assistant text is kept. Installed ca 0.1.119, readback SHA-256 `f39f083a33e303e31cacf5df7ce8df5f8bac6a0c99611a0f3474ab489ab5603b`. The installer verified candidate `d8044c711efffd105d4c3e2c2143c5174c34cd9bf6bfb98e3651acf46e9cfc37`. Recovery of 0.1.118: `/Users/aji/.local/bin/.ca-release-stage.CeJS0D/ca.recovery-ORC258-20261113T125835Z-b6ec9a26872176ec78bc015752fc5214ea32ed263de1397a80b89231ea223b85`. Focused check `scratch/check-change/runs/20261113T125333Z-19718/report.json` passed. Full `verify.sh` was not run.

`/config` (F2) lists Theme, Compact mode, Status indicators, Status line, Keymap, and Permissions. Enter follows the highlighted row. Compact mode and status indicators follow the live session value. Installed ca 0.1.118, readback SHA-256 `b6ec9a26872176ec78bc015752fc5214ea32ed263de1397a80b89231ea223b85`. Recovery of 0.1.117: `/Users/aji/.local/bin/.ca-release-stage.7GyxAV/ca.recovery-ORC258-20261113T104231Z-e20f4744b621962bc91c88b4767292e490101ab6caf6104447224b41a5c75ae0`. Full `verify.sh` was not run.

`/config` (F2) is the settings sheet. Image pastes are `[Image #N]` chips. Installed ca 0.1.116, readback SHA-256 `cd5e2bb65bf61deaa31afb0c7caa2d71603629282396e183b2436671e4657ea0`. Recovery: `/Users/aji/.local/bin/.ca-release-stage.m2If8s/ca.recovery-ORC258-20261113T095132Z-797534e57f02d7a0651bc6049158a7c1b92b62e863f703d50e8ce423f6fb8091`. Focused check `scratch/check-change/runs/20261113T094448Z-63281/report.json` passed. Full `verify.sh` was not run.



`/filter off` skips the pre-send review for this ca. `/filter on` brings it back and does not arm the stop filter. The review screen and the top status row say the same thing. A stop filter that is already armed stays until restart. Installed ca 0.1.115, SHA-256 `42c28d289d171616bb169c4112f56dddb13f55982b11f04651342a5282d77ab3`. Recovery of the previous binary: `/Users/aji/.local/bin/.ca-release-stage.o9MiL7/ca.recovery-ORC258-20261113T022351Z-0e5e734ac46d708178b5554a6667c497340d2c5ebc7e384c8fb834075309236c`. Focused check `scratch/check-change/runs/20261113T021836Z-4903/report.json` passed. Full `verify.sh` was not run.

ca 0.1.112 receipt `scratch/check-change/runs/20261112T223813Z-14249/report.json`. The pinned prompt slides off when the next user card reaches it, `write_file` and native reads share the path lock, and oversized fetch keeps its artifact directory across a multi-thread await. Full `verify.sh` was not run. Minimal scrollback and the tree-sitter index stay deferred.

CHG-343 ChatGPT plan (SIWC) focused TDD is green including loopback
authorize and `connect_chatgpt_plan`. The local `chatgpt-shim` route stays a
conversation: no automatic task map, no filename aliases, no tool-call
protocol, and no filter preview. Other agents can call `ask_chatgpt` for one
answer from that shim. Remaining: live SIWC in a disposable
CA_HOME (user present). Plan:
[CHG-343](docs/exec-plans/completed/CHG-343-chatgpt-plan-siwc.md). Receipt:
`scratch/check-change/runs/20261112T183757Z-95358/report.json`.

## Completed task

FLT-342 is installed as ca 0.1.102. An automatic Dream with a full supervised queue is a quiet skip. A finished task lists each provider alias, and the task row uses the restored summary. The filename filter is checkable from the
status line, the saved provider spelling, a narrow-terminal listing row, and
the pre-Send preview. Plan:
[FLT-342](docs/exec-plans/completed/FLT-342-filter-proof.md). Focused check
`scratch/check-change/runs/20261110T222806Z-12882/report.json` passed. Full
`verify.sh` was not run.

## Completed task

BUG-341 is installed as ca 0.1.99. The exact root and child-folder filter tasks
pass through normal CA with real DeepSeek, one Send and automatic execution.
Full workspace gate, independent combined/native reviews, one signed release,
recoverable installation and installed-entry readback pass. Evidence:
`scratch/BUG-341/qualification.json`; plan:
[BUG-341](docs/exec-plans/completed/BUG-341-real-deepseek-filter-listing.md).
No remaining BUG-341 task is queued.

## Previous completed task

BUG-340 is installed as ca 0.1.98. Filter preview now handles inaccessible folders
and bounded discovery from `/Volumes/projects`; exact host-schema binding keeps
automatic planning usable when local filenames match schema metadata. Both modes
pass the actual-volume listing workflow with one Send, protected native paths and
saved completion. Focused checks, review, signed installation and installed
readback are complete. Evidence: `scratch/BUG-340/qualification.json`. Plan:
[BUG-340](docs/exec-plans/completed/BUG-340-filter-discovery-volume-root.md).

## Previous completed task

BUG-339 is installed as ca 0.1.97. `~/projects` now completes through its link to
`/Volumes/projects`, preserving the entered path and listing the folder's
children. Valid file links also complete; broken/looping links are omitted.
Ten focused checks, formatting, strict lint, independent review, one release
build and candidate/installed normal-composer acceptance passed. Evidence:
`scratch/BUG-339/installation.json`. No remaining BUG-339 task is queued.

## Earlier completed task

FLT-338 automatic filter preview is installed as ca 0.1.96. Plan:
[FLT-338](docs/exec-plans/completed/FLT-338-automatic-filter-preview.md).
Combined review, focused and full workspace checks, corrected runtime evaluation,
recoverable signed installation and installed-entry acceptance are complete.
Evidence: `scratch/FLT-338/qualification.json`. No remaining FLT-338 task is queued.

## Prior installed task

ISO-337 was installed as ca 0.1.95 and remains included. A confirmed policy generation rejects a
nested spawn of the old grant. A restricted read holds admission, revocation
closes the live proxy and owned work, and the vetted proxy rejects a changed
address and another socket family. Cache headroom follows the active route. Plan:
[docs/exec-plans/completed/ISO-337-execution-isolation.md](docs/exec-plans/completed/ISO-337-execution-isolation.md).

Do not enable isolated-by-default on FreeBSD or Linux until those platforms
have their own native qualification. Do not claim a real-provider or real-model
benefit. FLT-334 stays unqualified.

Earlier task history is archived at
[docs/archive/NEXT_STEPS-before-iso337-summary.md](docs/archive/NEXT_STEPS-before-iso337-summary.md).
FLT-336 is the recovered 0.1.88 baseline. FLT-334 and earlier failed comparisons
stay unqualified.
