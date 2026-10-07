# FAL-061 — Explicit Provider Fallback

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-11

## Goal

Add explicit exact-pair provider fallback for interactive main turns. Fallback may occur once, only before usable output, only after the primary client's bounded retries are exhausted, and only for an allowed safe cause. The selected route is pinned through the entire tool loop, visibly persisted for that turn, and the user's configured/current primary is restored automatically for the next turn.

## Non-Goals

- Load balancing, health-based routing, provider races, fallback chains, automatic default changes, or per-model inference.
- Fallback on authentication/authorization/not-found, missing credentials, malformed/protocol/content/refusal errors, unavailable tools, or after text, reasoning, or a tool-call start/argument.
- Provider probes, `/btw`, checkpoints, reviews, verification, subagents, scripted tests, or headless automation. Those workflows deliberately validate or record an exact provider and need separate product contracts before silent route substitution.
- A fallback editor/TUI manager. This slice validates documented TOML and renders persisted per-turn provenance in the transcript.

## Configuration Contract

Add a default-empty top-level array of exact rules:

```toml
[[fallbacks]]
provider = "primary"
model = "primary-model"
fallback_provider = "backup"
fallback_model = "backup-model"
# optional; omission uses all four safe defaults
causes = ["connect", "timeout", "rate_limited", "server_error"]
```

- Provider/model fields are separate because model IDs may contain `/`.
- Both exact pairs must already be configured. Self-routes, duplicate primary pairs, duplicate causes, empty cause sets, and unknown entries fail config loading before runtime setup or traffic.
- Supported causes are intentionally closed: exhausted connect failure, exhausted timeout, HTTP 429, and retryable HTTP 5xx. Generic request errors and every 401/403/404/protocol/content cause remain ineligible.
- Rules are whole-config data and remain profile-scoped automatically. Bare reads the normal rules without persisting changes.

## Runtime Design and Invariants

- Add typed `ProviderFallbackCause`, `ProviderFallbackRule`, `TurnProviderRoute`, and defaulted `TurnProviderProvenance` boundaries. None contains endpoints, credentials, headers, bodies, or generated content.
- `start_llm_turn` establishes the primary route only for a new user turn. Tool continuations consume the process-only active route and never reevaluate fallback.
- The first request task resolves both primary and optional fallback tools/options against the selected model profiles. A fallback model explicitly marked tool-less or image-incompatible is ineligible for a request needing that capability.
- Track usable output in the task from canonical events only: nonempty text, nonempty reasoning, tool-call start, or nonempty tool-call arguments. Usage/finish/ping-like events do not count. Once usable output is observed, any error follows the existing explicit `/retry` path.
- On one eligible pre-output failure, finish the primary usage span as failed, emit a typed route-change event, reset only empty streaming state, begin a fallback usage span, prepare/record the fallback request, and continue within the same generation.
- Record one provenance entry keyed to the initiating user-message index. Render a derived sanitized marker before that turn's first assistant/streaming content. Persist the entry in defaulted Session V4 state; branches retain only entries whose initiating message remains in the branch.
- Never mutate `current_model`, `[defaults]`, provider config, model lists, reasoning selection, or catalog state. Clearing/completing/failing/cancelling the turn drops the active route; the next submitted user turn starts from `current_model` again.

## Milestones

### M1 — Config and classification

- Add failing serde/validation tests for defaults, exact configured pairs, duplicates, self-routes, unknown pairs, supported cause spellings, and immutable defaults.
- Add a pure safe error classifier proving only the four allowed exhausted causes can match.

### M2 — Turn routing and persistence

- Add active-route/provenance state, Session V4 defaulting/round-trip/branch filtering, transcript rendering, and cleanup invariants.
- Add orchestration tests for pre-output activation, primary success, partial-output refusal, continuation pinning, fallback failure, cancellation, next-turn restoration, and no default mutation.

### M3 — Loopback behavior

- Prove connect/timeout/429/5xx fallback and 401/403/404/malformed/partial-output non-fallback using credentialless loopback fixtures.
- Prove one fallback request, exact fallback model/protocol/tools/options, one pinned provider through a tool continuation, separate safe usage records, and visible persisted provenance.

### M4 — Delivery gates

- Run focused/full tests, format, snapshot hygiene, strict Clippy, release, help/version/profile smoke, and a disposable temporary-home TUI walkthrough.
- Reconcile the harness, archive this plan, and activate MCP-062 only after all gates pass.

## Acceptance Criteria

- Omitted config is behavior-identical and makes no extra request.
- Eligible failures fall back once before usable output and use the exact configured pair.
- Ineligible/partial-output failures retain the current safe error plus explicit `/retry` behavior.
- The chosen route stays pinned across tool continuations and the primary is used again for the next user turn.
- Fallback provenance survives save/resume/branch without changing canonical messages or defaults.
- No runtime secret or provider body enters config-derived rule state, events, provenance, snapshots, sessions, or debug output.

## Validation

Focused: PASS. Closed classifier/rule validation, config mutation preservation, Session V4 round-trip/branch filtering, interactive 503/429 activation, 401/403/404/protocol/partial-output refusal, cross-protocol request preparation, usage separation, tool-loop pinning, backup-failure no-chain behavior, and next-turn primary restoration all passed.

Completion: PASS. `cargo test --no-fail-fast` passed 707 unit and 28 CLI integration tests; `cargo fmt --check`, zero pending `.snap.new` files, strict Clippy, release build, and help/version/lazy-profile smoke all passed.

Manual/visual: PASS. Four persisted/streaming marker snapshots were reviewed at 100×30 and 60×24. The released TUI used a disposable home and credentialless loopback providers: the primary returned exactly two 503 responses, the visible marker named only the exact pairs and `server error`, and the backup returned `OK from backup`. `--no-session` created no session state. Temporary roots were moved to Trash.

## Progress

- [x] Active ExecPlan created before source implementation.
- [x] Config schema and safe classifier complete.
- [x] Interactive route pinning and fallback retry complete.
- [x] Provenance persistence/rendering complete.
- [x] Focused and repository gates pass.
- [x] Harness reconciled and plan archived.

## Recovery and Rollback

Use `apply_patch` to revert only FAL-061-owned schema/state/orchestration/render/test hunks. Because rules default empty and no migration is performed, rollback requires no user-state cleanup. Automated and manual validation use only temporary homes, fake configs, and loopback providers.

## Completion Record

Changed files: `src/fallback.rs`, `src/config.rs`, `src/app.rs`, `src/session.rs`, `src/main.rs`, `src/tui/render.rs`, `src/tui/render_tests.rs`, four `provider_fallback_*` snapshots, and the repository harness/plan documents.

Commands and results: focused fallback/config/session/orchestration/render tests PASS; full Cargo test PASS (707 + 28); format/snapshot hygiene PASS; strict Clippy PASS; release PASS; help/version/profile smoke PASS; isolated released-TUI loopback walkthrough PASS.

Remaining risks: Connect and timeout eligibility are covered by the pure closed classifier and the existing client deadline/retry tests, while the end-to-end fallback orchestration fixture uses 503/429 to stay fast and deterministic. Plan-approval continuation starts a new primary request by existing workflow semantics. Fallback intentionally does not apply to exact-provider automation or non-main runners.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `NEXT_STEPS.md`, `PARITY.md`, `TESTING.md`, and `docs/.INDEX.md` now record ownership, config grammar, validation evidence, extension status, and the `MCP-062` handoff.
