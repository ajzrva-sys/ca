# REQ-053 — Prepared Request and `/request` Inspector

Status: COMPLETE
Owner/session: `/root`
Last updated: 2026-08-11

## Goal

Route every model runner through one typed `PreparedModelRequest` boundary and retain exactly the latest process-only, bounded, secret-redacted semantic `RequestSnapshot` for a read-only `/request` inspector, without changing current Chat Completions wire behavior.

## Non-Goals

- Adding provider-protocol configuration, OpenAI Responses, Anthropic Messages, OAuth, fallback, deferred MCP discovery, or centralized tool selection; those remain PRV-057 onward.
- Persisting request snapshots, exposing raw HTTP bodies, endpoints, headers, credentials, environment values, binary bytes, or an unbounded transcript.
- Replacing `/context`, changing checkpoint thresholds, changing canonical/session messages, or claiming Hax/Cursor parity.
- Adding a request editor, replay button, export path, debug log, or headless event/schema change.

## Evidence and Current Behavior

- `src/agent/client.rs::ChatBackend` accepts loose message/tool slices plus options, while `AgentClient::chat_stream_with_options` separately injects model, stream, reasoning, endpoint, and authorization. There is no typed semantic request above provider serialization.
- Interactive main turns assemble exact system/message/tool state in `src/main.rs::start_llm_turn`; `/btw`, checkpoints, reviews, headless, scripted tests, provider probes, and `ProviderAgentRunner` each construct and send their own slices at separate call sites.
- `ContextBreakdown` retains only category estimates. It deliberately cannot show the actual latest system instructions, canonical provider messages, tool schemas, or per-request identity.
- `AppState` already supports safe read-only pager commands (`/context`, `/usage`) and can retain a process-only snapshot without changing sessions.
- Hax documents a model-facing transcript with the system prompt, schemas, tool arguments/results, and request statistics at `oracles/hax-master/docs/usage.md:241-249`. Its documented view is explicitly uncapped; REQ-053 adopts only the visibility goal and adds mandatory bounds/redaction. This is product evidence, not Cursor parity or observed Hax runtime behavior.
- Inferred: making production callers construct one owned request value before `ChatBackend` is the smallest characterization seam for later protocol adapters and tool selection while preserving the current serializer byte-for-byte.

## Scope

Allowed files:

- new `src/request.rs`; `src/agent/{mod,client,subagent}.rs`
- `src/{main,app,headless,review,provider_probe,test}.rs`
- `src/tui/{commands,render_tests}.rs`, focused tests, and deterministic snapshots
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, and this ExecPlan

Forbidden files/areas:

- `oracles/`, `reference/`, config/session schemas, provider endpoint/auth formats, real user runtime state, installed binary, external services, and unrelated future roadmap slices
- Raw request logging, unbounded snapshot content, persisted snapshots, binary retention, and provider-wire changes

Runtime state affected:

- One command-free, credential-free `RequestSnapshot` in a process-scoped store. It is replaced atomically when a model request is prepared and disappears at process exit.
- Validation uses fake backends and credentialless loopback servers under disposable homes only.

## Design and Invariants

