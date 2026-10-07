# Full Oracle-Informed Capability Program Audit

Audit date: 2026-08-12

This is the final requirement-to-evidence audit for the 49-item program accepted in the current task. It is not a Cursor or Ante parity claim. Oracle trees remain read-only evidence.

## ORC-218 Addendum — 2026-08-29

The remaining stable local guidance program is implemented and validated as
CA-owned behavior. This addendum records implementation evidence only; the
Ante, MiMo Code, Kun, ECC, Codex, Hax, and OpenCrabs bundles were not promoted
to runtime-parity authorities.

| Program | Result | Current evidence |
|---|---|---|
| AGT-157 | Implemented | Host-created receipts, deterministic artifact gates, serialized writable lanes, and aggregate autonomy budgets |
| DIA-219 | Implemented | Private bounded support bundles and secret-free configuration provenance |
| RES-220 | Implemented | Provider-neutral repetition and suspected-tool anomaly stops with partial-output preservation and no replay |
| REV-221 | Implemented | Two distinct isolated reviewers, bounded repair rounds, and immutable host-evidence acceptance packets |
| WFL-222 | Implemented | Immutable declarative research, fact-check, and experiment workflows with safe tree inspection |
| SYS-223 | Implemented | Reference-counted idle-only keep-awake and ownership-preserving combined job stop/wait |
| ART-224 | Implemented | Bounded metadata-only session artifact ledger and containment-revalidating browser |
| PAN-225 | Implemented | At most four pane-local interactive sessions with pane/generation-safe event routing |
| EDT-226 | Implemented | Stable ACP v1 explicit resources and drift-safe hash-anchored editing |
| IMP-227 | Implemented | Digest-bound transactional import of safe Claude instructions, rules, commands, and skills only |
| PRV-228 | Explicitly deferred | Documentation-only readiness record; no stable Responses WebSocket dialect is shipped |
| REL-229 | Complete | PRT-230 excludes exact Finder metadata; all code/build/protected gates pass and the release is recoverably installed with matching hashes and strict signature |

No paid provider request, hosted-service check, runtime profile/session
mutation, or oracle/reference edit was used to establish these results. The
full command evidence and the protected-state stop are recorded in
`TESTING.md` and `CONTINUITY.md`.

## Evidence Standard

An item is counted only when all of these are present:

1. its own archived ExecPlan with no unchecked milestone;
2. a current implementation seam in `src/` rather than a plan-only claim;
3. focused tests for the feature's authority and failure boundaries;
4. the plan's recorded isolated walkthrough or, for AUTH-060, the exact accepted official-documentation stop condition;
5. inclusion in the current repository-wide Rust, formatting, strict-lint, release, and smoke gates.

The final current-state gate supersedes the smaller historical test counts in individual plans. Those older counts remain useful evidence that the plans were delivered sequentially.

## Phase 1 — Daily TUI Workflow

