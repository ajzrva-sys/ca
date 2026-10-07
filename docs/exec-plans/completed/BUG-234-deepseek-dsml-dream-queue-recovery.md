# BUG-234 — DeepSeek DSML and Dream Queue Recovery

Status: ACTIVE
Owner/session: current Codex session
Last updated: 2026-09-02

## Goal

Normalize the exact DeepSeek V4 DSML tool-call wrapper into CA's existing
structured tool-event path, and recover only authentic legacy supervised
learning intents whose sole defect is the former character-count sanitizer's
UTF-8 byte overflow.

## Non-Goals

- No generic executable-text tool protocol or typo-tolerant DSML dialect.
- No change to advertised-tool, approval, sandbox, or unavailable-tool policy.
- No live provider request, credential read, user configuration/session edit,
  or manual mutation of the user's learning profile during validation.
- No AUT-231 feature expansion beyond the formatter and strict-lint blockers
  the user explicitly added to this task.
- No edits under `oracles/` or `reference/`.

## Evidence and Current Behavior

- `src/agent/client.rs::stream_adapter_request` currently sends decoded provider
  events through `StreamAnomalyGuard` and then directly to turn assembly.
- `src/agent/stream_anomaly.rs` does not recognize the documented DSML
  `tool_calls` / `invoke` / `parameter` vocabulary. The screenshot-shaped
  wrapper therefore reaches transcript text instead of invoking `propose_plan`.
- `src/learning.rs::load_supervised_intents` validates every queued intent
  before a new intent can be queued. A pre-fix multibyte record that exceeds the
  byte bound therefore blocks later Dream runs with the reported sanitizer
  error.
- The worktree already contains unrelated modified and untracked work. Target
  files were inventoried before editing; AUT-231 and protected evidence trees
  remain outside this plan.
- External format evidence: DeepSeek's tool-calling guide and vLLM's DeepSeek
  V4 DSML parser. These define the exact delimiter and parameter string
  semantics; they are evidence only, not copied implementation.

## Scope

Allowed files:

- `src/agent/{mod.rs,client.rs,client_tests.rs,stream_anomaly.rs,dsml.rs}`
- `src/{config.rs,provider_contract.rs,main.rs,learning.rs,codex_learning.rs}`
- `src/{automation.rs,schedule.rs,session.rs}` for the user-authorized AUT-231
  formatter/lint repair and `src/agent/lifecycle.rs` for the test-only
  deterministic forged-handle correction exposed by the completion matrix
- Focused tests colocated with those modules
- `docs/exec-plans/completed/BUG-234-deepseek-dsml-dream-queue-recovery.md`
- `docs/exec-plans/completed/`, `docs/.INDEX.md`, `ARCHITECTURE.md`,
  `CONTINUITY.md`, `NEXT_STEPS.md`, `PARITY.md`, and `TESTING.md` only for
  durable BUG-234 reconciliation

Forbidden files/areas:

- `oracles/`, `reference/`, user runtime configuration, sessions, credentials,
  and live learning state
- AUT-231 milestones beyond the narrow completion-gate cleanup

Runtime state affected:

- Normal provider streams may gain canonical tool events only for the exact
  built-in DeepSeek V4 Chat Completions contract or a validated explicit model
  opt-in.
- On the next normal writable-profile launch, authentic legacy intents may be
  privately archived and atomically repaired. Validation uses temporary roots.

## Design and Invariants

- A bounded stream normalizer sits after wire decoding and before anomaly
  detection/turn assembly. It buffers an exact DSML-looking prefix so markup is
  never rendered transiently.
- Exact ASCII and full-width delimiters are accepted. A complete wrapper may
  contain ordered invocations and unique parameters. `string="true"` becomes a
  JSON string; `string="false"` must contain strict JSON.
- Complete calls become `ToolCallStart`, one complete JSON argument fragment,
  and `Finish(ToolCalls)` events. Host-generated IDs are unique. The existing
  tool loop remains the sole executor and policy boundary.
