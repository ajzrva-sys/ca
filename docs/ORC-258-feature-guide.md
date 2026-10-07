# ORC-258 feature guide

This guide describes the ORC-258 feature delivery. Later AUT-259 and BUG-262/263
records supersede its original workspace and installation status; the installed
release is verified in [the publication report](PUBLICATION_VALIDATION.md). Actual
eight-hour duration and selected real-model retrieval checks remain separately
tracked in [the active plan](exec-plans/active/ORC-258-oracle-informed-improvements.md).

## Checkpoints and saved-session ownership

Manual and automatic compaction require the eight existing sections in order.
Invalid output receives one bounded quality repair using the same source;
transport errors receive no quality repair. A failed or cancelled operation
keeps the previous checkpoint and history. Historical checkpoints still load.

Active turns and heartbeats hold shared session writer ownership through their
final save. Idle panes reload changed committed history before their next turn.
Conflicting revisions, duplicate stored identities and changed workspace
ownership refuse writes rather than replacing another owner's state.

Stored identity checks use bounded private caches validated against fresh file
identity and timestamp proofs. Cold inventories prepare through blocking workers;
short write checks can refuse an incomplete inventory without discarding pane
edits. Their cooperative deadline bounds inventory work, not whole-save latency.

## Saved edits and recovery

Saved sessions automatically record typed write, edit, hashline, patch and
delete operations, including parent-attributed child edits. Private recovery
blobs and journals are separate from transcripts and exports. A storage error
does not cancel an ordinary edit; its tool result warns about incomplete
coverage. Restart reconciliation observes exact before/after states and leaves
conflicts visible.

- `/recover` lists recorded turn groups and coverage.
- `/recover preview TURN_UUID` shows bounded changes and a confirmation token.
- `/recover restore TOKEN` restores the exact preview after checking current
  hashes, workspace identity, journal revision and relevant active work.
- `/undo` and `/redo` reload persisted native receipts, including after restart.

Restoration records its own rescue transaction, preserves the transcript and
invalidates affected verification. Repeated edits collapse to each file's
first-before/last-after state. Shell, MCP, foreign execution, unsaved sessions,
binary recovery and whole-workspace rollback are outside this coverage. The
existing file and batch bounds remain; retention is 20 completed groups per
session and 256 MiB per profile. Pending/conflicting records are not pruned to
make room. Older binaries and unrelated external writers do not participate in
the new ownership coordinator; exact observed file drift still blocks restore.

## Plan notes and branches

Open `/plan show`, select plan text and press `N` to prepare a comment. Use
`/plan comments` to inspect notes, task identities and logical Unicode offsets.
Comments can also be added directly with `/plan comment task NUMBER_OR_UUID
COMMENT` or `/plan comment text START:END COMMENT`. Edit/delete draft notes with
`/plan comment edit UUID COMMENT` and `/plan comment delete UUID`.

`/plan submit` submits one revision request for the reviewed plan. It does not
approve or execute the plan. Changed content makes affected drafts stale; task
progress alone does not change the content digest. Save/resume retains drafts;
forks retain historical provenance without actionable submission queues.

`/branches` and `/tree` show paginated family metadata, including archived
ancestors and unavailable ancestry. Use `/branches page OFFSET` and
`/branches switch SESSION_UUID`. Switching follows existing save-before-switch
behavior. Archived sessions are restored through `/resume`.

`/bookmark` lists message-boundary bookmarks; `/bookmark add LABEL` bookmarks
the latest completed boundary. Rename/delete/branch actions are shown in the
bookmark pager. Forks retain only valid copied-prefix anchors. Labels are at
most 256 bytes; each session has at most 128 bookmarks.

`/branches summarize SESSION_UUID` generates a bounded, accounted, tool-free
summary of divergent history. Review its preview, then explicitly use
`/branches accept TOKEN` or `/branches cancel`. Acceptance records context
provenance separately and switches; it does not transfer process grants,
queued execution or verification.

## Dashboard and lifecycle events

`/dashboard` observes all local panes and children with stable owner identities.
Selected previews do not change focus. Local Focus, Reply and Stop use the
existing owning pane/child controls. Other running processes publish private
bounded metadata with verified process-start identities. External rows support
observation only; their available transcript previews are labeled
last-committed. Stale records are unavailable rather than counted active.

Existing hook configuration accepts opt-in `pre_compact`, `post_compact`,
`subagent_start` and `subagent_stop`. Pre-compaction denial preserves history.
Post-compaction runs only after checkpoint activation, including deferred
activation. Child terminal notifications run once after owned cleanup. Hook
contexts are scoped to authority owner and invocation, so another pane or child
cannot consume them.