| ID | Current implementation evidence | Focused and walkthrough evidence | Result |
|---|---|---|---|
| SAN-042 | `src/sandbox.rs`, `SandboxHubState`, `/sandbox`, `/config` routing, immutable effective versus saved mode | sandbox/config/input/render tests and ten reviewed snapshots; isolated next-launch/bare walkthrough in the [plan](exec-plans/completed/san-042-sandbox-hub.md) | Proven |
| CTX-043 | `ContextLimitEditorState`, field-wise `CaConfig` resolution and atomic exact-model updates, `/context-limit` | context/config/orchestration tests and ten snapshots; temporary-home save/reopen walkthrough in the [plan](exec-plans/completed/ctx-043-context-limit-editor.md) | Proven |
| USG-044 | `src/usage.rs::UsageLedger`, typed kinds/outcomes/token subsets/exact-decimal pricing, `/usage` | usage/parser plus every existing TUI runner test and four snapshots; failed/successful provider walkthrough in the [plan](exec-plans/completed/usg-044-usage-cost-ledger.md) | Proven |
| PRV-045 | `src/provider_discovery.rs`, exact 103-port literal-loopback scan, no-proxy/no-redirect client, Add Provider handoff | exact-port, credentialless, bounds, concurrency, reducer/render tests and eight snapshots; no-write discovery walkthrough in the [plan](exec-plans/completed/prv-045-local-provider-discovery.md) | Proven |
| BTW-046 | independent `BtwState`, generation/task/cancellation path, `UsageKind::Btw`, no-tool prepared request | protocol/frozen-context/non-persistence tests and ten snapshots; released concurrent-side-question walkthrough in the [plan](exec-plans/completed/btw-046-ephemeral-side-questions.md) | Proven |
| KEY-047 | `src/keymap.rs`, typed strokes/chords/conflict validation, `/keymap`, `/history`, dynamic labels | keymap/history/input/render tests and twelve snapshots; restart/bare walkthrough in the [plan](exec-plans/completed/key-047-keymaps-history-search.md) | Proven |
| SES-048 | `SessionV4` defaulted `pinned`/`queued_inputs`, `--no-session` capability gates, restored drafts that do not auto-submit | compatibility/queue/no-session/retry tests and no-session snapshots; resume and explicit-retry walkthrough in the [plan](exec-plans/completed/ses-048-session-v4-no-session-queue-persistence.md) | Proven |
| SES-049 | UUID-checked pin/archive/restore in `src/session.rs`, profile archive roots, protected current row | session/input/orchestration tests and archive snapshots; default/named-profile walkthrough in the [plan](exec-plans/completed/ses-049-archive-and-pinning.md) | Proven |
| SES-050 | bounded Markdown/normalized `ca.session-export.v1` in `src/session_portability.rs`, new-UUID sanitized import | format/bounds/symlink/unresolved-tool/overwrite tests; default/named-profile walkthrough in the [plan](exec-plans/completed/ses-050-session-import-export.md) | Proven |
| SES-051 | disabled `RetentionSettings`, protected current/pinned/archive filtering, preview-first batch deletion | retention/config/session/input/render tests; explicit preview/delete walkthrough in the [plan](exec-plans/completed/ses-051-session-retention.md) | Proven |
| HOK-052 | asynchronous `HookRunner`, complete seven-event lifecycle, sandbox/timeouts/process-group cleanup, `/hooks` repair surface | lifecycle/exact-once/fail-closed/cancellation/config/render tests and fourteen snapshots; fake-hook walkthrough in the [plan](exec-plans/completed/hok-052-complete-hook-lifecycle.md) | Proven |

## Phase 2 — Provider, Policy, and Tooling Foundations

