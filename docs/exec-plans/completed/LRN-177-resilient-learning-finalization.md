# LRN-177 — Resilient Learning Finalization

Status: COMPLETE (2026-08-25)
Owner/session: completed user-directed continuation
Last updated: 2026-08-25

## Goal

Make one bare `/learn` finish safely against the real heterogeneous archive even
when a correction submission contains stale or hallucinated cluster references,
without weakening explicit cluster coverage, artifact authority, privacy,
publication atomicity, or the eight-turn bound.

## Non-Goals

- Do not copy or modify oracle or reference source.
- Do not install the release binary, dependencies, plugins, or services.
- Do not retry a live provider run until deterministic and repository-wide gates
  pass.
- Do not accept an unknown cluster as evidence or publication authority.

## Evidence and Current Behavior

- The user-authorized repository-built bare `/learn` on 2026-08-25 indexed 1,589
  messages across CA, Codex, Hermes, and MiMo and inventoried 110 reusable assets.
- Valid partial decisions survived correction, but turn eight cited unknown ID
  `cluster-90018d136211`; validation rejected the whole otherwise accumulated
  result and published nothing.
- `LearningPassBatch::validate_cluster_decisions` rejects any unknown cluster
  before checking whether all current catalog clusters were already explicitly
  decided.
- `merge_learning_pass_correction` merges cluster IDs before applying catalog
  authority, so an irrelevant non-authorizing unknown ID can poison the final
  accumulated result.

## Scope

Allowed files: `src/learning.rs`, `src/codex_learning.rs`, focused learning
tests, current learning harness documents, and deterministic TUI snapshots only
if output changes.

Forbidden files/areas: `oracles/`, `reference/`, credentials, installed binaries,
services, unrelated dirty worktree changes, and Git history.

Runtime state: tests use temporary profiles. One repository-built bare `/learn`
may run against the existing profile only after all gates pass, under the user's
standing authorization; it must not retry automatically.

## Design and Invariants

- The host remains authoritative for the current stable cluster catalog.
- Unknown cluster references never count as decided, cited, supported, or
  retired.
- A correction may discard an unknown reference only from a non-authorizing
  `skipped` or `needs_more_evidence` decision. Known IDs in the same decision are
  retained; an unknown-only non-authorizing row is dropped.
- `created` or `extended` decisions containing an unknown cluster remain invalid
  because they may authorize publication.
- Completion still requires every exact current cluster ID once, complete
  catalog delivery, valid citations, and a committed atomic journal.
- Invalid structured data, missing clusters, conflicting outcomes, and artifact
  drift remain in-run repair errors and publish nothing when exhausted.

## Milestones

### M1 — Characterize the live failure

- Add a failing adaptive-loop regression where a valid retained submission is
  completed by a correction containing all missing IDs plus one unknown skip ID.
- Add rejection coverage for unknown IDs on authorizing decisions.

### M2 — Canonicalize correction cluster references

- Filter only safe non-authorizing unknown references against the current host
  catalog before additive merge.
- Preserve exact missing-ID feedback and retained canonical provider compaction.

### M3 — Audit and validate

- Audit correction, resume, exhaustion, retirement, and publication paths for
  adjacent cluster-identity defects.
- Run focused learning tests, the complete workspace suite, formatting, strict
  Clippy, and release build.

### M4 — One live acceptance run

- Run the repository-built CA with bare `/learn` exactly once.
- Verify the final receipt, asset registry, truthful coverage, undo authority,
  and absence of incomplete publication.

## Acceptance Criteria

- An extraneous unknown cluster in a skip/needs-evidence correction cannot abort
  an otherwise complete valid run and never appears in final coverage.
- An unknown cluster attached to created/extended authority is rejected.
- Missing or conflicting real cluster decisions still fail closed.
- All focused and completion gates pass.
- The one live run completes and its receipt proves complete current-catalog
  decisions and atomic publication, or an external provider/environment failure
  is recorded without retry or evidence loss.

## Validation

Focused: adaptive learning correction/merge/validation tests and archive-learning
publication/retirement tests.

Completion: `cargo test --workspace --no-fail-fast -- --test-threads=1`,
`cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`, and
`cargo build --workspace --release --all-features`.

Manual: one live repository-built bare `/learn` after automated gates pass.

## Progress

