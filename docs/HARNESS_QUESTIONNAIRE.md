# CA behavior questionnaire and personality steering

The interactive questionnaire lets you choose how CA works, then saves those
preferences as permanent personality steering. A separate continuous audit
learns which behavior questions need attention from recorded outcomes. Both
cover 48 questions across 12 areas.

## Set your preferences interactively

Inside CA, enter `/personality` (or `/questionnaire`). From a terminal, run:

```sh
ca personality
```

No provider or model is needed for setup. The opening menu offers:

- **Quick setup:** eight important questions to get started.
- **Full walkthrough:** all 48 preference questions.
- **Explore by section:** choose any of the 12 behavior areas.
- **Review and save:** inspect the exact instructions that will steer CA.
- **Additional instructions:** write anything the choices do not cover.

Quick setup ends after eight questions and goes directly to Review and save.
The other 40 questions are optional. You can also save after any number of
answers: press Ctrl+S on a question to open review, then Enter to save. Reopen
Quick setup later to resume at the first unanswered question. Its progress
counter covers the eight quick questions, not the full walkthrough.

The preference sections cover voice and personality, detail and explanation,
initiative and completion, understanding and clarification, reasoning and
judgment, tools and delegation, evidence and verification, failure and
recovery, memory and learning, efficiency and budgets, data and permissions,
and repository and long-running work.

Each question offers three explained choices, a custom answer, and an option
to leave it unanswered. Use arrows or the displayed number keys to select,
then Enter to continue. Left or Backspace returns to the previous question;
Esc returns to the menu. Saved answers remain selected when you reopen the
questionnaire, and a walkthrough resumes at the first unanswered question.

Custom answers and additional instructions support multiline text, Unicode,
and paste. Ctrl+S accepts text and returns to the questionnaire. Outside a text
editor, Ctrl+S opens the review. Scroll the review with arrows or Page Up/Down,
then press Enter to save. Closing through the menu or Esc with unsaved changes
offers keep-editing or discard choices. Before Save, Ctrl+C closes immediately
and discards the unsaved draft. An in-progress save completes. The draft is
saved only when you select Save in the review.

Save generates and publishes the personality instructions automatically;
there is no file to edit manually. The next ordinary turn in CA or a new
headless request uses your saved preferences. A turn already in progress and
its retries retain their original steering. The interface stays open after a
successful save so you can review or make another adjustment.

Preferences belong to the selected CA profile. For example:

```sh
ca personality --profile work
ca harness questionnaire --profile work steering
```

`steering` is a read-only view of the saved active instructions. The equivalent
setup command is `ca harness questionnaire setup`. Running
`ca harness questionnaire` directly in a terminal also opens setup; piped or
JSON invocation retains the audit-question listing. Bare mode cannot persist
preferences.

Existing authored instructions are preserved in Additional instructions.
Saving preserves a private recovery copy of the previous file. If another
process changes the preferences while you answer, Save reports the conflict
and leaves that process's changes intact; reopen to load the new revision.
Unanswered questions add no instructions. The underlying `personality.md`
remains readable Markdown with hidden structured answers for reopening.

Your selections are human steering. Outcome statistics and model reflections
do not rewrite your choices into new human rules. Existing instruction
priority, explicit task constraints, and tool grants still govern actions.
An improvement discovered by the audit remains an investigation or candidate
until the applicable evaluation and activation path admits it.

## Inspect and run the audit

```sh
ca harness questionnaire show
ca harness questionnaire run
ca harness questionnaire status
ca harness questionnaire watch --interval 300 --limit 200
```

`show` prints every stable question ID, risk weight, evidence requirement, and
improvement objective. `run` reads recorded session outcomes, updates the
questionnaire's durable state, and reports the current priorities. `status`
shows the retained report. `watch` repeats the same pass until stopped, so it
can remain running while new outcomes arrive. The audit works from existing
recorded evidence; the count of scheduled passes is not a count of new tasks or
successful learning experiments.

Use the same CA profile for the personality file, audit, and conversations you
want to steer. The commands' `--help` output gives the available output and
watch controls. Stop a watch with Ctrl+C; the retained audit state can be
inspected and another watch can be started later.

The watch interval is in seconds; the default is 300. `--limit` bounds the
recent-outcome input for each pass (default 200, maximum 1,000). For a bounded
trial, add `--cycles 5`. `--json` selects structured output, and a watch emits
its first report and subsequent changed reports. An unchanged watch stays
quiet. `status` has no saved report until the first run or watch pass.

Each local scan separately caps discovery at 4,096 directory entries, 64 MiB
of input and 2 MiB per outcome file. Invalid or incomplete records remain
excluded; reaching a scan bound reports an error. These bounds are distinct
from the number of recent eligible outcomes retained in the learning window.

For example, keep a work profile's steering and audit together:

```sh
ca personality --profile work
ca harness questionnaire --profile work steering
ca harness questionnaire --profile work watch --interval 300
```

Bare mode can display the static questions, but commands that use persistent
profile state require ordinary profile mode.

## What the audit questions cover

Stable IDs remain usable when wording changes. Each domain has four questions.
Weights run from 1 to 5 and express investigation impact; they are not
model-generated grades.