- `PreparedModelRequest` owns request kind, safe provider/model IDs, the current fixed `chat_completions` protocol label, exact `ChatMessage` values, exact advertised `ToolDef` values, request options, and optional complete context estimates. It never contains endpoint, headers, or credentials.
- Add `ChatBackend::stream_prepared(&PreparedModelRequest)`. Every production runner constructs this value immediately before I/O and calls the new method. Existing loose methods remain compatibility shims for focused fakes/tests, but production call sites no longer use them.
- `AgentClient` validates the prepared model against its configured model, records the safe snapshot, then serializes the same model/messages/tools/options into the existing `ChatRequest`. Characterization compares serialized JSON and loopback bytes before/after the wrapper.
- `RequestKind` distinguishes main turn, tool continuation, side question, checkpoint, review, subagent, headless, scripted, and provider probe. A request snapshot records the kind that actually initiated the round trip rather than inferring from transcript shape.
- A cloneable `RequestStore` contains only the latest already-sanitized snapshot and a monotonic process-local sequence. `AppState` may hold it because the store never receives resolved credentials. Runtime clients receive the store plus their credential internally and build a snapshot before the request is sent.
- Snapshot generation redacts the exact nonempty resolved credential known to the runtime client and recursively masks structured fields named like authorization, API keys, passwords, secrets, or tokens. Redaction occurs before strings enter the store. Secret values, replacements, and lengths are not retained.
- Instructions are derived from system-role messages for display; non-system messages retain role, bounded safe content, tool-call name/arguments, and tool-result identity. This categorization does not change the exact message list sent to the provider.
- Data/image URLs and other binary-shaped content become media type plus `payload omitted`, never bytes or base64. Unknown JSON is rendered through recursive safe normalization. Terminal control text is sanitized.
- Snapshot structure is bounded independently from pager rendering: maximum item counts, bounded identifiers/text/schema fragments, a global semantic-text budget, exact omitted-item counts, and an overall 64-KiB rendered cap with a truthful omission marker. Estimates are deterministic char/serialized-byte estimates and explicitly labeled estimates.
- Tool snapshots include exact safe names/descriptions/parameter schemas within the budget. Request options expose only reasoning shape/effort, never provider-private transport state.
- `/request` is a visible read-only command available while work is active. Before the first prepared request it shows a deterministic empty explanation. Otherwise it opens the generic pager on one immutable point-in-time snapshot; opening it performs no MCP, provider, filesystem, or config work.
- Concurrent runners use last-prepared-wins ordering at the store lock. Completion order cannot overwrite a newer preparation because only preparation records snapshots.
- Snapshots never enter canonical messages, Session V4, headless output, usage records, debug formatting, or failure strings.

## Milestones

### M1 — Characterize requests and add the typed boundary

- Add failing tests that capture every runner's request kind/provider/model/messages/tools/options and characterize current Chat Completions JSON, omitted-empty-tools behavior, reasoning fields, and message/tool ordering.
- Implement `PreparedModelRequest`, the compatibility trait method, and migrate main/continuation, BTW, checkpoint, review, subagent, headless, scripted, and provider-probe call sites.
- Validation: focused caller tests prove no production runner invokes the loose request seam and loopback bodies remain equivalent.

### M2 — Build bounded secret-safe snapshots

- Implement the process-only store, safe snapshot types, exact/runtime-key plus structured-field redaction, terminal sanitization, media metadata substitution, item/text/global caps, and deterministic estimate calculation.
- Add adversarial tests for secrets in system/user/tool arguments/results/schemas, URL/header-like text, control sequences, base64/data URLs, deeply nested JSON, Unicode boundaries, excessive messages/tools, and clone/Debug non-leakage.
- Validation: serialized/rendered snapshots remain below bounds and contain none of the sentinels while the loopback request still contains the exact intended non-secret semantic content.

### M3 — Add `/request` and orchestration coverage

- Register the visible read-only command, add empty/latest pager rendering, and share the same process store with interactive main, reviews, checkpoints, provider checks, BTW, and provider-backed subagents.
- Add command/palette/orchestration tests for availability during work, no-I/O opening, latest-prepared ordering, tool continuation identity, and non-persistence across new processes/sessions.
- Add reviewed wide/narrow snapshots for empty, populated, omitted-content, binary-metadata, and redacted states.

### M4 — Complete gates and isolated walkthrough

- Run focused suites, full tests, formatting, strict Clippy, release build, help/version/profile smoke, and zero-pending-snapshot checks.
- In a disposable home/workspace with a credentialless loopback provider, inspect `/request` before I/O, after a main request, after one tool continuation, after `/btw`, and after a provider check. Confirm kind/provider/model changes, bounded safe content, exact loopback requests, and no session/config mutation from inspection.
- Reconcile the five harness documents, archive this plan, and activate TOL-054 only after every gate passes.

## Acceptance Criteria

- Every production model request path constructs `PreparedModelRequest` and calls `stream_prepared`; tests name every runner and fail if any direct loose call remains.
- Existing Chat Completions request bodies, message ordering, tools omission, reasoning fields, stream behavior, errors, cancellation, usage, and exit codes remain unchanged.
- The latest request snapshot correctly identifies kind/provider/model/protocol and shows bounded safe instructions, non-system messages, tool schemas, options, and explicitly estimated counts.
- Exact known runtime credentials, credential-shaped structured fields, endpoint/header data, base64/image bytes, control sequences, and adversarial sentinels cannot enter `RequestStore`, `Debug`, pager snapshots, sessions, usage, or headless output.
- Snapshot and rendered output caps are deterministic, UTF-8 safe, and truthfully report omitted items/content.
- `/request` is discoverable and usable while a turn is active, opens without external work, and reports a clear empty state before any request.
- Concurrent preparation is last-prepared-wins; stale completions cannot replace the snapshot.
- Focused/full gates and the isolated walkthrough pass without touching real runtime state, oracle files, external services, or the installed binary.

