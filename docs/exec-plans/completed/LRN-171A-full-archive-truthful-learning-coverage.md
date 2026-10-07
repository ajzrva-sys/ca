# LRN-171A — Full-Archive, Truthful Learning Coverage

Status: COMPLETE (2026-08-24)
Owner/session: current user-requested implementation
Last updated: 2026-08-24

## Goal

Make one bare `/learn` search every eligible recent session while keeping each
model request bounded, publish only validated output, and report honestly which
evidence was indexed, searchable, retrieved, inspected, cited, and retired.

## Non-Goals

- Do not copy or modify MiMo, Codex, Cursor, or other oracle source.
- Do not expose raw tool output, email, secrets, incomplete turns, reasoning, or
  subagent chatter to the learning model.
- Do not install a binary, activate background maintenance, or invoke the real
  learning profile before all code and validation gates pass.
- Do not broadly reopen old learning runs. Repair only the latest legacy run
  `567d44c0-fb60-466e-9cb5-d4f474ddf482`.

## Evidence and Current Behavior

- `compact_materialized_corpora` selects at most
  `learning::MAX_CORPUS_SESSIONS` before constructing `LearningCorpus`.
- `LearningCorpus::adaptive_search` searches only its retained `sessions`, so
  sessions beyond the first 32 cannot be retrieved.
- `apply_proposal_with_cancel` then adds every review message ID to
  `processed_message_ids`, including evidence the model could not search.
- The real completed run reported 32 supporting sessions but retired 1,515
  messages from CA, Codex, and MiMo sources.
- The read-only MiMo Distill contract treats raw trajectory as authoritative,
  memory as an index, inventories existing assets, searches recent history, and
  completes one manual invocation with a compact result.

## Scope

Allowed files: `src/learning.rs`, `src/codex_learning.rs`, directly related TUI
render/tests/snapshots, and current learning documentation.

Forbidden files/areas: `oracles/`, `reference/`, credentials, installed
binaries/services, Git history, and unrelated dirty changes.

Runtime state: tests use temporary profiles. After every gate passes, one real
bare `/learn` may atomically reopen the named legacy run and publish a corrected
result, as explicitly authorized by the user.

## Design and Invariants

- The run-local corpus indexes all eligible sanitized messages. The 32-session
  limit applies only to one search result; session inspection remains bounded to
  eight sessions and the existing byte ceiling.
- Every session belongs to a deterministic workflow cluster. Repeated safe
  action shapes are primary; conservative title/request/conclusion terms are
  secondary; unmatched sessions remain singleton clusters.
- The initial prompt receives the complete cluster directory when bounded.
  Larger catalogs are paged through a host-controlled tool. Checkpoints retain
  only identifiers, digests, counts, and delivered page indexes.
- State and journals distinguish indexed, searchable, retrieved, inspected,
  supporting, and retired evidence. Legacy `processed_message_ids` remains a
  read-compatible alias for retired IDs.
- A complete revision retires only after exact index membership, complete
  cluster-directory delivery, search authority, validated output, and committed
  publication. Otherwise only inspected messages and retrieved messages from
  cited support sessions retire.
- Every successful run, including a no-op, has a committed coverage journal.
  Cancellation or failure retires nothing incomplete.
- The next direct run reopens only the latest matching coverage-less journal,
  preserves its memory/assets, records the migration idempotently, and includes
  existing memory in duplicate checks.

## Milestones

### M1 — Failing full-archive characterization

- Add a 184-session/1,515-message fixture whose reusable BSD workflows occur
  outside the first 32 sessions.
- Prove current search cannot find the late evidence and current completion
  over-retires it.

### M2 — Full run-local index and cluster catalog

- Retain all sanitized sessions/messages and exact source IDs.
- Add bounded search, inspection, cluster paging, and identifier-only coverage
  checkpoints.

### M3 — Truthful retirement and targeted migration

- Upgrade state/journal/receipt coverage records and atomic publication.
- Reopen only the named latest legacy journal during the next manual direct run.

### M4 — Product and TUI coverage

- Prove two-turn BSD command publication, partial coverage, zero-change
  completion, privacy exclusions, interruption/resume, cancellation, rollback,
  invalid repair, and turn exhaustion.
- Update wide/narrow transcript and status snapshots with truthful coverage.

### M5 — Validation and one authorized run

- Run focused learning/TUI suites and all repository completion gates.
- Reconcile harness documents and archive this ExecPlan.
- Invoke bare `/learn` exactly once against the current profile; never retry an
  external failure automatically.

## Acceptance Criteria

- All 184 fixture sessions and 1,515 messages are searchable; a search returns
  at most 32 sessions and bounded bytes.
- Late BSD rsync, Transmission, and seedbox evidence yields parameterized
  commands in two provider turns; dedup remains a candidate and one-off FreeBSD
  work is skipped.