| Domain | IDs | Observable behavior |
| --- | --- | --- |
| Intent | `CAQ-INT-01`–`04` | Deliver the requested outcome, incorporate corrections, resolve routine ambiguity, ground permanent preferences in user words. |
| Autonomy | `CAQ-AUT-01`–`04` | Complete reviewable work, reuse granted authority, recover with a changed approach, report the actual completion state. |
| Instructions | `CAQ-INS-01`–`04` | Preserve objectives through continuation, respect instruction scope, retain compatible work under steering, honor cancellation. |
| Tools | `CAQ-TOL-01`–`04` | Use supported tools and schemas, inspect results, preserve restrictions during fallback, bind tool calls to their results. |
| Verification | `CAQ-VER-01`–`04` | Reproduce failures, test the real user path, support claims with independent evidence, preserve validation boundaries. |
| Recovery | `CAQ-REC-01`–`04` | Classify retries, reconcile uncertain effects, clean up owned workers, retain work and accounting across restart. |
| Learning | `CAQ-LRN-01`–`04` | Derive reusable hypotheses, verify candidates independently, activate the evaluated change, invalidate or roll back regressions. |
| Memory | `CAQ-MEM-01`–`04` | Keep provenance and authority, refresh stale recall, isolate workspace assumptions, honor durable correction and removal. |
| Efficiency | `CAQ-EFF-01`–`04` | Read bounded evidence, batch independent work, control loops and persistent budgets, keep foreground input responsive. |
| Security | `CAQ-SEC-01`–`04` | Respect instruction provenance, preserve secrets, enforce exact grants, retain data and recovery paths. |
| Communication | `CAQ-COM-01`–`04` | Explain outcomes clearly, provide useful updates, ask consequential questions, report failures and limits accurately. |
| Repository | `CAQ-REP-01`–`04` | Preserve existing edits, make causal focused changes, run applicable checks, distinguish source from installed behavior. |

## How continuous learning works

The audit associates each question with a conservative outcome signal such as
tool failures, failed verification, rejected feedback, corrections, undo,
loop-guard trips, denials, interruption, or recovery. Some questions have no
reliable signal in current outcome telemetry and remain explicitly unknown.
Each question also states the evidence needed to establish its answer and an
objective for investigating a possible improvement.

Evidence is grouped by the effective behavior cohort, including relevant
provider/model route and harness identity. A new build or changed effective
behavior is not silently compared as though it were the previous cohort.
The first pass after a new build can therefore have no eligible evidence;
new independent tasks supply that build's baseline.
Repeated scans and related continuations do not create new independent
evidence: task roots are deduplicated. This keeps a long session or a fast
watch interval from overwhelming several independent tasks.

As new independent outcomes arrive, retained counts and trends change the
investigation priorities. More consequential recurring signals receive more
attention. Analyze receives all 48 questions, each with its required evidence
and concrete improvement objective, within a 32 KiB questionnaire bound.
Ordinary TUI turns and headless requests receive up to six high-priority
questions from the latest matching workspace, provider, model, and build
cohort. That focus supplies the question, evidence requirement, and improvement
objective as guidance about what to inspect and verify, rather than a new
asserted user preference. Without matching observed signals, ordinary requests
receive no learned focus.

This is durable learning of where to look next. A permanent behavior change
can come from your saved questionnaire preferences or an admitted learning candidate.
The audit does not establish that CA's behavior improved merely because its
priorities changed, a model described an improvement, or a pass finished.

## Evidence and scoring boundaries

An observed signal is a reason to investigate. A tool failure can be a service
outage; a denied call can be the approval system working correctly; a
correction can refine a previously reasonable interpretation; an interrupt
can be the user's intended stop. These events cannot by themselves prove a
particular question failed.

Likewise, a task's verification or explicit acceptance applies to that task's
recorded scope. It does not prove every question passed. A successful coding
test does not verify secret handling, cancellation latency, memory authority,
or the quality of progress updates. Questions without the required observable
evidence retain an unknown answer. Coverage and outcome evidence must be read
alongside priorities; an empty or unchanged dataset does not show improvement.

The host owns the catalog, identities, evidence ingestion, and priority rules.
Model-generated answers and root-cause explanations are hypotheses. They do
not supply executable verifiers, promote their own changes, or override user
feedback. Broad personality or memory claims need appropriate scenarios and
receipts rather than being inferred from the existing coding fixtures.

For an actual candidate improvement, use CA's existing learning and harness
controls. They retain their frozen baseline/candidate identity, route, grants,
budget, evaluation, review, activation, and recovery requirements. A verified
comparison must demonstrate the relevant acceptance conditions without losing
already-passing behavior; the questionnaire does not bypass those controls.

## A useful review cycle

1. Open `/personality` or `ca personality`, answer the questions, and review
   and save the behavior you want.
2. Run the audit once to see evidence coverage, then use `watch` while CA
   accumulates outcomes from real independent tasks.
3. Inspect high-priority questions and their stated evidence requirements.
   Reproduce a suspected problem before deciding what should change.
4. Use Analyze or the learning workflow to propose one falsifiable improvement
   from exact attributable evidence, then evaluate it through existing controls.
5. Inspect later outcomes in the applicable cohort and retain the previous
   behavior when a change regresses or no longer applies.

Related implementation: [catalog](../src/harness_questionnaire_catalog.rs),
[automation harness](automation-harness.md), and
[HRS-253 evidence design](exec-plans/completed/HRS-253-evidence-driven-harness-improvement.md).