| ID | Current implementation evidence | Focused and walkthrough evidence | Result |
|---|---|---|---|
| REQ-053 | `src/request.rs::{PreparedModelRequest,RequestSnapshot}` used by every `RequestKind`, bounded semantic `/request` inspector | runner-coverage/redaction/binary/depth/bounds tests and ten snapshots; loopback multi-runner walkthrough in the [plan](exec-plans/completed/req-053-prepared-request-inspector.md) | Proven |
| TOL-054 | `src/tool_selection.rs::ToolSelectionPolicy`, replacement/include/exclude resolver used across runner profiles | ordering/unknown/restrictive/MCP/CLI tests; flag-order walkthrough in the [plan](exec-plans/completed/tol-054-central-tool-selection.md) | Proven |
| APR-055 | `src/permission.rs::{PermissionRule,PermissionMatcher,PermissionDecision}`, deny/ask/allow precedence and process grants | glob/prefix/shell-bypass/manager/headless tests and snapshots; grant/remove/network walkthrough in the [plan](exec-plans/completed/apr-055-rich-permission-policy.md) | Proven |
| HED-056 | `src/headless_contract.rs`, verify-only `--check`, explicit repair authorization, sole-final bounded schema, `ca.headless.v2` | lifecycle/exit/schema/zero-I/O/repair tests; released loopback walkthrough in the [plan](exec-plans/completed/hed-056-stronger-headless-contracts.md) | Proven |
| PRV-057 | `src/agent/provider.rs::ProviderProtocol`, canonical request/events and adapter-owned Chat Completions serialization | byte-characterization, usage/tool/error/cancellation tests; isolated compatibility walkthrough in the [plan](exec-plans/completed/prv-057-protocol-neutral-provider-boundary.md) | Proven |
| PRV-058 | complete `OpenAiResponses` adapter and required `response.completed` lifecycle | request/SSE/reasoning/tool/result/usage/malformed/timeout tests; exact-request-hash walkthrough in the [plan](exec-plans/completed/prv-058-openai-responses.md) | Proven |
| PRV-059 | complete `AnthropicMessages` adapter, conditional `x-api-key`, system/tool translation and `message_stop` lifecycle | translation/usage/tool/error/body-free tests; credentialless released walkthrough in the [plan](exec-plans/completed/prv-059-anthropic-messages.md) | Proven |
| AUTH-060 | No direct OAuth code or credential import exists, as required when unsupported; API-key/env/Keychain/credentialless provider paths remain intact | fresh 2026-08-12 official manual still limits browser/device login to Codex-owned surfaces and general API traffic to Platform keys; app-server remains a separately scoped supported client boundary. The [plan](exec-plans/completed/auth-060-codex-chatgpt-authentication.md) records the accepted blocked-without-workaround result | Accepted stop condition proven |
| FAL-061 | `src/fallback.rs` exact primary/fallback pairs, allowed typed causes, one switch before usable output, persisted sanitized provenance | config/session/route/tool-loop/auth/partial-output tests and marker snapshots; released failover walkthrough in the [plan](exec-plans/completed/fal-061-explicit-provider-fallback.md) | Proven |
| MCP-062 | `McpTransport::StreamableHttp`, HTTPS/literal-loopback authority, environment header references, redirect/secret/network gates | HTTP/SSE/SSRF/redirect/header/cancellation/profile tests; released loopback walkthrough in the [plan](exec-plans/completed/mcp-062-streamable-http-mcp.md) | Proven |
| MCP-063 | `src/mcp_oauth.rs` discovery, PKCE/device, refresh rotation, pinned issuer/host and profile/server secret storage | browser/device/refresh/redirect/state/secret/failure tests and six snapshots; loopback OAuth walkthrough in the [plan](exec-plans/completed/mcp-063-mcp-oauth.md) | Proven |
| MCP-064 | lazy eligible-server startup plus bounded `search_tools` and process-local exact schema activation | activation/invalidation/collision/context/headless tests and four snapshots; released lazy-search walkthrough in the [plan](exec-plans/completed/mcp-064-deferred-mcp-tool-discovery.md) | Proven |
| LSP-065 | `src/lsp.rs::LspManager`, six semantic tools, configured executable only, UTF-16 conversion, workspace URI and cleanup bounds | fake-server positions/results/timeouts/cleanup/selection tests; released fake-LSP walkthrough in the [plan](exec-plans/completed/lsp-065-semantic-navigation.md) | Proven |
| DOC-066 | `read_pdf` typed registry/execution path with page, file, text, result and blocking-worker caps | range/encrypted/malformed/panic/action/output tests and four snapshots; released local-PDF walkthrough in the [plan](exec-plans/completed/doc-066-pdf-text-reading.md) | Proven |
| MM-067 | `src/image_content.rs::{ModelContent,ModelContentBlock,ModelImage}`, modality gates, normalized `view_image`, unavailable persistence markers | adapter/image-size/pixel/metadata/session/snapshot tests; released multimodal walkthrough in the [plan](exec-plans/completed/mm-067-typed-multimodal-image-inspection.md) | Proven |

## Phase 3 — Memory and Autonomous Work

| ID | Current implementation evidence | Focused and walkthrough evidence | Result |
|---|---|---|---|
| MEM-068 | `src/memory.rs::MemoryRecord`, separate user/workspace stores, disabled recall/generation, bounded indexes and approval-only proposals | scope/secret/provenance/stale/bounds/manager tests and snapshots; released proposal lifecycle walkthrough in the [plan](exec-plans/completed/mem-068-project-user-memory.md) | Proven |
| GOA-069 | durable `GoalState`, finite `GoalLimits`, fresh read-only evaluator and typed terminal outcomes | limit/evidence/approval/restore/usage/input/render tests and fourteen snapshots; released goal walkthrough in the [plan](exec-plans/completed/goa-069-goal-sessions.md) | Proven |
| WFL-070 | `WorkflowDefinition`, bounded TOML/JSON DAG validation, deterministic scheduler, grants/retries/outputs/evidence | cycle/64-step/retry/authority/restore tests and workflow snapshots; released DAG walkthrough in the [plan](exec-plans/completed/wfl-070-declarative-workflow-engine.md) | Proven |
| WT-071 | `src/worktree.rs` profile-owned registry, explicit create/status/diff/attach and clean confirmation-only cleanup | path/symlink/dirty/branch/attachment tests and snapshots; temporary-Git walkthrough in the [plan](exec-plans/completed/wt-071-worktree-lifecycle.md) | Proven |
| AGT-072 | public typed `AgentDependencyEdge` projected from `AgentTaskDefinition.depends_on`, grants/worktree/attempts/evidence and durable ready-node scheduler | cycle/dependency/edge/CAS/grant/evidence/retry/restore tests and fourteen snapshots; concurrent-root walkthrough in the [plan](exec-plans/completed/agt-072-dependency-aware-agent-graph.md) | Proven |
| RUN-073 | `src/multi_run.rs`, two-to-five isolated candidates, exact routes/worktrees, read-only judge and proposed fusion | isolation/cost/peer-failure/judge/fusion tests and fourteen snapshots; three-worktree walkthrough in the [plan](exec-plans/completed/run-073-multi-run-best-of-n.md) | Proven |
| SKL-074 | exact/alias/BM25 ranking in `src/skills.rs`, visible non-activating suggestions and unchanged hash activation | rank/tie/bounds/body-isolation/reload tests and eight snapshots; released activation walkthrough in the [plan](exec-plans/completed/skl-074-semantic-skill-suggestions.md) | Proven |
| CHG-075 | `src/change_walkthrough.rs`, one bounded exact Git snapshot, sole structured explanation and hash staleness | target/refusal/omission/stale/request tests and fourteen snapshots; two-hash walkthrough in the [plan](exec-plans/completed/chg-075-guided-changes-walkthrough.md) | Proven |

