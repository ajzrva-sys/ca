# REL-147 — Route-Aware Token Estimates and Relevant Memory Recall

Status: COMPLETE
Owner/session: Codex / current implementation session
Last updated: 2026-08-19

## Goal

Prevent automatic checkpointing from relying on the unsafe UTF-8-bytes/4
estimate, and select bounded recalled memory for the current request rather
than truncating persisted insertion order.

## Non-Goals

- Do not make automatic provider token-count requests or add paid/network
  traffic to request preparation.
- Do not claim an exact local tokenizer where an official model contract does
  not identify one, vendor provider tokenizer assets, or apply one tokenizer
  family to OpenAI, MiMo, and DeepSeek indiscriminately.
- Do not add vector embeddings, model downloads, a persistent search index, or
  a memory/session schema migration.
- Do not change memory validation, scope, approval, Dream/CAS ownership, or
  provider context-window configuration.

## Evidence and Current Behavior

- `src/context.rs` estimates text, serialized values, and canonical messages as
  UTF-8 bytes divided by four. `start_context_checkpoint` compares the resulting
  breakdown with the configured/default 80% threshold, so the estimate affects
  control flow rather than display alone.
- The request inspector labels the value as estimated, but no estimator quality
  or resolved-route distinction reaches context accounting.
- Official OpenAI documentation exposes an authenticated
  `POST /responses/input_tokens` count endpoint, but automatic network counting
  would add traffic and cannot protect an already-near-limit request. The
  current GPT-5.6 model pages do not publish a local tokenizer encoding.
- Official DeepSeek documentation publishes a separate offline tokenizer
  package and explicitly notes model-dependent tokenization. MiMo publishes
  usage totals but no reviewed stable local tokenizer contract. Inferred: CA
  must keep exact tokenizers behind an internal route/model seam and use an
  honestly labelled conservative local estimator until a tokenizer is safely
  bundled and certified for an exact current model.
- `src/memory.rs::render_recall_index` walks persisted records in insertion
  order and stops at 200 lines or 25 KiB per scope. `MemoryRecall` stores the
  resulting strings and accepts no request query. Approved in-process mutations
  rebuild recall; external file edits are not watched during an active session.
- The store is capped at 512 records and CA already uses deterministic local
  BM25-style ranking for skill suggestions, so memory retrieval needs no vector
  or persistent-index dependency.

## Scope

Allowed files: context estimation/accounting, memory recall projection,
headless/service/interactive request assembly, focused tests, and durable
project documentation.

Forbidden files/areas: `oracles/`, `reference/`, user configuration, memory or
session stores, credentials, installed binaries, Git history, and unrelated
provider/request behavior.

Runtime state affected: none during implementation or automatic validation.

## Design and Invariants

- Add an internal estimator selected from the exact resolved first-party
  endpoint/protocol/model contract, with a generic route for custom providers.
  Estimator quality is explicit. No route is labelled exact unless backed by a
  certified local tokenizer; unsupported routes use the same deterministic,
  conservative lexical/BPE approximation behind separate family identities.
- Conservative estimation counts punctuation, long/high-entropy identifiers,
  Unicode scalars, and framing more heavily than English prose. It must never
  return less than the legacy bytes/4 estimate and must remain linear, bounded,
  allocation-light, deterministic, and network-free.
- Automatic checkpoint decisions and prepared context category totals use the
  resolved attempt route, including a fallback route. Legacy helper APIs remain
  generic for non-control estimates and compatibility.
- `MemoryRecall` retains cloned, already-validated scoped records in process and
  renders per request. It never logs query or record text and changes no stored
  order or schema.
- Per scope, reserve at most 4 KiB/eight records for the most recently updated
  broadly applicable user preferences/workflows or workspace conventions/
  decisions. Fill the remaining existing 25 KiB/200-line budget with positive
  deterministic BM25 matches from the current request, ordered by score,
  coverage, recency, then UUID. A record is emitted whole or skipped.
- Query selection is local and pure. Empty/no-term queries retain only the
  always-applicable reserve; external facts require a positive lexical match.
  User and workspace limits remain independent.

## Milestones

### M1 — Characterize both failures

- Add failing route-estimator tests for exact first-party selection, proxy
  isolation, conservative code/JSON/Unicode behavior, and no legacy
  undercount.
- Add failing memory tests proving a late relevant record beats older unrelated
  records, broad preferences survive lexical misses, scopes remain independent,
  and private text stays out of Debug.

### M2 — Implement bounded local estimation and retrieval

- Implement the route/model estimator seam and thread it through automatic and
  prepared context accounting.
- Replace prompt-facing pre-rendered memory with per-request deterministic
  scoped ranking while preserving storage and mutation behavior.

### M3 — Integration and completion

