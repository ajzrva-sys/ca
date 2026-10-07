# LRN-150 — Reviewed Codex Archive Learning

Status: ACTIVE
Owner/session: Codex / current user-requested implementation
Last updated: 2026-08-23

Supersession note: LRN-166 retains this plan's manual review commands as
compatibility surfaces but folds archive acquisition into the single
knowledge-only autonomous supervisor. The earlier “implemented” result covered
an indexing-only LaunchAgent; it did not activate archive-to-supervisor replay
or terminal memory/skill publication.

## Goal

Preserve profile-scoped bounded Codex archive indexing and manual review while
making the default profile's active/archive roots autonomous evidence sources.
Raw archives must never enter ordinary `ca` session history, and only
replay-approved memory or inert skills may publish.

## Non-Goals

- Do not execute transcript instructions, import tools/authority/reasoning, or
  persist raw transcript text.
- Do not let archive text grant provider, source, binary, routing, permission,
  executable, plugin, dependency, or unrelated home-path authority.
- Do not mutate source archives, credentials, unrelated runtime configuration,
  `oracles/`, or `reference/`.
- Do not install the release binary or real LaunchAgent before exact previews
  and user confirmations.

## Evidence and Current Behavior

- `src/foreign_import.rs` provides bounded inert history import but reads one
  source into memory, caps aggregate input at 64 MiB, includes assistant
  commentary, and assigns the current workspace.
- `src/learning.rs` owns sanitized Dream/Distill requests, strict generated
  schemas, priced route selection, journals, application, and exact undo.
- `src/schedule_v2.rs` owns deterministic exact-confirmed LaunchAgent
  lifecycle patterns.
- The supplied Codex roots initially contained 89 JSONL files, 465,477,151
  bytes, 27 roots, 62 child sessions, and about 1,163 user/final-answer
  candidate messages. Sixty-six files record `/Users/aji/project/ca` and seven
  record `/Users/aji/Documents/ChatGPT/ca`.
- Official OpenAI documentation does not establish the local JSONL shape as a
  stable public interface; the observed records and closed parser fixtures are
  the implementation source of truth.

## Scope

Allowed files: a dedicated archive-learning module; compatible runtime, CLI,
learning, memory/skills, and TUI integration; deterministic service lifecycle;
focused tests/snapshots; harness documents.

Forbidden areas: source archives, credentials, installed binary, real
LaunchAgent before confirmation, ordinary session history, oracle/reference
trees, and unrelated dirty-tree work.

Runtime state affected: owner-only profile source/index/review/proposal state;
after separate exact confirmations only, reviewed Dream memory, reviewed
auto-distilled skills, an atomically replaced installed binary with rollback,
and a per-profile user LaunchAgent.

## Design and Invariants

- Stream no-follow JSONL under 10,000 files/1 GiB aggregate, 64 MiB per file,
  1 MiB per line, depth 64, and 8 KiB per normalized message.
- The first `session_meta` owns file identity/workspace/parent. Only user input
  and completed assistant `final_answer` text survive; all other record kinds,
  media, incomplete turns, control text, and secret-shaped content are skipped.
- Stable item/content identities deduplicate active/archive moves and forked
  history. Provider prompts receive workspace digests, never source paths.
- The index stores metadata, hashes, processed identities, review manifests,
  and generated proposals, never raw transcript bodies. Source drift revokes a
  pending evidence authorization.
- The compatibility evidence preview still binds selected revisions,
  sanitized byte/call bounds, outputs, priced route, and estimated spend.
- The autonomous path uses the same completed-turn, secret, workspace, bound,
  and deduplication rules, then exports immutable intents to the enrolled
  supervisor. Evidence is acknowledged only after terminal replay rejection or
  successful journaled memory/inert-skill publication.
- The old indexing-only LaunchAgent is migrated away. One stable supervisor
  owns the recurring tick and the separately sandboxed execution profile;
  credentials remain external references.

## Milestones

### M1 — Streaming scanner and persistent index

- Add characterization fixtures for modern Codex record phases, forks,
  append/move/dedup, bounds, symlinks, secrets, workspace partitioning, and
  unknown records.
- Add no-follow streaming scan, exact previews, owner-only atomic state, and
  incremental source registration/sync/status CLI.

### M2 — Reviewed evidence and inert proposals

- Add exact evidence manifests, bounded corpus materialization, priced route
  preview, explicit provider authorization, generated proposal persistence,
  second review, selective application, recovery, and undo integration.

### M3 — TUI and recurring macOS service

- Add `/learn` manager/sync/review states and reviewed wide/narrow snapshots.
- Add deterministic 15-minute RunAtLoad LaunchAgent preview plus exact
  install/status/uninstall using the shared trusted lifecycle pattern.

### M4 — Validation and delivery approvals

- Run focused/default/all-feature/format/strict-Clippy/release/smoke/snapshot
  and protected-tree gates.
- Dry-run both supplied roots without profile/source mutation, then present the
  installed-binary, source/service, evidence, and generated-proposal approval
  boundaries in order.