- Status and receipt show exact state/source/cluster coverage rather than a
  generic processed count.
- Complete coverage may retire the exact review; incomplete coverage leaves
  untouched evidence eligible.
- The targeted legacy repair reopens exactly 1,515 IDs once and preserves the
  previous memory and assets.
- MiMo SQLite remains query-only without WAL/SHM creation and forbidden content
  never enters model-facing data.

## Validation

Focused: archive-learning, adaptive-learning, command-publication, migration,
privacy, TUI render, and snapshot suites.

Completion: `cargo test --workspace --no-fail-fast`, all-target/all-feature
tests, `cargo fmt --all -- --check`, strict workspace Clippy, all-feature
release build, release smokes, diff/snapshot hygiene, and protected-tree status.

Manual: one real bare `/learn` only after all automated checks pass.

## Progress

- [x] Current compaction, adaptive search, retirement, journal, and receipt
  boundaries traced.
- [x] Read-only MiMo behavior reviewed.
- [x] Failing late-session and over-retirement characterizations added.
- [x] Full archive index and cluster catalog implemented.
- [x] Coverage state, retirement policy, and targeted migration implemented.
- [x] Focused and complete validation passed.
- [x] One authorized real `/learn` completed or its external failure recorded.

## Decisions

- Decision: repair only the latest named legacy run.
  Evidence: the user explicitly selected “This run only.”
  Consequence: older legacy journals remain unchanged.
- Decision: preserve host-side intelligence and bound retrieval, not the search
  universe.
  Evidence: the 32-session corpus caused the reported false completion.
  Consequence: prompt size remains bounded without discarding searchable work.

## Surprises and Discoveries

- The 797-message test checked only that aggregate totals appeared in the
  prompt; it did not create or search a 797-message corpus.
- Publication records every review message as processed even though the
  authorization corpus contains only the selected 32 sessions.
- Byte-truncating a serialized search response could produce invalid JSON. The
  bounded search result is now assembled incrementally as a complete envelope.
- A resumed provider conversation does not contain previously returned cluster
  tool bodies, so resume conservatively redelivers page zero and requires all
  other catalog pages again before complete retirement.
- Journal version 3 needed explicit rollback and undo compatibility; otherwise
  truthful coverage journals could be written but not safely recovered.

## Recovery and Rollback

Revert only LRN-171A source/tests/docs. Do not reset, clean, stash, or overwrite
the surrounding dirty worktree. A failed real run must leave the reopened
evidence eligible and publish nothing incomplete.

## Completion Record

Changed files:

- `src/learning.rs`: full run-local corpus, deterministic cluster catalog,
  bounded search/inspection tools, exact identifier coverage, and resumable
  adaptive checkpoints.
- `src/codex_learning.rs`: truthful state/journals/receipts, exact retirement,
  targeted legacy reopening, archive-wide selected-message projection binding,
  and end-to-end publication tests.
- `src/tui/render_tests.rs` and four learning snapshots: wide/narrow coverage
  activity, status, and final receipt proof.
- Current learning harness documents and this ExecPlan.

Commands and results:

- `cargo test --bin ca codex_learning::tests:: -- --nocapture`: 61 passed.
- `cargo test --bin ca learning::tests:: -- --nocapture`: 86 passed.
- Focused direct-learning and status snapshot tests: passed.
- `cargo fmt --check`: passed.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `cargo test -- --test-threads=1`: passed across the CA, CLI, embedded
  scheduler, integration, and doc-test suites.
- Parallel `cargo test`: the CA suites passed, but the embedded scheduler test
  `stateful_clean_filter_cannot_replace_prevalidated_commit_bytes` raced twice.
  Its exact rerun passed, as did the complete serial suite; scheduler source was
  outside this change.
- `cargo build --release`: passed.

Real-run result:

- Bare `/learn` run `9c24e870-3ae0-4036-b95d-80879b283659` committed with
  `openai-codex/gpt-5.6-sol` after five adaptive turns and one in-run correction.
- Coverage was Indexed/Searchable/Retired 1,533 messages in 186 sessions;
  Retrieved 144 messages/30 sessions; Inspected 43 messages/eight sessions;
  Used as support eight sessions; 116 clusters and both catalog pages delivered.
- Source coverage was CA 115, Codex 840, and MiMo/Xiao 578 messages.
- Publication created `.ca/commands/review-oracle-gaps.md`, refreshed the slash
  palette immediately, and recorded `/learn undo
  9c24e870-3ae0-4036-b95d-80879b283659`.
- Actual usage was 225,464 input and 4,015 output tokens across five provider
  turns. The archive was ambiguous enough to require investigation and repair;
  the deterministic clear-case fixture remains two turns.

Harness updates:

- `NEXT_STEPS.md`, `CONTINUITY.md`, and `docs/.INDEX.md` record completion and
  the committed real-run outcome.