- Cover headless, service, interactive, side-question, fallback-route, request
  inspector, context display, and memory refresh integration.
- Run focused and repository gates, reconcile durable docs, and archive this
  plan.

## Acceptance Criteria

- Official OpenAI/Codex, MiMo, and DeepSeek exact routes receive distinct
  estimator identities; named custom proxies remain generic. No estimator
  performs network I/O or falsely claims exactness.
- Code, JSON, high-entropy identifiers, and multilingual fixtures estimate no
  lower than legacy bytes/4 and trigger checkpoint decisions conservatively.
- A relevant record appended beyond the old recall cutoff is selected for a
  matching request, while unrelated earlier records do not crowd it out.
- Broad preference/convention records retain a small deterministic reserve;
  external facts without a positive match are not injected.
- Existing scope, byte/line, validation, approval, Dream/CAS, persistence,
  session, and provider behavior remains compatible.
- No secret, memory text, or retrieval query enters Debug, errors, traces,
  snapshots, or usage records.

## Validation

Focused: context estimator/checkpoint/request tests; memory/storage/learning
tests; headless/service/orchestration integration; relevant render snapshots if
visible labels change.

Completion: `cargo test --no-fail-fast`, `cargo test --all-targets
--all-features --no-fail-fast`, `cargo fmt --all --check`, `cargo clippy
--all-targets --all-features -- -D warnings`, `cargo build --release
--all-features`, help/version, snapshot hygiene, `git diff --check`, and
oracle/reference diff checks.

Manual/visual: deterministic tests only unless a disposable no-session
loopback walkthrough materially adds evidence.

Blocked checks: exact live provider counts and external tokenizer assets are
outside automatic validation and require explicit provider-specific authority.

## Progress

- [x] Read the harness, inspect current estimator/recall paths, and verify the findings.
- [x] Add failing characterization tests.
- [x] Implement route-aware conservative estimation.
- [x] Implement query-aware bounded memory recall.
- [x] Run focused and completion validation; reconcile documentation.

## Decisions

- Decision: do not add a global `cl100k_base` tokenizer.
  Evidence: the three supported provider families do not share one published
  local encoding contract.
  Consequence: exact tokenizers remain an injectable internal future extension,
  while control flow immediately becomes more conservative and route-aware.
- Decision: use an in-memory per-request lexical index rather than SQLite FTS or
  vectors.
  Evidence: the validated store is bounded at 512 records and no persistence is
  needed for deterministic BM25 scoring.
  Consequence: no model download, network grant, index migration, or stale
  derived database exists.

## Surprises and Discoveries

- The working tree already contains extensive user-owned changes, including
  Dream/CAS additions in `src/memory.rs` and dependency changes in
  `Cargo.toml`/`Cargo.lock`. REL-147 will patch the current versions surgically
  and will not reset, reformat, or normalize unrelated changes.
- A common stopword (`the`) initially made an unrelated external fact a
  positive lexical match in the broad-reserve test. The bounded tokenizer now
  drops a small fixed English stopword set before BM25 scoring; broad records
  remain governed by their separate reserve.
- The initial fallback implementation recomputed message/tool categories but
  retained primary-route system/skill numbers. Prepared attempts now estimate
  the complete rendered system, skill component, transient controls, tools,
  canonical messages, and attachments with the same attempt estimator.

## Recovery and Rollback

Revert only REL-147 hunks with targeted patches. Do not reset the dirty
worktree, restore files from `HEAD`, accept snapshots broadly, or discard the
existing Dream/CAS and provider work.

## Completion Record

Changed files: `src/context.rs`, `src/memory.rs`, `src/main.rs`,
`src/service.rs`, the internal family accessor in `src/provider_contract.rs`,
five `/context` snapshots, this plan, and the durable harness pages.

Commands and results: focused context (24), memory (13), and service (22)
suites pass; enabled-memory, mutation-refresh, side-question, fallback, and
context-dashboard suites pass. Default completion passes 1,651 main and 37 CLI
tests with one ignore. All-target/all-feature completion passes 1,680 main,
118 experiment, and 37 CLI tests with two ignores. Formatting, strict
all-feature Clippy, all-feature release, help/version, diff/snapshot hygiene,
and oracle/reference checks pass.

Remaining risks: current estimators are deliberately conservative rather than
exact and can checkpoint early. Exact local counts require separately reviewed
model tokenizer assets/contracts. External owner-managed memory-file edits are
still observed only when recall is rebuilt or the process restarts.

Harness updates: `ARCHITECTURE.md` records estimator/retrieval ownership and
invariants; `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `docs/.INDEX.md`
record completion and evidence. No live provider/tokenizer traffic, runtime
state mutation, installation, native TUI walkthrough, or oracle/reference
access occurred.
