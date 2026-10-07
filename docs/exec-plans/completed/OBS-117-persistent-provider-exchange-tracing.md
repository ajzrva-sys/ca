# OBS-117 — Persistent Provider Exchange Tracing

Status: COMPLETE
Owner/session: Codex / current reliability roadmap session
Last updated: 2026-08-16

## Goal

Provide explicit, default-off, persistent, redacted provider exchange traces for
every production model route, with bounded failure-isolated capture and safe
local inspection/deletion through `/trace` and `--trace`.

## Non-Goals

- No implicit saved trace setting or automatic retention/pruning.
- No session/export/history/request/usage schema changes.
- No capture of MCP, browser, Git, shell, email transport, or other non-model
  network traffic.
- No oracle parity claim and no oracle/reference execution or modification.
- No implementation of INI-118 through PRJ-126 in this plan.

## Evidence and Current Behavior

- `src/agent/client.rs::AgentClient::stream_adapter_request` is the common
  physical HTTP/retry/stream boundary for prepared provider requests.
- `src/request.rs::RequestKind` already identifies main turns,
  continuations, compaction, review, probes, goals, workflows, subagents,
  headless, service, scheduled, and email-gateway model requests.
- `src/request.rs::RequestStore` is process-only and intentionally keeps only
  the latest semantic prepared request; it is not a wire trace store.
- `src/runtime.rs::RuntimePaths::profile_data_dir` is the canonical
  profile-isolated persistent data boundary. Bare profiles deliberately lack
  persistence authority.
- The command registry and pager already provide searchable slash commands and
  bounded scrollable text surfaces. Existing destructive confirmations default
  to cancel.
- Baseline: clean `master` at `ae10730 feat: add focused context and terminal
  UX workflows`.
- Oracle repositories are read-only design evidence only; OBS-117 is an
  intentional `ca` extension and not Cursor parity work.

## Scope

Allowed files:

- `src/provider_trace.rs`, `src/agent/client.rs`, `src/runtime.rs`
- CLI/service/TUI orchestration and focused tests/snapshots under `src/`
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`
- this ExecPlan and reviewed snapshot fixtures

Forbidden files/areas:

- `oracles/`, `reference/`, runtime user config/sessions/credentials
- provider request semantics, retries, canonical history, session or export
  schemas
- installed binaries, branches, worktrees, stashes, preservation directories

Runtime state affected:

- Only when explicitly enabled, owner-only JSONL files below
  `RuntimePaths::profile_data_dir(profile)/traces/`.
- Trace files persist until an explicit confirmed deletion.

## Design and Invariants

- A process trace hub is installed at the runtime boundary and shared by every
  `AgentClient`. Runtime on/off affects existing and future clients without
  changing prepared requests or provider continuations.
- Each activation creates one unique versioned JSONL trace. Records receive a
  monotonic sequence before entering one bounded asynchronous writer, preserving
  the accepted record order across concurrent requests.
- The writer uses create-new regular files, rejects symlink trace directories
  and entries, applies owner-only permissions on Unix, and enforces a 16 MiB
  file cap. Channel overflow, I/O, serialization, cap, or redaction uncertainty
  disables only that trace and exposes one sanitized diagnostic.
- Transport capture records typed request kind, request/attempt identity,
  elapsed time, sanitized final method/URL/headers/body, status, retry decision,
  bounded raw frames, decoded provider events, and terminal outcome.
- URL userinfo is removed and query values are replaced. Authentication,
  cookies, account IDs, configured/runtime secrets, secret-shaped JSON fields,
  encrypted reasoning, and binary/media payloads are never persisted.
- `/trace delete` validates a UUID-derived exact regular file immediately before
  deletion and defaults to cancel. No recursive or pattern deletion exists.
- Bare mode refuses `--trace` and `/trace on`; tracing never becomes a config
  field.

## Milestones

### M1 — Characterize boundaries and storage safety

- Add failing tests for runtime paths, CLI/command classification, store
  permissions/symlinks/truncated JSONL/list/open/delete, cap, and redaction.
- Expected: tests describe the absent trace boundary and fail before production
  implementation.
- Validation: focused `provider_trace`, `trace_`, and runtime tests.

### M2 — Persistent trace service and provider transport capture

- Implement the versioned bounded writer, store, redactor, global hub, and
  transport attempt/frame/event/outcome instrumentation.
- Exercise all provider protocols, retry paths, concurrent ordering, failures,
  and adversarial content with loopback-only fixtures.
- Validation: focused provider/client/trace tests.

### M3 — CLI and TUI lifecycle

- Add `--trace` to standard/service launches and `/trace` management commands,
  pager output, cancel-first deletion, diagnostics, and wide/narrow snapshots.
- Validation: focused CLI, command, input, orchestration, render, and snapshot
  tests.

### M4 — Completion and evidence

- Run local focused/full/all-feature/format/Clippy/release/smoke/diff/snapshot
  gates, an offline disposable loopback walkthrough, and physical FreeBSD exact
  source gates when available.
- Reconcile architecture/harness docs and archive this plan only after results
  are recorded.

## Acceptance Criteria

- Tracing is off by default and never written to config.
- `--trace` works for TUI, headless, and service launches; bare mode refuses it.
- `/trace`, `on`, `off`, `list`, `open <id>`, and cancel-first `delete <id>` are
  searchable, bounded, sanitized, and deterministic.
- All production `RequestKind` routes crossing `AgentClient` are traceable, and
  provider behavior is unchanged when capture succeeds or fails.
- Each physical retry is recorded in order with its final sanitized request,
  response/retry decision, raw bounded stream data, decoded events, and outcome.
- Trace directories/files are owner-only, symlinks are refused, each trace is
  capped at 16 MiB, and there is no automatic pruning.
- Traces remain outside sessions, exports, canonical history, `/request`, usage,
  and continuation state.

## Validation

Focused:

- Initial `cargo test trace_`: expected FAIL because
  `RuntimePaths::provider_trace_dir` and `CommandId::Trace` did not exist.
- `cargo test provider_trace --no-fail-fast`: PASS, 16 tests.
- `cargo test trace_ --no-fail-fast`: PASS, 14 tests.

Completion:

- `cargo test --no-fail-fast`: PASS, 1,379 main + 36 CLI; one documented
  manual case ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,408 main
  + 118 experiment + 36 CLI; two documented cases ignored.
- Formatting, strict all-feature Clippy, all-feature release build,
  help/version/service-help, diff check, and snapshot hygiene: PASS.
- Local arm64 release SHA-256:
  `bc363d34e5355e52c8dcc3534b848b1c7b0e0f641b79073a381010139f85929b`.

Manual/visual:

- PASS in a disposable home/repository with a loopback-only provider and
  synthetic credential. Headless `--trace` and TUI status/on/off/list/open/
  Cancel-first delete were exercised; owner-only files were created and the
  credential/raw request ID were absent on disk.
- Ten reviewed wide/narrow snapshots cover status, list, open, confirmation,
  and confirmation failure. The two provider command-palette snapshots have
  the intentional searchable `/trace` addition.

Blocked checks:

- None.

Physical FreeBSD:

- Exact 863-file source archive SHA-256:
  `124e80c6a6a1865025daadfec787017cc8c2276155c46119c375036a774dfccb`.
- Focused: PASS, 16 `provider_trace` and 14 `trace_` tests.
- Default: PASS, 1,366 main + 36 CLI; one documented ignore.
- All target/features: PASS, 1,395 main + 116 experiment + 36 CLI; two
  documented ignores.
- Native formatting, strict all-feature Clippy, release, help/version/service
  help, snapshot hygiene, ELF identity, and linkage: PASS.
- FreeBSD release SHA-256:
  `794bc65e7958e88a8f95a3cf8a8d1fd9356a9ec7bcc176abd5c0b18a9dfe012a`.

## Progress

- [x] Baseline and common provider transport/request-kind/runtime-path boundaries verified.
- [x] Active ExecPlan created and the ordered OBS-117–PRJ-126 roadmap recorded.
- [x] Failing characterization tests added and observed.
- [x] Trace store, sanitizer, writer, and provider capture complete.
- [x] CLI/TUI management and reviewed snapshots complete.
- [x] Local completion and disposable manual walkthrough complete.
- [x] Physical FreeBSD exact-source gates complete or honestly recorded blocked.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: instrument the common `AgentClient` transport and share one runtime
  hub rather than patching individual runners.
  Evidence: every prepared production model request uses the client transport;
  `RequestKind` carries route identity.
  Consequence: retries and protocol decoding are observed once without changing
  provider request construction.
- Decision: use a dedicated bounded writer and immutable create-new JSONL files.
  Evidence: synchronous file I/O cannot run in the TUI transport path, while
  append-only records must preserve accepted order.
  Consequence: trace backpressure disables capture instead of slowing or
  altering a provider turn.
- Decision: persist indefinitely until exact confirmed deletion.
  Evidence: explicit roadmap requirement.
  Consequence: list/status surfaces report size; no hidden retention policy is
  introduced.
- Decision: bind each in-flight request recorder to the trace activation that
  created it.
  Evidence: a provider stream may outlive `/trace off` and a later `/trace on`.
  Consequence: late frames/events are ignored after rotation and can neither
  contaminate nor disable the new trace.
- Decision: retain failed writer health until its one sanitized diagnostic has
  been consumed.
  Evidence: an asynchronous write can fail after `/trace off` removes the
  active writer.
  Consequence: late cap/I/O failures remain visible without retaining normal
  completed writers indefinitely.

## Surprises and Discoveries

- The final code review found that an unbound request recorder could otherwise
  write into a later activation. A failing rotation test now locks this
  boundary.
- Decoded provider text and incomplete tool-argument fragments need their own
  secret-shaped JSON check even after raw-frame sanitization. Adversarial tests
  now prove those fragments are replaced rather than persisted.
- The final sanitizer audit found that secret-shaped JSON embedded inside an
  otherwise ordinary JSON string also needs recursive provider-text handling.
  A failing nested-string regression now proves the unknown secret is removed;
  local and exact-source FreeBSD gates were rerun after the fix.
- The physical FreeBSD host's user-local `rustup` shim is a stale Linux ELF.
  The packaged native `/usr/local/bin/cargo-fmt` and `cargo-clippy` are usable;
  no host tool or configuration was changed.

## Recovery and Rollback

- Disable an active trace with `/trace off`; provider work continues regardless
  of trace state.
- Source rollback is a normal review/revert of the OBS-117 diff; no Git reset,
  stash, worktree, or runtime migration is required.
- Trace files are standalone versioned JSONL. A crash-truncated final line is
  ignored by the reader while prior complete records remain inspectable.
- Never delete trace directories recursively. `/trace delete <id>` targets one
  validated regular file only.

## Completion Record

Changed files: `src/provider_trace.rs`; provider integration in
`src/agent/client.rs`; runtime, CLI, service, TUI state/input/render and tests;
ten trace snapshots plus the intentional two command-palette updates;
`ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and this
plan.

Commands and results: all local focused/manual/completion and exact-source
physical-FreeBSD results are recorded above.

Remaining risks: trace content is intentionally sensitive diagnostic material
even after mandatory redaction, so activation remains explicit and files
persist until the user deletes them. The 16 MiB bound limits one trace, not the
number of explicitly created traces.

Harness updates: architecture, continuity, testing, index, and ordered roadmap
are reconciled. OBS-117 is complete; INI-118 remains queued and unselected.