## Acceptance Criteria

- The real archive inventory can be streamed despite exceeding 64 MiB, while
  raw text is absent from index/debug/status output and sources remain intact.
- Repeated scans learn only newly completed unique messages; moves/forks and
  unknown records do not duplicate or widen evidence.
- No provider request occurs before an exact evidence approval and no memory or
  skill changes occur before a second proposal approval.
- User/workspace evidence rules and the existing 32-memory/two-skill per-run
  limits remain authoritative.
- The installed service invokes only the absolute installed binary without a
  shell, scans locally, and cannot perform semantic or application work.

## Validation

Focused: PASS. Twenty-two archive-learning tests cover bounds, no-follow
streaming, first metadata, completed turns, secret/malformed/future records,
same-size/same-mtime drift, active/archive/fork deduplication, workspace
partitioning, owner-only state, route/cost binding, inert provider generation,
individual proposal apply/reject, fake `launchctl`, and background sandboxing.
The CLI/TUI command tests and six reviewed archive-learning wide/narrow pager
snapshots also pass.

Completion: PASS. `cargo test --no-fail-fast --quiet` passed 1,695 main tests
and 38 CLI tests with one manual-only ignore. The all-target/all-feature run
passed 1,724 main tests, 118 experiment tests, and 38 CLI tests with two
manual-only ignores. Formatting, strict all-target/all-feature Clippy, the
all-feature release build, release help/version, `git diff --check`, snapshot
hygiene, and oracle/reference immutability all pass.

Manual/visual: the reviewed wide/narrow status, evidence/cost, proposal, and
failure snapshots are accepted. The final read-only real-source preview saw
93 files, 504,570,420 bytes, and 1,145 completed candidate messages; four files
were created in the live Codex roots after the initial 89-file observation.
It reported zero provider calls/profile writes and profile state was absent
before and after. Real provider processing, binary replacement, source/service
installation, and proposal application remain blocked on their planned exact
review boundaries.

## Progress

- [x] Existing import, learning, route, service, archive shape, and bounds characterized.
- [x] Streaming scanner, source registry, incremental index, and CLI complete.
- [x] Evidence/proposal review and application pipeline complete.
- [x] TUI manager and macOS service complete.
- [x] Full validation, dry-run, and harness reconciliation complete.
- [x] Autonomous archive-to-supervisor replay/publication wiring implemented
      and pre-activation validated under LRN-166.
- [ ] One exact service activation and live terminal workflow complete.

## Decisions

- Decision: preprocess directly into an archive evidence index rather than
  importing ordinary sessions.
  Evidence: the archive is about 444 MiB, fork-heavy, multi-workspace, and the
  ordinary importer/corpus would be lossy and mis-scoped.
  Consequence: session history stays clean and raw text remains transient.
- Decision: retain indexing-only commands as compatibility surfaces, but make
  the stable supervisor the sole recurring service.
  Evidence: the user explicitly authorized one activation followed by
  continuous provider evaluation and memory/inert-skill publication.
  Consequence: routine work has no approval prompts, while the enrolled
  knowledge-only contract remains narrower than general runtime authority.

## Surprises and Discoveries

- Modern Codex assistant messages expose `phase=commentary|final_answer`; the
  old importer accepts both, so archive learning needs a stricter adapter.
- Sixty-two of 89 files are child sessions, making stable item-level dedup and
  parent grouping necessary before semantic processing.
- An intermediate preview saw 89 files / 469,585,296 bytes and 1,129 completed
  candidate messages. The final preview saw 93 files / 504,570,420 bytes and
  1,145 candidates because these are live Codex roots and four sessions were
  added during implementation. Both previews created no profile state.
- The exact final source-preview digest is
  `678ed9cd9d64677d503e14d53b84afb07f981c57ed33607101beab3f873e74e2`;
  it must be regenerated if the live roots drift before registration.
- The deterministic default-profile service digest is
  `2dd381e8f24e24b38445f3d8d3ed1a398c819d8407029ad34602eb4ac1fc220f`.
- The validated release binary digest is
  `4bdfc475ded5d62f8f007c3b33009dad18635c9bf787eea8af49b3c463da75bc`;
  the currently installed binary digest is
  `ed19d8500b6fddab085d0d8abc2043bab7853829a8a0a0d58e607896da4e764c`.
- A direct LaunchAgent alone did not constrain ambient home/environment access;
  background sync now immediately re-execs under required macOS sandboxing
  with a scrubbed environment, read-only source roots, denied networking, and
  only its own index directory writable.

## Recovery and Rollback

Use targeted inverse patches only. Profile writes use create-new temporary
files, fsync, rename, and exact digests. Generated application reuses learning
journals/undo. Service uninstall removes only an exact matching plist; binary
replacement retains one verified rollback copy. Never reset or clean the dirty
worktree and never delete archive sources.

## Completion Record

Implementation is reconciled through LRN-166. This plan remains ACTIVE until
the exact service activation and live end-to-end workflow pass.