## Phase 4 — Session and Terminal Polish

| ID | Current implementation evidence | Focused and walkthrough evidence | Result |
|---|---|---|---|
| SES-076 | `SessionBundleMetadataV1`, append-only generation JSONL, bounded `SessionHistoryPage`, V1–V4 read/migrate path | 107 session-filtered tests and eight paging snapshots; 64/130-page released walkthrough in the [plan](exec-plans/completed/ses-076-lazy-history-storage.md) | Proven |
| SES-077 | process-only exact mutation receipts and paired undo/redo stacks with drift refusal and mutation invalidation | create/edit/delete/order/drift/symlink/capacity tests; released two-cycle walkthrough in the [plan](exec-plans/completed/ses-077-redo.md) | Proven |
| TUR-078 | explicit `/pause`/`/unpause` state at completed provider/tool/finalization boundaries | boundary/repetition/cancellation/input tests and six snapshots; released side-effect walkthrough in the [plan](exec-plans/completed/tur-078-soft-pause.md) | Proven |
| TRM-079 | `src/terminal_sessions.rs`, exact existing-tmux create/list/attach/detach/confirmed-kill and external ownership | argv/parsing/terminal-restoration/busy/error tests and fourteen snapshots; fake-tmux survivor walkthrough in the [plan](exec-plans/completed/trm-079-durable-terminals.md) | Proven |
| TUI-080 | `src/ui_settings.rs`, closed profile themes/status fields and bounded sandboxed custom status worker | config/preview/worker/cancellation/render tests and twenty-two snapshots; save/restart/bare walkthrough in the [plan](exec-plans/completed/tui-080-themes-and-status-surfaces.md) | Proven |

## Phase 5 — Service, Automation, and Managed Ecosystem