## Validation

Focused:

- `cargo test request --no-fail-fast`: PASS, 27 request-filtered tests.
- Provider-probe, review, scripted-backend, provider-subagent, and checkpoint request characterizations: PASS.
- Ten wide/narrow empty, populated, omitted, binary-metadata, and redacted snapshots: reviewed; zero pending snapshots.

Completion:

- `cargo test --no-fail-fast`: PASS, 617 unit tests and 25 CLI integration tests.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- Rebuilt help/version/profile smoke: PASS; missing named profile remained uncreated.

Manual/visual:

- PASS in a disposable bare-profile home/workspace with a credentialless loopback provider. `/request` showed no prepared request before I/O, then `tool continuation`, `side question`, and `provider check` after those exact operations.
- The server captured exactly four POST bodies: main and continuation each advertised 35 tools; BTW and probe advertised none. The inspector produced no traffic, config stayed exact, and no session file was created.

Blocked checks:

- None currently.

## Progress

- [x] Traced every current production request path, the existing ChatBackend serializer boundary, context estimates, pager commands, and Hax documentation evidence.
- [x] Created the active ExecPlan before implementation.
- [x] Added request-equivalence and every-runner characterization tests.
- [x] Implemented and migrated the prepared-request boundary.
- [x] Implemented bounded secret-safe request snapshots/store.
- [x] Added and reviewed `/request` states.
- [x] Passed focused and repository-wide gates.
- [x] Completed isolated walkthrough and harness reconciliation; archival follows this final plan update.

## Decisions

- Decision: keep Chat Completions as a fixed protocol label in REQ-053 rather than adding config protocol selection early.
  Evidence: PRV-057 separately owns provider protocol configuration and adapters.
  Consequence: this slice supplies the semantic boundary and inspector without changing transport compatibility.
- Decision: record at preparation, not completion.
  Evidence: the inspector describes what was sent; a request may fail, time out, or remain active without a completed response.
  Consequence: failed requests remain inspectable and concurrent ordering is deterministic.
- Decision: the process store owns only sanitized snapshots; runtime secrets remain inside clients during redaction.
  Evidence: resolved credentials are forbidden from manager state even when wrapped in a redactor type.
  Consequence: `/request` can read a shared store without acquiring credential authority.
- Decision: preserve loose ChatBackend methods as compatibility shims during this slice.
  Evidence: many hermetic fake backends implement the existing trait, while production routing can be enforced at caller sites and tested.
  Consequence: the migration stays local; PRV-057 may remove/replace the shims after adapter characterization.

## Surprises and Discoveries

- `ContextBreakdown` is assembled after asynchronous MCP catalog loading in the main TUI path, but reviews, checkpoints, subagents, probes, and headless requests have no equivalent retained semantic object.
- The current provider probe bypasses `ChatBackend` and calls the concrete client directly, so it requires an explicit prepared request rather than relying only on trait-call migration.
- The TUI constructs the provider-backed subagent client once at startup; sharing the request store there is required for later child requests to become the process's latest snapshot.

## Recovery and Rollback

- Code rollback is patch-only and limited to REQ-053-owned files. Do not use destructive Git commands.
- The compatibility methods remain available until every migrated caller and equivalence test passes, allowing individual call sites to be reverted without wire changes.
- Test/walkthrough artifacts use exact disposable roots and loopback processes. Stop exact PIDs and move only recorded roots to Trash after inspection.

## Completion Record

Changed files: `src/request.rs`; request construction/orchestration in `src/{main,app,headless,review,provider_probe,test,test_tests,doctor}.rs` and `src/agent/{client,client_tests,subagent}.rs`; command/render tests and ten snapshots under `src/tui/`; this plan and the five harness documents plus `docs/.INDEX.md`.

Commands and results: all focused and completion commands above passed. The final full suite contained 617 unit and 25 CLI tests.

Remaining risks: the boundary deliberately remains Chat Completions-specific until PRV-057; loose trait compatibility methods remain for hermetic fakes and direct client tests, while production callers use `stream_prepared`.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, and `docs/.INDEX.md` now record the safe request boundary, validation, and intentional-extension status.