- Native structured calls pass through unchanged. Mixed native/DSML, malformed,
  incomplete, oversized, prose-mixed, and non-opted-in wrappers fail with a
  bounded protocol error. Fenced examples remain ordinary text.
- `tool_call_format = "deepseek_dsml"` is valid only for tool-capable
  Chat Completions model profiles. Exact built-in DeepSeek V4 remains automatic
  even when unrelated model attributes are overridden.
- Legacy recovery first validates owner-only regular-file access, V1 shape,
  enrollment, original immutable digest, filename/ID, structural bounds, and
  every unchanged safety rule. It proceeds only when current re-sanitization is
  the sole failure and rejects secret-shaped evidence.
- The exact original bytes are written once to a private digest-addressed
  archive. An identical existing archive is accepted; a collision fails closed.
  The corrected intent preserves its ID, recomputes byte totals and digest, and
  replaces the queue file atomically. Both archive and replacement are synced;
  reruns are idempotent.

## Milestones

### M1 — Failure-first characterization

- Add focused screenshot-shaped DSML and multibyte legacy-intent regressions.
- Confirm each fails against current behavior for the expected reason.
- Validation: focused test filters for the two new characterizations.

### M2 — DSML normalization and configuration

- Add bounded normalizer/parser and integrate it before anomaly detection.
- Add validated public model-profile opt-in and exact built-in activation.
- Cover delimiter/chunk/call/parameter/error/native/fenced/provider behavior.
- Validation: focused agent, provider, config, and loopback tests.

### M3 — Narrow durable learning recovery

- Add structural legacy classification, private archival, atomic replacement,
  startup/harvest/queue integration, and bounded recovery status.
- Cover repair, export, unchanged ID, archive fidelity, idempotence, digest
  drift, collision, unsafe evidence, malformed data, and symlinks.
- Validation: focused learning, harvesting, and TUI tests.

### M4 — Integrated validation and reconciliation

- Run both serial workspace matrices, formatting, strict all-feature Clippy,
  all-feature release, command smokes, signature, diff/snapshot/protected-tree,
  and orphan-process checks.
- Record actual outcomes and reconcile project harness documents.
- Installation stops on any failed gate.

### M5 — Recoverable installation, only if M4 is clean

- Preserve the installed binary as a timestamped full-SHA recovery copy.
- Stage on the destination filesystem, atomically replace, and verify candidate,
  staged, and installed SHA-256 equality, ownership, mode `0755`, strict
  signature, path lookup, version/help, and representative offline commands.
- Restore the prior binary atomically if any integrity or smoke check fails.

## Acceptance Criteria

- The screenshot-shaped `propose_plan` wrapper never appears in transcript or
  session content; exactly one structured plan call executes and its result is
  continued to the provider.
- Exact full-width/ASCII markers, Unicode byte splits, multiple calls, string
  parameters, and strict JSON parameters normalize in order.
- All malformed, incomplete, oversized, duplicate, prose-mixed, mixed-native,
  unadvertised, and non-opted-in cases preserve existing fail-closed policy.
- Native events and fenced DSML examples retain prior behavior.
- Exact built-in DeepSeek V4 Chat Completions is automatic; compatible proxies
  work only with the validated explicit opt-in; generic routes do not execute or
  display DSML.
- One authentic legacy multibyte intent is archived byte-for-byte and repaired
  with unchanged ID, corrected totals/digest, successful export/completion, and
  idempotent rerun. Tampered, unsafe, symlinked, malformed, or non-legacy inputs
  remain untouched and rejected.
- Startup and later queueing report only a bounded recovery count and are no
  longer blocked by the authentic legacy record.

## Validation

Focused:

- Targeted agent/client/stream/config/provider/learning/harvest/TUI tests.

Completion:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`
- `cargo test --workspace --all-targets --all-features --no-fail-fast -- --test-threads=1`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo build --workspace --release --all-features`
- Release help/version and representative offline command smokes
- Strict code-signature verification
- `git diff --check`, zero `.snap.new`, protected-tree verification, dirty-tree
  audit, and orphan-process checks

Manual/visual:

- Deterministic transcript/TUI coverage for DSML holdback and bounded recovery
  status; no live provider call.