| ID | Current implementation evidence | Focused and walkthrough evidence | Result |
|---|---|---|---|
| SRV-081 | public `ServeOp`/`ServeEvent`, strict `ca.serve.v1` JSONL, process-only sessions, canonical requests, exact approvals/interactions | service lifecycle/framing/bounds/cancellation/cleanup tests; released stdio walkthrough in the [plan](exec-plans/completed/srv-081-jsonl-stdio-service.md) | Proven |
| SRV-082 | authenticated literal-loopback WebSocket and `ca attach`, protected file/environment token sources | auth/frame/overload/reconnect/cleanup tests; released listener/attach walkthrough in the [plan](exec-plans/completed/srv-082-loopback-websocket-and-attach.md) | Proven |
| SCH-083 | public strict `ScheduleDefinition`, disabled profile records, one-at-a-time atomic claims, finite workers and local/signed delivery | definition/cron/claim/worker/webhook tests and CLI integration; released scheduled prompt walkthrough in the [plan](exec-plans/completed/sch-083-scheduled-tasks.md) | Proven |
| PLG-084 | public strict `PluginManifest`, inert local registry, capability/collision previews and composition through existing extension boundaries only | manifest/digest/collision/profile/enable tests and CLI integration; released inert-to-enabled walkthrough in the [plan](exec-plans/completed/plg-084-local-plugin-bundles.md) | Proven |
| PLG-085 | pinned Ed25519 catalog, closed signed package/file hashes, staged disabled update/rollback | signature/revision/package/traversal/local-source tests and CLI integration; two-revision released walkthrough in the [plan](exec-plans/completed/plg-085-signed-curated-catalog.md) | Proven |
| BRW-086 | opt-in `src/browser.rs::BrowserManager`, ephemeral local Chromium/CDP, per-navigation network authority and bounded actions | fake-CDP/security/cleanup tests plus explicit ignored real-Chromium test; disposable-profile walkthrough in the [plan](exec-plans/completed/brw-086-browser-automation.md) | Proven |
| VOC-087 | disabled `/voice`, explicit Space capture, bounded process-local WAV, local/provider transcription and editable draft only | 22 fake capture/transcription/process/HTTP/usage tests; safe Ready-modal walkthrough in the [plan](exec-plans/completed/voc-087-voice-input.md) | Proven |
| EXT-088 | public safe `ManagedRuntime`, exact-source Ed25519 catalogs, per-file verification, retained versions and inert entrypoints | 10 trust/catalog/hash/path/cleanup/rollback tests; signed two-version walkthrough in the [plan](exec-plans/completed/ext-088-signed-external-runtime-manager.md) | Proven |
| LOC-089 | `src/local_models.rs`, exact private GGUF lifecycle and process-owned sandboxed loopback `ManagedLocalServer` | 9 header/hash/download/argv/readiness/log/cleanup tests; signed-runtime credentialless walkthrough in the [plan](exec-plans/completed/loc-089-managed-llama-cpp-and-gguf.md) | Proven |
| UPD-090 | `src/self_update.rs`, exact authority/manifests/staging/invoking-executable install and retained rollback | 9 trust/signature/network/drift/lock/install/rollback tests; copied-release two-request walkthrough in the [plan](exec-plans/completed/upd-090-explicit-self-update.md) | Proven |

## Public and Compatibility Contract

- Exact shared boundaries are present: `UsageLedger`, `PreparedModelRequest`, `RequestSnapshot`, `ToolSelectionPolicy`, `PermissionRule`, `ProviderProtocol`, API-key source/authentication state, fallback rules/provenance, `McpTransport`, model modalities, `ModelContentBlock`, `MemoryRecord`, `GoalState`, `WorkflowDefinition`, `AgentDependencyEdge`, `ServeOp`, `ServeEvent`, `ScheduleDefinition`, `PluginManifest`, and `ManagedRuntime`.
- Missing provider protocol remains Chat Completions; missing MCP transport remains stdio; missing model modality remains text-only/legacy-compatible. Existing raw TOML is not rewritten merely by loading.
- `SessionV4` remains defaulted and backward-readable through V1–V4. Lazy storage is a separately versioned commit format and migrates only on successful write.
- Default config leaves fallback, memory recall/generation, voice, session retention, and extension registries inactive. Browser requires explicit tool selection; schedules and plugins are created disabled; service, goals, workflows, worktrees, graphs, multi-run, local runtimes, and updates require explicit commands.

## Security and Scope Contract

- Current Cargo/source contain no dependency or runtime path into `oracles/` or `reference/`; the sole non-document occurrence is an inert parity fixture citation.
- Provider credentials, OAuth tokens, raw request endpoints/headers, image/audio/browser bytes, and plugin/runtime secrets are absent from safe manager/request/session structures and have dedicated redaction/body-free failure tests.
- No GUI, Cursor-hosted dependency, arbitrary marketplace URL, non-loopback listener, always-on microphone, automatic pruning, automatic runtime/model download, or automatic self-update was introduced.
- AUTH-060 remains deliberately unavailable because no official third-party direct OAuth/provider contract exists. The official Codex app-server is a distinct supported integration architecture and was not silently substituted.

## Final Gate

The 49-item program's final command results and corrections remain recorded in
`AUD-091` and `TESTING.md`. Later programs preserve unrelated active plans;
completion is determined by their own linked ExecPlans and gates, not by
requiring the repository-wide active-plan directory to be empty. ORC-218's
stable implementation rows are complete, while REL-229 correctly remains
active until its pre-existing protected-tree blocker is resolved and the
authorized installation is verified.