- [x] Live failure reproduced and exact validation stage identified.
- [x] Failing regression added.
- [x] Cluster canonicalization implemented.
- [x] Adjacent protocol audit complete.
- [x] Focused and completion gates pass.
- [x] One live acceptance run verified.

## Decisions

- Decision: do not increase the turn cap to hide this failure.
  Evidence: the final submission was rejected for an irrelevant unknown identity,
  not insufficient evidence retrieval.
  Consequence: repair the host/model identity boundary while preserving the
  bounded protocol.

## Surprises and Discoveries

- The prior completed LRN-175 real run proved additive correction, but the next
  larger foreign-asset inventory exposed a distinct stale/unknown-ID edge case.
- A focused publication regression proved that two created rows may legitimately
  share one cluster outcome when the same workflow yields both memory and a
  command. Same-outcome overlap remains valid; only conflicting outcomes fail.
- Enumerating every cluster in the structured schema is bounded to 512 entries;
  larger catalogs retain the compact pattern schema and the same host-side exact
  canonicalization, preserving request-size limits.
- The successful live receipt exposed a separate presentation defect: its
  hard-coded source ordering omitted Hermes even though status and coverage
  journals counted four Hermes messages. Direct receipts now share the complete
  CA/Codex/Hermes/MiMo/Other renderer with status, with a focused regression.
- One unrelated `codex-team` stateful clean-filter test transiently observed one
  call instead of two during the first post-receipt full gate. Its exact rerun
  passed, and the subsequent complete serial workspace rerun passed cleanly.

## Recovery and Rollback

Revert only LRN-177 additions with targeted patches. A failed live run publishes
and retires nothing; use no reset, clean, stash, or broad formatter.

## Completion Record

Changed files:

- `src/learning.rs`: host-canonical correction references, exact bounded cluster
  enums, and stale/non-authorizing versus authorizing regressions.
- `src/codex_learning.rs`: complete source-count rendering in direct receipts,
  including Hermes, plus a zero-count regression.
- `NEXT_STEPS.md`, `CONTINUITY.md`, `TESTING.md`, `docs/.INDEX.md`, and this
  ExecPlan: durable design, live evidence, and validation record.

Commands and results:

- Focused learning tests: PASS (`learning` 98, `skills` 21,
  `prompt_commands` 5, `foreign_knowledge` 3, and the direct receipt source
  summary regression).
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS on the final
  clean run, 2,599 tests passed and 3 documented manual/external tests ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- Release `--version`, `--help`, and `--doctor`: PASS; `ca 0.1.0`, Ready.
- `git diff --check` and `.snap.new` hygiene: PASS.
- Repository-built bare `/learn`: PASS once, with no retry. Run
  `020c3875-ef34-4a0e-b034-51708fe22b0a` used
  `openai-codex/gpt-5.6-sol`, 4 adaptive turns, 20 bounded retrieval steps,
  120,418 input plus 5,503 output tokens, and committed atomically.
- Live coverage: 189 searchable sessions / 1,591 messages; CA 115, Codex 872,
  Hermes 4, MiMo 600; 207 messages / 66 sessions retrieved; 26 support
  sessions; both catalog pages delivered; 118/118 clusters explicitly decided;
  1,591 messages retired after commit.
- Live publication: truthful zero-change success. The recurring BSD rsync,
  Transmission, and seedbox cluster was skipped because the three exact
  MiMo commands already exist at `~/.mimocode/command/`; no redundant asset was
  written. Undo authority is
  `/learn undo 020c3875-ef34-4a0e-b034-51708fe22b0a`.

Remaining risks:

- No known LRN-177 correctness defect remains. A heterogeneous 118-cluster real
  archive required four turns rather than the two-turn clear-case target, but
  remained within the eight-turn cap and independently bounded retrieval path.
- The optimized repository binary was exercised in place for acceptance. After
  plan completion, the user explicitly authorized installation; the exact
  validated release was atomically installed at `/Users/aji/.local/bin/ca`
  (SHA-256 `5474464afa6dadf5c2e6497481ff31c72a62c2895d334bc35ec3a5d841c9d415`),
  with the prior binary preserved at
  `/Users/aji/.local/bin/ca.backup-lrn177-20260825T135550Z`.

Harness updates:

- LRN-177 is complete in `NEXT_STEPS.md`; continuity and testing records include
  the live journal, source-total receipt repair, transient unrelated test rerun,
  final clean gates, and the separately authorized installed-binary verification.
