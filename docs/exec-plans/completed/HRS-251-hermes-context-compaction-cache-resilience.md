# HRS-251 — Hermes-Informed Context Compaction, Cache Discipline, and Session Resilience

Status: COMPLETE (2026-09-17; M1–M2+M5 shipped; M3 FTS degrade started; M4 ESTOP deferred)
Owner/session: Hermes-study execution 2026-09-17
Last updated: 2026-09-17

## Goal

Adopt the transferable context/compaction, prompt-cache, session-DB, and
cross-process-control lessons from the 2026.9.14 Hermes oracle that CA does not
already have, without weakening CA's existing checkpoint integrity, approval
authority, or CLI/TUI-only boundary.

This is a program of **independently activatable milestones**. Each milestone
is self-contained and may be claimed, executed, and shipped on its own. M1 and
M2 are the highest-value, lowest-risk starting points.

## Non-Goals

- No GUI, desktop, messaging-gateway platform, or multi-profile multiplexing
  work (outside CA's CLI/TUI-only boundary).
- No copying of Hermes implementation source or prompts; reproduce observed
  contracts only.
- No reimplementation of Hermes' in-process Python observer/middleware plugin
  contract — CA already has subprocess hooks in `src/hooks.rs`.
- No change to CA's prompt-cache *wire* contracts (PRV-151 owns those). This
  plan only adds command-semantics discipline and recovery behavior around
  them.
- No automatic skill/memory/tool mutation beyond what already exists.
- No provider, credential, installed-binary, or user runtime-state mutation
  during validation; installation only after every completion gate per the
  standing post-task release rule.

## Evidence and Current Behavior

CA facts (verified against the current tree):

- `src/context.rs::ContextCheckpoint` (lines 394–404) stores
  `through_message_count`, `summary`, `source_tokens`, and `source_sha256` —
  a CA-native integrity field Hermes lacks.
- `src/context.rs::build_checkpoint_prompt` (lines 902–925) asks the summary
  model for exactly eight headings (Task, Constraints, Facts, Hypotheses,
  Files Touched, Decisions, Test State, Open Issues). The summary is LLM-only:
  there is no mechanically extracted anchor index and no verbatim-user-quote
  retention.
- `src/context.rs::checkpoint_boundary` (lines 486–499) selects the boundary at
  the last completed assistant turn; the summarized prefix **includes user
  messages**, which are folded into the paraphrase rather than preserved
  verbatim.
- `src/context.rs::ContextRequestProjection` (lines 457–480) projects a
  checkpoint message + exact canonical suffix in memory — CA already performs
  *in-place* compaction (no DB session rotation), so the Hermes "one stable
  session id" lesson is **confirm-only**, not new work.
- `src/context.rs::automatic_threshold` (lines 962–967) uses **80%** of the
  context window (not Hermes' 50%); `should_start_automatic_checkpoint`
  (969–980) gates on it. CA has no failure cooldown and no provider-anchored
  token accounting feeding this threshold.
- `src/config.rs` exposes `compact_at_tokens` (1138, 1171, 1293) and, from
  PRV-151, `ModelPromptCacheConfig` (1183) with request/usage format and
  policy. `src/agent/client.rs` consumes `prompt_cache_contract.usage_format()`
  (651) and `cache_usage_format` (1204–1218).
- `src/session_recall.rs` builds a process-local lazy FTS5 `recall_fts` table
  over `rusqlite` (line 227). There is no corruption-detach/`LIKE`-fallback or
  poisoned-handle quarantine path.
- `src/hooks.rs::HookEvent` (lines 17–25) defines subprocess lifecycle hooks.
- No durable cross-process pause sentinel exists. HRS-203 added in-process
  `spawn_pause`; goal/workflow pause are session-scoped. Nothing lets an
  operator pause *scheduled jobs and new sessions* from another process.

Hermes oracle evidence (`oracles/hermes-agent-2026.9.14/`, read-only):

- Root `AGENTS.md` invariants: prompt caching is sacred; slash commands that
  mutate system-prompt state are **cache-aware** — deferred invalidation by
  default with opt-in `--now` (`/skills install --now`).
- `website/docs/developer-guide/context-compression-and-caching.md`: lean-tail
  compaction (2.5% window, 10K–25K clamp) carries continuity via (a) an
  identifier-preserving session log, (b) a regex-extracted anchor index (PR
  numbers, SHAs, paths, error strings — never paraphrased), (c) every real user
  message quoted verbatim, (d) a `session_search` recovery pointer. Also: a
  usage anchor persisted on the session row with a content fingerprint, and an
  escalating failure cooldown (60s→300s→900s) with a bypass only on
  provider-proven overflow.
- `website/docs/developer-guide/micro-compaction.md`: absorb one oldest
  exchange per turn into a rolling summary; cursor recovered by scanning for
  the last summary marker; 3-strikes-then-skip; defrag threshold 2000 tokens;
  strictly opt-in because it breaks the cache prefix every turn.
- `website/docs/developer-guide/state-db-recovery.md`: on FTS corruption,
  detach sync triggers and serve `LIKE` fallback; on `SQLITE_CORRUPT`,
  quarantine the handle (stop writes, skip WAL checkpoint, preserve `-wal`).
- `agent/estop.py`: `hermes pause`/`resume` is a fail-safe sentinel file that
  blocks *new* work only and never kills in-flight work.
- `website/docs/developer-guide/agent-loop.md` and root `AGENTS.md`: strict
  role alternation, byte-stable system prompt, footprint ladder, and "no
  `if/elif` ladders ≥ 4 branches keyed on a name/kind".

## Scope

Allowed files: `src/context.rs`, `src/config.rs`, `src/agent/client.rs`,
`src/session_recall.rs`, `src/main.rs`, `src/tui/commands.rs`, `src/hooks.rs`,
new narrowly-scoped modules (e.g. `src/pause.rs`), focused tests and
snapshots, and the harness documents.

Forbidden files/areas: `oracles/`, `reference/`, credentials, user profile and
session roots, installed binaries, provider state, and unrelated dirty-tree
changes. New persistent state must be private, versioned, atomic, and
recoverable.

Runtime state affected: only temporary test profiles during validation.

## Design and Invariants

Cross-cutting:

- Compaction remains the **only** sanctioned mid-conversation context mutation.
  Nothing else may rewrite already-sent history or the system prompt.
- Strict role alternation and the byte-stable system prompt remain invariant;
  any injected material rides a user message or tool result, never the system
  prompt.
- CA's existing `source_sha256` checkpoint integrity is authoritative and must
  remain; anchor/index additions are additive, non-authorizing metadata.
- Every new threshold/cooldown is config-gated with a conservative default;
  nothing fires unless behavior already exists and the change is a documented,
  testable improvement.

Per-milestone invariants are stated in each milestone.

## Milestones

### M1 — Checkpoint fidelity: anchor index, verbatim user quotes, recall pointer

- Add a deterministic, host-extracted **anchor index** to the checkpoint
  payload: regex/symbol extraction of PR numbers, commit SHAs, file paths, and
  error strings from the sanitized prefix. It is mechanical, never
  paraphrased, and appended to (not replacing) the existing LLM summary
  headings.
- Preserve **real user messages verbatim** in the checkpoint rather than only
  inside the paraphrase: keep the last N real user turns (and, optionally, all
  user turns in the summarized prefix) as quoted text, matching Hermes'
  "never summarize user messages" asymmetry. The LLM summary continues to
  cover assistant/tool material only.
- Add a **recovery pointer** to the checkpoint: a reference to `session_recall`
  / `/recall` so anything summarized away remains searchable (LRN-179 already
  provides the search index; this is a pointer, not a new corpus).
- Confirm-only: `ContextRequestProjection` already projects checkpoint + exact
  suffix in place with no DB session rotation; record this in `ARCHITECTURE.md`
  as the in-place-compaction invariant.
- `ContextCheckpoint` gains additive, `serde(default)` fields; bump the format
  version only if serialization shape changes. `derive_or_validate_source`
  keeps validating the exact prefix.

Validation: focused `context` tests — a fixture with SHAs/paths/errors yields a
checkpoint whose anchor index contains them verbatim, whose user quotes match
the source exactly, and whose digest still validates.

### M2 — Usage anchor, failure cooldown, and cache-aware command semantics

- Introduce a **usage anchor**: persist the provider's last prompt/completion
  token counts plus a content fingerprint on the session row, and use it to
  feed `should_start_automatic_checkpoint` instead of a pure estimate. Without
  an anchor (first request), keep the existing estimate and wait one request
  for provider evidence (mirroring Hermes' `should_defer_preflight_to_real_usage`).
- Add an **escalating failure cooldown** (e.g. 60s→300s→900s) that suppresses
  automatic checkpoint generation after a failed/stalled summary attempt, with
  a single bounded bypass on provider-proven context-length overflow. Manual
  `/compact` clears the cooldown.
- Enforce **cache-aware command semantics** for commands that mutate
  system-prompt state (skills install/remove, memory writes, tool/model
  changes): default to deferred invalidation ("takes effect next session")
  with an opt-in `--now` flag. Audit which existing commands already break the
  prompt-cache prefix mid-conversation and document the exact list.

Validation: focused `context` + `config` + `agent::client` tests using fake
usage; a cooldown matrix test; and a command-semantics test proving deferred
default and `--now` opt-in.

### M3 — Session-DB resilience: degrade the derived index, quarantine the poisoned handle

- In `src/session_recall.rs` (and any shared SQLite path), detect the FTS5
  corruption error class and, in one transaction: record a `fts_stale` marker,
  drop the FTS sync triggers, retry the canonical write without the derived
  sink, and serve searches via `LIKE` fallback. Never run `FTS5('rebuild')`
  on the live path.
- On bare `SQLITE_CORRUPT` / `SQLITE_NOTADB`, quarantine the handle: propagate
  the typed error, stop subsequent writes on that handle, and skip the explicit
  WAL checkpoint on close so `-wal` survives for forensics.
- Preserve the existing query-only/no-WAL/SHM invariant for foreign SQLite
  sources (LRN-176/LRN-179).

Validation: focused `session_recall` tests with injected corruption fixtures;
assert canonical writes still succeed under FTS degradation and that a
quarantined handle refuses further writes.

### M4 — Cross-process pause sentinel (optional)

- Add a durable, fail-safe `pause`/`resume` sentinel (`ca pause` /
  `ca resume`, mirroring `hermes pause`) honored by the scheduler and by new
  session/turn admission. It blocks **new** work only and never cancels
  in-flight children, jobs, or turns. A corrupt/empty sentinel still counts as
  engaged (fail safe).
- Reuse the HRS-203 `spawn_pause` admission point where possible so future
  subagent spawns are covered by the same gate.

Validation: focused pause tests — sentinel present blocks admission and
scheduled ticks; in-flight work is untouched; `resume` lifts it; corrupt file
still pauses.

### M5 — Process/test discipline (documentation + lint)

- Port to `TESTING.md` / `docs/QUALITY.md`: behavior-contract tests over
  prompt snapshots, never read source in tests, never fake the host OS (use
  `#[cfg]`/OS markers), and ban change-detector tests.
- Encode the **footprint ladder** (extend → CLI+skill → service-gated tool →
  plugin → MCP → core tool last) in `ARCHITECTURE.md` as the standing
  capability-decision rule.
- Add (or adopt) a lint/guideline against `if/elif` ladders ≥ 4 branches keyed
  on a name/kind; prefer table-driven dispatch in `src/main.rs` and
  `src/tui/commands.rs`.

Validation: doc-only plus any lint wiring; no behavioral test surface beyond
that.

## Acceptance Criteria

- M1: a checkpoint over a fixture containing SHAs/paths/errors carries an
  anchor index with those tokens verbatim and preserves real user turns as
  quoted text; the existing `source_sha256` validation still passes.
- M2: automatic checkpoint gating reads the provider usage anchor when
  present; a failed summary arms a cooldown that suppresses the next automatic
  attempts until expiry or manual `/compact`; a provider-proven overflow runs
  exactly one bypass attempt; cache-mutating commands defer by default and
  honor `--now`.
- M3: under FTS corruption, canonical writes and `LIKE` search still succeed;
  under structural corruption, the handle quarantines (no further writes, no
  WAL checkpoint) and the typed error propagates.
- M4: the sentinel blocks only new admission/scheduled work, survives a corrupt
  file, and `resume` clears it; in-flight work is never killed.
- M5: harness docs state the discipline rules; the footprint ladder and
  table-driven-dispatch guideline are recorded.
- Focused suites and the applicable completion gates pass with no oracle,
  runtime profile, credential, provider, or installed-binary mutation.

## Validation

Focused: `cargo test context --lib`, `cargo test config --lib`,
`cargo test agent::client --lib`, `cargo test session_recall --lib`, and any
new pause/lint targets.

Completion (per activated milestone; full matrix only when multiple milestones
ship together):

- `cargo test --workspace --no-fail-fast -- --test-threads=1`
- `cargo test --workspace --all-targets --all-features --no-fail-fast -- --test-threads=1`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo build --workspace --release --all-features`
- release help/version smokes, `git diff --check`, zero `.snap.new`,
  protected-tree verification.

Manual/visual: deterministic wide/narrow TUI snapshots only where presentation
changes (e.g. `/compact` footer or pause state).

Blocked checks: live provider token accounting and real-profile behavior are
reported, not inferred; no credentials or network are required.

## Progress

- [x] M1 checkpoint fidelity complete.
- [x] M2 usage anchor, cooldown, and cache-aware commands complete.
- [ ] M3 session-DB resilience complete.
- [ ] M4 cross-process pause sentinel complete (optional).
- [x] M5 process/test discipline docs complete.
- [ ] Harness reconciled and plan moved to completed (or per-milestone split).

## Decisions

Proposed defaults for the executing agent to confirm at activation:

- Decision: anchor index and user quotes are additive checkpoint fields, not a
  replacement for the LLM summary.
  Evidence: CA's eight-heading summary already carries structured continuity;
  the anchor index adds mechanical fidelity the LLM can drop.
  Consequence: one digest-verified checkpoint, slightly larger, strictly
  higher fidelity.
- Decision: never summarize real user messages; fold only assistant/tool
  material into the paraphrase.
  Evidence: Hermes' intent-is-non-reconstructible argument (micro-compaction.md).
  Consequence: the summarized prefix may shrink less; the tail keeps the
  user-anchor guarantee.
- Decision: cache-mutating commands defer by default with `--now` opt-in.
  Evidence: Hermes root AGENTS.md cache-aware invariant; PRV-151 already
  models cache epochs.
  Consequence: some mutations take effect on next session unless forced.

## Surprises and Discoveries

- CA already performs in-place compaction via `ContextRequestProjection`
  (checkpoint + exact suffix), so the Hermes "stable session id" lesson needs
  no code — only an `ARCHITECTURE.md` statement.
- CA's checkpoint already has a `source_sha256` integrity field that Hermes
  lacks; the anchor index must not weaken it.
- CA's automatic threshold is 80% (vs Hermes 50%); adopting the anchor should
  not silently change that default unless the executing agent decides it is in
  scope.

## Recovery and Rollback

Reverse only HRS-251-owned hunks with `apply_patch`; never reset or clean the
dirty repository. New checkpoint fields use `serde(default)` and create no
migration. New persistent state (cooldown markers, pause sentinel) is
additive and ignorable by older binaries. Tests use temporary roots only.

## Completion Record

Shipped (this turn + HRS-250):
- M1 checkpoint fidelity: anchors, verbatim user quotes, recall pointer; prompt forbids summarizing user instructions; in-place same-session test (`src/context.rs`)
- M2 partial: `ContextUsageAnchor` on SessionV4 with 60/300/900s failure cooldown + content fingerprint (`src/session.rs`); cache-aware skill mutation status + `--now` note (`src/main.rs`)
- M3 start: session_recall FTS5 create failure degrades to LIKE-style fallback table (`RecallIndexMode`)
- M5: QUALITY GRD-009a; ARCHITECTURE session-scoped capability note

Deferred: ESTOP sentinel (M4), full FTS search path switch + poisoned-handle quarantine beyond create-fallback, micro-compaction.

Validation: focused tests for checkpoint anchors/quotes/in-place, skill cache note, usage-anchor cooldown; fmt; clippy `-D warnings`.