Blocked checks:

- Any credentialed or live-provider check is intentionally prohibited.

## Progress

- [x] Repository harness, relevant call paths, dirty-tree boundaries, and
  external DSML format evidence inspected.
- [x] Failure-first DSML and legacy-intent characterizations recorded.
- [x] DSML normalizer, route activation, and configuration validation complete.
- [x] Legacy intent recovery and writable entry-point integration complete.
- [x] Focused and completion validation recorded.
- [x] Harness reconciled for implementation and pre-install validation.
- [x] Release recoverably installed after every gate passed.

## Decisions

- Decision: automatic DSML applies only to the exact built-in DeepSeek V4 Chat
  Completions contract; other compatible endpoints require model-profile opt-in.
  Evidence: user-selected scope and the provider contract's exact model/endpoint
  resolver.
  Consequence: generic endpoints fail closed without broad executable-text
  heuristics.
- Decision: recovery keeps the intent/run ID and archives immutable original
  bytes before replacement.
  Evidence: queue identity is referenced by status/completion paths, while a
  digest-addressed archive makes the migration auditable and idempotent.
  Consequence: no queued work is silently recreated, deleted, or renumbered.
- Decision: after the user explicitly expanded scope, clear AUT-231's formatter
  and strict-lint blockers by connecting the existing due-claim path and
  removing a redundant receipt field; keep the remaining AUT work paused.
  Consequence: the repository-wide gates can run without suppressions or
  claiming AUT-231 feature completion.

## Surprises and Discoveries

- The current sanitizer already byte-bounds new multibyte evidence; only stored
  pre-fix records require migration.
- AUT-231's unused due-claim method had an intended scheduler-service call path;
  wiring it removed dead code without weakening lint policy. The unused public
  receipt field duplicated an internal execution count and could be removed.
- The first all-feature matrix exposed a one-in-sixteen test-only flake: forging
  a capability by replacing its first hex digit with `0` is a no-op when it
  already starts with `0`. Selecting the opposite digit made the mutation
  deterministic; the clean replacement matrix passed.

## Recovery and Rollback

- Source rollback uses a narrow inverse patch for BUG-234 files; no destructive
  Git command is permitted in the dirty worktree.
- Learning tests use disposable roots. Production migration retains an exact
  private archive and is not invoked manually during validation/install.
- Installation retains a timestamped full-SHA prior binary; a failed integrity
  or smoke check restores it with an atomic rename.

## Completion Record

Changed files: `src/agent/{mod.rs,client.rs,client_tests.rs,stream_anomaly.rs,dsml.rs}`,
`src/{config.rs,provider_contract.rs,main.rs,learning.rs,codex_learning.rs}`,
`src/{automation.rs,schedule.rs,session.rs}` (AUT-231 formatter/lint repair),
`src/agent/lifecycle.rs` (test-only deterministic forged-handle fix), focused
colocated tests, and the reconciled harness pages.

Commands and results: focused DSML/stream/client/config/provider/learning
coverage passes; both serial workspace matrices pass (default CA 2,395/4
ignored, all-feature CA 2,424/4 ignored, experiment 118/1 ignored, codex-team
652/1 ignored, MCP stdio 8/8, all other targets); `cargo fmt --all -- --check`,
strict all-target/all-feature Clippy, and the all-feature release pass; help,
version, and offline smokes pass; strict signature, `git diff --check`, zero
`.snap.new`, protected-tree verification, and orphan checks pass.

Remaining risks: no live DeepSeek request ran (intentionally prohibited); the
DSML path is proven by deterministic loopback and fenced/native coverage.

Harness updates: `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`,
`ARCHITECTURE.md`, `PARITY.md`, and `docs/.INDEX.md` reconciled. The candidate
SHA-256 `60727a1c249dbef2984f32b07a0a4c53dfc59321b5be64eb5067b2b5087ad566`
was recoverably installed to `~/.local/bin/ca` with a timestamped full-SHA
recovery copy, matching candidate/staged/installed hashes, ownership, mode
`0755`, strict signature, path lookup, and version/help/offline smokes.