## Tool discovery, reads and recall

Built-in discovery remains eager by default. Opt in with:

```toml
[agent]
tool_discovery = "lazy"
```

The host-owned `search_tools` reveals only authorized native tools. Explicit
replacements/includes and required workflow/structured tools remain pinned.
New schemas activate after the complete result batch commits and appear in the
next request. Fixed review/checkpoint contracts remain eager. Native independent
read runs use explicit eligibility, at most four calls in flight, original
result order and serial approval/hook/mutation barriers.

Lexical recall remains the default. Optional hybrid recall requires explicitly
registered local runtime/model IDs:

```toml
[memory.local_hybrid]
runtime_id = "YOUR_REGISTERED_RUNTIME_ID"
model_id = "YOUR_REGISTERED_EMBEDDING_MODEL_ID"
```

No model downloads or remote embeddings are automatic. Native memory indexes
prepare off the input loop and freeze selection for a logical turn. An
unavailable/stale index or query over 200 ms falls back to the lexical result.
Evaluate fixtures with `ca memory benchmark tests/fixtures/memory_recall/ranking.json`.
Use `--embedding-runtime ID --embedding-model ID` for an explicitly selected
real model; synthetic-score fixtures are separately labeled.

Offline benchmark JSON preserves the quality fields and adds `latency`.
Its `ranking` summary reports measured sample count, median, p95, maximum and
total milliseconds. Each case reports `ranking_ms` and
`selected_context_bytes`, the UTF-8 byte size of the rendered recall context.
Synthetic-score runs measure selection and ranking; they do not measure an
embedding model or establish real-model quality.

The real-model report separates cold preparation, lexical baseline and warm
queries. `latency.cold_preparation` reports verified server startup, memory
indexing and their total. `latency.cases` reports each query's `lexical_ms`,
`embedding_ms`, `selection_ms`, `warm_added_ms` and lexical/selected context
bytes. Warm added time includes embedding, vector similarity, fusion, ranking
and context selection; the lexical baseline is not subtracted.
`warm_attempts` includes every query, while `warm_semantic` summarizes only
successful semantic queries. Both summaries include median, p95 and maximum.

Each warm query has an explicit `outcome`: `semantic`, `timeout`,
`embedding_error` or `selection_timeout`. A failed or over-budget query uses
the exact lexical result and is counted in `fallback_cases`.
`warm_gate_passed` requires every query to complete semantically within 200 ms.
The report's `acceptance.passed` and calibration gates require both that warm
gate and the quality gates: sufficient positive/negative samples, at least ten
percentage points of recall improvement, identifier nonregression, and a
negative-query false-positive rate at most 5% without increasing the lexical
rate. Failed gates remain inspectable JSON; a successful CLI exit alone does
not mean the acceptance gate passed.

Benchmark timing covers the selected fixture's managed local embedding and
shared selection/render path. It does not measure TUI scheduling or engine
mutex contention. Synthetic test servers may report a null cold startup time
because the caller supplied an already running fixture. Real-model quality
and warm latency remain unmeasured until the user explicitly selects a runtime
and model; no real-model gate has passed.

## Local automations

`/automation` lists definitions. `inspect`, `history`, `pause`, `resume`, `run`
and `result` actions use the existing local scheduler/execution mechanisms.
`/automation preview FILE` freezes a normalized definition and displays its
exact one-use token. `create TOKEN` and `update ID TOKEN` confirm that preview;
later file changes do not alter it. Conversational proposals use the same
preview and confirmation path.

Conversational `list_local_automations` returns bounded metadata for the current
profile. It does not expose definition prompts or execution grants. Definition
removal also uses a reviewed, one-use token and retains run history and results.

Busy heartbeat occurrences coalesce to one pending run. Recorded provider
results settle across crashes without repeating provider work. Request, token,
tool, runtime, output and cost admission occurs before dispatch. Unknown usage
settles conservatively. Default heartbeat meaningful-change reporting stays
quiet for unchanged/duplicate state; native notifications use bounded metadata,
durable one-attempt admission and verified-focus suppression. Service
installation continues to require its existing separate confirmation.

Output admission bounds retained native command and parallel-read results before
dispatch. Tool batches reserve their output ceilings conservatively. Settled
budget events outside the rolling window move to immutable private receipts;
unsettled reservations remain in the live ledger.
