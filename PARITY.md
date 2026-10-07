# Parity Contract

## HRS-252 Apple Container execution backend

Design-only capability boundary (`docs/HRS-252-apple-container-execution-backend.md`).
Optional VM execution backend for jobs/`run_command`, separate from `SandboxMode`.
Hermes PR #78035 and read-only Hermes environment-tree patterns are behavioral
evidence only; no oracle implementation is copied or required. There is no clean
Cursor fixture for container/VM terminal backends, so **no Cursor runtime parity
is claimed**. Acceptance would be CA-owned policy→argv, mount, network-deny, and
lifetime fixtures (plus optional native integration), not oracle equivalence.
Row remains **Unverified** until HRS-252 is activated and those tests exist.
Do not promote this row from source inspection or from Hermes PR status.

## PLN-244 review-only card

The follow-up screenshot is evidence of unwanted persistent plan display, not
instructions to execute the pictured work. The user's clarification limits the
expanded default to review; stored plans remain accessible through `/plan show`.
CA-owned rendering/lifecycle fixtures define acceptance. No oracle changes or
Cursor parity claim.

## PLN-243 expanded plan default

The user requests that the plan be shown by default. The screenshot is display
evidence only. CA's inline card now shows all tasks and complete wrapped entries,
verified with CA-owned wide/narrow fixtures. This is a requested CA behavior;
no Cursor runtime parity is claimed and no oracle implementation is copied.

## BUG-242 Astra provider repair

The user screenshot is failure evidence, not operational instructions. This
CA capability repair uses official model documentation at
https://developers.openai.com/api/docs/models/gpt-6-astra and allowlisted local
Codex model metadata (reasoning/default/context/modalities only). It makes no
Cursor parity claim and copies or changes no oracle implementation. Codex's
ultra metadata describes automatic delegation; this repair exposes documented
API wire efforts low through max, not that separate orchestration mode.


## PLN-241 user-requested plan repair

The supplied screenshots are failure evidence, not operational instructions.
The plan card, reader, and current-agent Execute transition implement the
user's explicit CA expectations. Acceptance is based on CA-owned reducer,
request/tool, input, rendering, session, and local fake-provider terminal
fixtures. No oracle source is copied or changed and no Cursor runtime parity
claim is made. The existing protected-tree baseline discrepancy is separate.

## BUG-239 native Codex catalog compatibility

This is the user's explicit CA provider integration repair, not a new Cursor
parity requirement. Read-only Codex protocol observations at
`oracles/codex/codex-rs/codex-api/src/endpoint/models.rs` and
`oracles/codex/codex-rs/protocol/src/openai_models.rs` establish the native
versioned request and `models`/`slug`/list-visibility envelope. CA independently
implements a minimal decoder; no oracle source or remote instruction text is
copied into agent behavior. The synthetic seven-visible-model fixture is
`tests/fixtures/codex-model-catalog.json`. A real CA-authenticated catalog-only
check returned the same seven visible IDs and matching picker entries. Other
providers/custom endpoints and model capability profiles remain unchanged.
The pre-existing protected-tree baseline discrepancy is not repaired here.

## SES-236 user-requested fresh sessions

`/new` implements the user's explicit fresh-session expectation. Read-only
Hermes evidence (`oracles/hermes-agent-main/hermes_cli/commands.py` and
`cli.py::new_session`) corroborates new identity/history and preserving prior
messages before rotation. No implementation was copied and no full Hermes or
Cursor runtime parity is claimed. CA retains `/clear` separately and keeps
save failures in place rather than discarding the outgoing conversation.

## BUG-235 CA-specific provider recovery boundary

The one-safe-replay policy and automatic replacement branch are CA reliability
behavior, not a Cursor parity claim. The supplied screenshot is failure
evidence only. Source inspection identified a dangling failed user message
that ordinary submission previously followed with another user message; the
exact provider-side reason for HTTP 400 remains unproven because error bodies
remain deliberately discarded. HTTP 400 is not retried, and no silent input
truncation was added. Acceptance uses hermetic physical-request, event-order,
immutable-session, cancellation, and wide/narrow render fixtures. No oracle or
reference implementation was copied or modified.

## BUG-234 DeepSeek protocol evidence boundary

DeepSeek's public tool-calling guide and the reviewed vLLM DeepSeek V4 parser
define the exact DSML delimiter and parameter semantics used by CA's bounded
normalizer. They are wire-format evidence, not implementation source or a claim
of broad provider parity. Automatic activation is limited to CA's exact
built-in DeepSeek V4 Chat Completions contract; compatible proxies require an
explicit validated model-profile opt-in, and generic endpoints fail closed.
The Dream repair is a CA-owned migration for its historical sanitizer and makes
no oracle parity claim. No `oracles/` or `reference/` byte changed.

## ORC-218 remaining-oracle guidance boundary

ORC-218 adopts stable local engineering guidance from the reviewed Ante, MiMo
Code, Kun, ECC, Codex, Hax, and OpenCrabs evidence only through CA-owned
contracts and fixtures. Those sources are guidance, not claims of observed
runtime parity. CA retains its stricter privacy, authority, provider, plugin,
session, and no-hosted-service boundaries.

Responses WebSocket support is explicitly not a parity requirement or shipped
capability. The dated PRV-228 record observes that official OpenAI WebSocket
event schemas remain under a Beta Responses resource, while CA's supported
stable Responses adapter remains HTTP/SSE. Reopening requires a separately
authorized stable wire contract and the complete safety/accounting evidence in
`docs/RESPONSES_WEBSOCKET_READINESS.md`.

All approved stable ORC-218 children are implemented and pass CA-owned focused
and integrated fixtures. This establishes CA feature completion, not observed
runtime equivalence with an oracle product. Exact `.DS_Store` components are
now excluded under the versioned protected-tree algorithm without changing any
oracle bytes. The exact manifest-only commit establishes HEAD trust, and
REL-229's recoverable installation is verified.

## CUR-204 local feature evidence boundary

CUR-204 implements CA-native local capabilities corresponding to features found
in the clean Cursor `2026.07.09-a3815c0` bundle: named multi-root workspaces,
conditional rule and command managers, background promotion and completion,
local plugin management, explicit direnv and shell integration, one-shot launch
and worktree controls, runtime diagnostics, terminal presentation, ACP v1, and
bounded automation flags. The bundle is implementation evidence only because it
is not runnable in the clean harness. CUR-204 therefore makes no claim of
observed Cursor runtime parity.

Acceptance is established by CA-owned fixtures and contracts. CA intentionally
retains stricter behavior where the evidence does not establish an adequate
trust boundary: no arbitrary remote plugin marketplace; no automatic `.envrc`
evaluation; no unapproved repository setup; no detached OS-daemon jobs; no
hosted authentication or ACP v2; metadata-only diagnostics; sanitized,
scheme-restricted terminal links; and bounded ephemeral history, Git, and
environment inputs. Existing default CLI/session/provider bytes remain the
compatibility authority when the new opt-in surfaces are absent.

## ASY-187 process watcher extension

`ca-watch`, typed local/SSH PID registration, and the unified token-free
background activity surface are `ca` extensions. No clean Cursor fixture
establishes this exact process identity, copy-marker, fixed-SSH, approval, or
provider-isolation contract, so ASY-187 makes no Cursor parity claim. Existing
adopted shell-job lifecycle, interactive approval, `/jobs`, cancellation, and
TUI layout behavior remains authoritative where the extension shares those
surfaces. The extension is accepted by CA-owned fake-transport, tool-catalog,
approval-identity, provider-count, event-order, and deterministic snapshot
tests; no oracle implementation was read as executable instruction or copied.

## LRN-171 direct adaptive learning extension

The supplied MiMo interaction is behavioral evidence only: direct invocation,
adaptive local investigation, smallest-form asset selection, and its compact
Inventory/Shortlist/Created/Skipped/Needs-more-evidence report are reproduced
without copying oracle source. CA safety remains authoritative: typed bounded
retrieval replaces arbitrary shell/SQLite access; private evidence is excluded;
automation is recommendation-only; publication is provenance-bound, journaled,
and undoable. The legacy archive CLI remains compatibility-only for this TUI
path.

## SEC-246 approval/trust/sandbox CLI Observed notes

Cursor Agent CLI `agent-cli-local` `2026.09.15-d2fe57e` help text (study bench;
no Cursor account) Observed 2026-09-17 via `agent-local --help`:

- `--trust` — Trust the current workspace without prompting
- `-f/--force` / `--yolo` — Run Everything (exclusive with auto-review in that CLI)
- `--auto-review` — Smart Auto / server classifier (not CA behavior)
- `--sandbox enabled|disabled`
- `--mode <plan|ask>` / `--plan` — plan: read-only planning; ask: Q&A read-only
- Headless `stream-json` emitted `tool_call started|completed` with shell parse/timing fields
- `--resume [chatId]` / `--continue` / `persist` list|attach|stop; headless resume retained prior-turn memory by `session_id`

CA acceptance remains CA-owned fixtures (Ask zero-tool resolution, Plan profile
proposal tools, launch-mode status strings, HeadlessEventV1 ToolStart/ToolFinish
shapes, slash description/model-alias tests, SessionStore resume-memory,
CLI exclusivity). This is **not** a Cursor TUI parity claim; startup/composer,
approvals, slash/pickers, diff/tool cards, and tool result loop rows stay
Unverified.

## Target

`ca` targets observable Cursor Agent CLI/TUI behavior in a local Rust implementation. “Parity” is demonstrated by behavior and fixtures, never by copying bundled code or matching internal architecture.

## In Scope: Must Match

For features intentionally implemented by `ca`, preserve the user-observable contract of:

- startup, prompt entry, streaming, cancellation, and completion states;
- conversation/tool-call ordering and status transitions;
- approval prompts and allow/deny effects;
- slash commands, aliases, pickers, keybindings, and mode transitions;
- tool argument/result presentation, diffs, errors, truncation, and expansion;
- session save/resume/fork/rewind behavior where supported;
- layout behavior at representative terminal widths and with Unicode input/output;
- CLI flags and headless stdout/stderr/exit behavior that the project explicitly adopts.

A feature is not parity-complete until the exact reference behavior, `ca` fixture, and acceptance check are recorded.

`ca schedule` is an oracle-informed `ca` extension. No clean Cursor fixture establishes generic scheduled-task management, isolated workers, or signed delivery, so SCH-083 makes no Cursor parity claim.

`ca plugin` is an oracle-informed `ca` extension. No clean Cursor fixture establishes this local declarative bundle/activation registry, so PLG-084 makes no Cursor parity claim.

`ca plugin catalog` is an oracle-informed `ca` extension. No clean Cursor fixture establishes pinned signed catalogs or staged plugin lifecycle, so PLG-085 makes no Cursor parity claim.

`run_privileged` is a security-motivated `ca` extension. The optional OpenCrabs
oracle informed only the general masked-callback and `sudo -S` pattern; no
clean Cursor fixture establishes SEC-154/SEC-155's exact-command digest, one-use
consent, helper/executable trust checks, private process boundary, or streaming
secret redaction. The Linux/FreeBSD extension therefore makes no Cursor parity
claim and does not change the existing generic approval or job-input parity
rows.

The opt-in `browser` tool is an Ante-informed `ca` extension. Ante documentation describes an approval-gated CDP Browser category, but no Ante runtime observation or clean Cursor fixture establishes this exact tool contract, so BRW-086 makes no parity claim.

Explicit `/voice` capture and transcription are a `ca` extension. The reviewed Ante documentation does not describe voice input and its hosted API documentation explicitly excludes audio endpoints; no clean Cursor fixture establishes this interaction, so VOC-087 makes no parity claim.

`ca runtime` is an Ante-informed `ca` extension. Ante documentation describes pinned checksum-verified managed llama.cpp builds, but no Ante runtime observation or clean Cursor fixture establishes this generic exact-source, signed multi-file lifecycle, so EXT-088 makes no parity claim.

`ca local`, managed GGUF storage, and explicit process-owned llama-server runs are Ante-informed `ca` extensions. Ante documentation describes managed offline llama.cpp loading and local model selection, but no Ante runtime observation or clean Cursor fixture establishes this exact storage, supervision, or process-only provider contract, so LOC-089 makes no parity claim.

`ca update` is an Ante-informed `ca` extension. Ante documents explicit updates, channels, and rollback, but no Ante runtime observation or clean Cursor fixture establishes `ca`'s exact Ed25519 authority, closed-manifest, staged current-executable replacement, or retained-byte rollback contract. UPD-090 therefore makes no parity claim.

OPT-152's deterministic hosted-model retrieval guidance, semantic model-facing
command condensation, and cache/latency efficiency observations are `ca`
extensions. No clean Cursor fixture establishes these exact prompt or reporting
contracts, so this work makes no runtime parity claim; default bounded command
results and existing adopted interaction behavior remain unchanged.

LRN-166's continuous native/Codex evidence harvesting, cross-provider replay,
knowledge-only supervisor, and automatic memory/inert-skill publication are
`ca` extensions. The implementation reuses locally characterized LRN-134,
LRN-150, and INT-151 contracts; it does not treat Codex archives or any oracle
implementation as executable instructions, and no Cursor parity claim is made.
Only `ca`'s own deterministic bridge, publication, service, and safety tests are
acceptance evidence for this extension.

LRN-170's MiMoCode-style reviewed learning pass is also a `ca` extension. The
read-only MiMoCode oracle and user-provided observations establish behavioral
ideas—raw trajectory evidence, existing-asset inventory, one coherent Distill
analysis, high-confidence creation, and Created/Skipped/Needs-more-evidence
reporting—but no oracle implementation was copied and no Cursor parity claim is
made. Native fake-provider, SQLite, publication, TUI, and safety tests are the
acceptance evidence; CA retains exact approval, inert portable-skill, journal,
and undo boundaries that differ from MiMoCode internals.

`docs/FULL_CAPABILITY_AUDIT.md` is an implementation-completion matrix, not parity evidence. It proves the accepted `ca` extension program against current source/tests/walkthrough records while preserving each feature's existing evidence label; it does not upgrade any Ante-derived or source-derived behavior to verified Cursor parity.

## Allowed to Differ

- Rust/ratatui/tokio instead of Cursor’s JavaScript/Ink implementation.
- Internal type/module names and provider client internals.
- Config/session storage details when not visible in an adopted contract.
- Local OpenAI-compatible providers instead of Cursor-hosted authentication and transport.
- Extra diagnostics or tools when they do not alter an adopted Cursor interaction and are documented.
- Codex-inspired testing or architecture patterns.

## Explicitly Out of Runtime Scope

The Cursor oracle documents Connect/protobuf endpoints, compression, TLS, model-loading calls, and proxy experiments. `ca` currently speaks an OpenAI-compatible provider API directly. Reproducing Cursor's proprietary Connect service topology is not a parity requirement unless the user explicitly expands the product scope. Keep those findings as research evidence; do not redirect `ca` toward `cursor-proxy` or make its Go code authoritative for this Rust runtime.

## Must Not Be Imported

- Cursor source, minified bundle fragments, certificates, auth bypasses, tokens, or proprietary network behavior.
- Oracle build artifacts or nested dependencies.
- Cursor-hosted service requirements.
- Codex product behavior presented as Cursor parity.

## Evidence Order

1. Direct observation of a clean Cursor Agent binary/version with exact input, terminal size, and captured output.
2. Clean backup artifacts under `oracles/cursor-CLI-reverse/cursor-agent/backup/` corroborated by behavior.
3. Extracted module names/strings and screenshots in the Cursor oracle.
4. Reverse-engineering reports such as `COMPLETE_FINDINGS.md` and `FINAL_STATUS.md`, treated as hypotheses where they conflict.
5. `reference/` snippets and historical project specs.
6. Codex oracle material for engineering lessons only.

The live `cursor-agent/` oracle tree is patched. Never use it as sole proof of stock behavior.

## Oracle Map

| Evidence | Use | Caveat |
|---|---|---|
| `oracles/cursor-CLI-reverse/cursor-agent/*.index.js` | discover module/command/tool UI names and protocol clues | minified; live files may be patched |
| `oracles/cursor-CLI-reverse/cursor-agent/backup/2026-07-14/` | compare clean bundled artifacts | still implementation evidence, not behavioral proof |
| `oracles/cursor-CLI-reverse/cursor-agent/COMPLETE_FINDINGS.md` | locate known experiments and files | findings evolved and can conflict with `FINAL_STATUS.md` |
| `oracles/cursor-CLI-reverse/cursor-agent/FINAL_STATUS.md` | understand failed offline experiments | not a product parity specification |
| `oracles/cursor-CLI-reverse/cursor-proxy/` | protobuf/transport experiment evidence | not a `ca` dependency or target architecture |
| `reference/*.js` | older extracted UI snippets | incomplete, secondary evidence |
| `oracles/codex/codex-rs/tui/` | snapshot/state-machine/testing patterns | different product; never parity authority |

## Transferable Codex Lessons

Use, without copying product behavior:

- ratatui `TestBackend` plus text snapshots at wide/narrow sizes;
- component-local state-machine documentation, as shown by `tui/src/bottom_pane/AGENTS.md`;
- snapshot coverage for resume pickers, diffs, status, markdown, streaming, and Unicode;
- default terminal colors, restrained semantic color roles, and theme-safe output from `tui/styles.md`;
- focused tests first, then crate-level format/lint/build gates.

## Parity Matrix

Maintain one row per claimed slice. `Observed` means direct reproducible evidence exists; `Characterized` means a local fixture exists; `Matched` means both pass.

| Slice | Cursor evidence | ca implementation | Test | Status |
|---|---|---|---|---|
| startup/composer | historical spec and extracted UI only | `src/tui/render.rs`, `src/tui/input.rs` | hermetic startup plus wide/narrow composer snapshots | Unverified |
| streaming lifecycle | bundle clues | `src/main.rs`, `src/agent/client.rs` | byte-split SSE/assembler failures plus streaming/retry snapshots | Unverified |
| tool result loop | protocol clues | interactive + headless paths | canonical serialization, fake-provider continuations, and CLI tool-loop subprocesses | Unverified |
| approvals | tool UI names/bundle clues | `src/main.rs`, `src/approval.rs`, TUI | ordered exactly-once queue/policy tests, headless refusal, and wide/narrow snapshots | Unverified |
| slash commands/pickers | bundle strings + old spec | `src/tui/input.rs`, `src/tui/render.rs` | centralized registry/input tests plus command/picker snapshots | Unverified |
| diff/tool cards | extracted module names | `src/tui/render.rs` | temporary-Git service tests plus wide/narrow diff/tool snapshots | Unverified |
| sessions | old spec | `src/session.rs`, resume overlay | temp-root V1-V4 lifecycle/branch tests plus resume/timeline snapshots | Unverified |
| Unicode/narrow layout | no captured Cursor fixture yet | input/render paths | scalar-boundary key tables plus 100x30/60x24 snapshots | Unverified |

`PAR-005` attempted the clean evidence lane on 2026-08-09. The dated wrapper was locally modified and the clean bundle was not runnable as a standalone CLI under a network-denied empty-home environment. The secret-free blocked record is `tests/fixtures/parity/cursor-runtime-blocked-2026-08-09.md`. No row is promoted; a stock versioned Cursor runtime is still required.

The test column records current local characterization, not Cursor evidence.
Every row remains `Unverified` because no clean, versioned Cursor observation
exists; local coverage alone cannot promote a row to `Observed` or `Matched`.

## Intentional `ca` Extensions

The completed oracle-informed roadmap does not turn secondary oracle patterns into Cursor parity claims. Until a clean Cursor fixture is captured, these are documented `ca` terminal behaviors:

- stable version-2 JSON sessions, searchable metadata, non-destructive timeline branching, and checkpoint persistence;
- explicit inactive-session deletion from `/sessions` (`/resume` alias), including active-session protection and non-cascading branch behavior. Ante documentation informed the workflow idea; no Cursor fixture or Ante runtime behavior is claimed;
- stable-ID queued drafts with editing/reordering, busy Enter/Ctrl+S interrupt-and-send,
  explicit Alt+Enter queueing, and Esc/Ctrl+C stopping (TUI-240). Cancellation holds
  pending drafts until explicitly sent/requeued, with success-only draining for
  released drafts. This is a user-requested CA extension, not an exact Codex/MiMo
  keymap or Cursor parity claim;
- Session V4 queued-draft persistence and the `--no-session` run capability. Ante session/workflow documentation informed the product priority; no clean Cursor fixture or Ante runtime observation is claimed;
- profile-scoped session pinning and Active/Archive organization in `/sessions` (`/resume` alias), including exact one-file transfer, current-session protection, and no branch cascade. Ante session-organization ideas informed the priority; no clean Cursor fixture or Ante runtime observation is claimed;
- title-first `/sessions` presentation with date groups, updated times, exact saved-session rename, best-effort first-turn generated names, and explicit per-row legacy-name repair. Read-only MiMoCode behavior informed the title-generation and interaction hierarchy; `ca` retains its own Recent/Archived organization, immediate local fallback, stale-result guards, and UUID-checked persistence, and no Cursor parity or copied oracle implementation is claimed;
- bounded Markdown and normalized JSON session export plus strict inert-history import with fresh profile-scoped UUIDs. MiMo/OpenCode and Kun documentation informed the product idea; no clean Cursor import/export fixture or oracle runtime behavior is claimed;
- optional saved-session age/count retention through an exact Cancel-first `/retention` preview, with current/pinned/archived protection and no automatic pruning. Hax and Hermes documentation informed the vocabulary and dry-run idea; no clean Cursor fixture or oracle runtime observation is claimed;
- lazy saved-history storage through a stable metadata commit point and generation-scoped append-only JSONL log, bounded `/sessions` paging, full-history semantic gates, and exact-pair lifecycle operations. Oracle long-session ideas informed the priority; no clean Cursor lazy-history fixture or oracle runtime observation is claimed;
- bounded process-only `/undo` and `/redo` for exact built-in UTF-8 file mutations, including live-state drift refusal and new-mutation redo clearing. Oracle editing workflow ideas informed the priority; no clean Cursor redo fixture or oracle runtime observation is claimed;
- process-only `/pause` and `/unpause` at completed provider, tool, and finalization boundaries, with exact-once continuation and cancellation kept distinct. Oracle turn-control ideas informed the priority; no clean Cursor soft-pause fixture or oracle runtime observation is claimed;
- model-generated non-destructive context checkpoints, complete category-level `/context` estimates, and derived transcript checkpoint markers. Ante documentation informed the reporting shape; no Ante runtime behavior or Cursor parity is claimed;
- the read-only full-screen “Working tree changes” list/diff review, including tracked-only diff opening;
- process-scoped asynchronous foreground/background jobs, job-control tools, and the `/jobs` list/detail surface.
- explicit `/term` management for existing user-installed tmux, including user-owned session survival, exact create/attach/detach/confirmed-kill operations, and safe terminal restoration. Ante terminal documentation informed the priority; no clean Cursor durable-terminal fixture or Ante runtime observation is claimed.
- profile-scoped `/theme` and `/statusline` managers with closed built-in palettes, ordered safe fields, and an optional bounded network-denied custom command. Oracle appearance/status-surface ideas informed the priority; no clean Cursor configurable-theme/statusline fixture or oracle runtime observation is claimed.
- ranged truthful file reads, independent job stream omission reporting, bounded WebFetch decoding, and labelled generic/headless clipping. Ante documentation informed the bounded-output priority; no Cursor fixture or Ante runtime behavior is claimed.
- the `/provider` (`/providers`) management hub, including provider settings, model browsing, targeted catalog refresh, confirmed provider deletion, the explicit editable OpenAI Responses template, and the searchable audited 40-row provider-template catalog shared with `/provider setup`. The catalog is a `ca` extension derived from public provider documentation; the clean Cursor oracle has no generic provider manager or provider-template fixture. Only its searchable `/model` command-to-picker interaction informed the UI shape.
- the explicit `/provider setup` credential walkthrough, typed health results, repair flow, and passing-provider finish chooser. Cursor's `/mcp` list/detail and `/config` failure-in-place patterns informed the interaction, but no generic Cursor provider-check command or fixture was found.
- the offline `/doctor` and `ca --doctor` readiness report, plus explicit `--connect` provider checks. Ante documentation informed the diagnostic shape; this is not a Cursor parity claim or an Ante runtime observation.
- the searchable `/config` hub and dedicated review-model selection. Ante's documented settings dialog informed the product idea, but no Cursor fixture or Ante runtime observation is claimed.
- immutable named whole-file run profiles and the capability-restricted `bare` profile. Ante documentation informed the product idea; no Ante runtime observation or Cursor parity claim is made.
- the `/sandbox` manager and `/config` integration, including saved/effective mode separation, backend/root/network summaries, next-launch-only persistence, and Cancel-first weaker-mode confirmation. Oracle settings/sandbox surfaces informed the idea; no clean Cursor sandbox-manager fixture or Ante runtime observation is claimed.
- the `/context-limit` editor and `/config` integration, including exact model targeting, independent inheritance, validation-in-place, and between-turn persistence. Ante settings documentation informed the daily-workflow priority; no clean Cursor fixture or Ante runtime observation is claimed.
- the process-only `/usage` ledger, including provider/model request provenance, typed completion/failure/cancellation outcomes, cached/reasoning token subsets, and exact optional configured cost. Oracle usage surfaces informed the idea; no clean Cursor fixture or Ante runtime observation is claimed.
- the explicit `/provider discover` loopback scanner and Add Provider handoff. It is a `ca` safety/product extension with fixed local authority; no generic Cursor local-provider discovery fixture or Ante runtime observation is claimed.
- the process-only `/btw [question]` side-question overlay, frozen protocol-complete context, independent generation/cancellation/usage state, and deliberate exclusion from canonical messages and sessions. Oracle workflow ideas informed the priority; no clean Cursor `/btw` fixture or Ante runtime observation is claimed.
- the profile-scoped `/keymap` manager, typed one/two-stroke global composer bindings, fixed safety controls, dynamic shortcut labels, and process-only `/history` draft restoration. Oracle shortcut/settings patterns informed the priority; no clean Cursor configurable-keymap or Ante runtime fixture is claimed.
- the asynchronous interactive main-session hook lifecycle and command-free `/hooks` manager, including exact Pre/Post event boundaries, bounded sandboxed execution, fail-closed startup repair, explicit testing, and descendant cleanup. Codex/ECC hook documentation informed engineering risks only; no Cursor hook-manager fixture or oracle runtime parity is claimed.
- the typed prepared-model-request boundary and process-only `/request` semantic inspector, including safe runner identity, bounded messages/schemas/estimates, credential and binary redaction, and no-I/O inspection. Hax documentation informed the visibility goal; no clean Cursor request-inspector fixture or Hax runtime observation is claimed.
- process-only central tool selection through `--tools`, `--include-tools`, and `--exclude-tools`, including one restrictive resolver for built-ins, dynamic MCP tools, review, checkpoint, scripted, headless, and subagent paths. Ante documentation informed the ordering and case-insensitive exact-name grammar; no Ante runtime observation or Cursor parity is claimed.
- rich Allow/Ask/Deny permission rules, bounded process-only session grants, safe command-prefix parsing, and the `/permissions` inspection/removal manager. Ante documentation informed the precedence and temporary-grant vocabulary; no Ante runtime observation or Cursor parity is claimed.
- opt-in headless verification and locally validated structured output under `ca.headless.v2`, while ordinary automation remains `ca.headless.v1`. Ante documentation informed the post-task check idea, but `ca` deliberately defaults the pass to verify-only and no Ante runtime observation or Cursor parity is claimed.
- the protocol-neutral provider adapter boundary, including default-preserving Chat Completions configuration, exact adapter-owned wire bytes/auth/path, canonical events/usage/tool continuations, and secret/body-free failures. Multi-provider oracle documentation informed the architecture priority; no clean Cursor protocol fixture or Ante runtime observation is claimed.
- the opt-in OpenAI Responses adapter, including ordered bounded completed output-state replay with encrypted reasoning content, typed message/function/function-output Items, flat function schemas, reasoning/text/tool/usage stream translation, and safe lifecycle failures; plus exact DeepSeek/MiMo `reasoning_content` continuation on their canonical Chat routes. Official provider documentation defines these wire contracts. They are intentional `ca` extensions with no clean Cursor/Ante fixture or runtime parity claim, and generic Chat Completions remains unchanged.
- the opt-in Anthropic Messages adapter, including top-level system blocks, client `tool_use`/`tool_result` replay, thinking/text/tool/cache-usage stream translation, versioned key authentication, and safe lifecycle failures while both existing adapters remain unchanged. Official Anthropic documentation defines the wire contract; no Cursor or Ante Anthropic runtime parity is claimed.
- explicit exact-pair provider fallback for interactive turns, including closed pre-output causes, whole-tool-loop route pinning, sanitized persisted provenance, and automatic primary restoration on the next turn. Oracle resiliency ideas informed the priority, but no clean Cursor fallback fixture or oracle runtime observation is claimed.
- opt-in MCP Streamable HTTP over HTTPS or literal-loopback HTTP, including environment-referenced headers, existing project/network authority, no proxy/redirect behavior, bounded JSON/SSE surfaces, and transport-neutral tools/resources/reconnect/shutdown. MCP ecosystem documentation informed the protocol choice; no clean Cursor HTTP-MCP fixture or oracle runtime observation is claimed.
- explicit MCP OAuth connect/refresh/disconnect for Streamable HTTP, including PKCE browser authorization, optional advertised device authorization, issuer/host pinning, profile-scoped Keychain storage, and secret-free status/failure surfaces. The MCP authorization specification and RFC 8628 informed the protocol boundary; no clean Cursor MCP-OAuth fixture or oracle runtime observation is claimed.
- deferred MCP discovery through a bounded `search_mcp_tools` call, exact process-local schema activation, list-change invalidation, and prepared-context/request accounting. MCP ecosystem patterns informed the product boundary; no clean Cursor deferred-MCP fixture or oracle runtime observation is claimed.
- explicitly configured process-scoped LSP semantic navigation through bounded symbols, definition, references, hover, implementation, and call-hierarchy tools. Oracle coding-agent navigation patterns informed the priority; no clean Cursor semantic-tool fixture or oracle runtime observation is claimed.
- bounded page-ranged local PDF text extraction with workspace authority, blocking parsing, truthful continuation metadata, and no embedded-file/action execution. Oracle document-reading ideas informed the priority; no clean Cursor PDF-tool fixture or oracle runtime observation is claimed.
- typed multimodal provider content and bounded local `view_image`, including exact-model capability gates, process-only normalized payloads, adapter-owned serialization, and unavailable-safe durable markers. Oracle image-inspection ideas informed the priority; no clean Cursor multimodal fixture or oracle runtime observation is claimed.
- opt-in profile-scoped user and workspace memory with bounded stale-labelled recall, local inspection/edit/export/delete, and explicit acceptance of process-only model proposals. Ante memory documentation informed the safety/product boundary; no clean Cursor memory fixture or Ante runtime observation is claimed.
- durable finite `/goal` sessions with fresh evidence-only evaluation, request-only continuation feedback, explicit human/approval pauses, bounded iteration/token/time budgets, and typed terminal outcomes. Ante goal-session documentation informed the product boundary; no clean Cursor goal fixture or Ante runtime observation is claimed.
- explicit finite `/workflow run <path>` execution for bounded TOML/JSON DAGs, with deterministic dependencies, restrictive tool selection, previewed process-local grants, evidence gates, bounded outputs/retries, and explicit pause/resume. Ante workflow documentation informed the product boundary; no clean Cursor workflow fixture or Ante runtime observation is claimed.
- explicit profile-scoped `/worktree` creation, bounded inspection, attachment, and confirmation-only cleanup with immutable child-workspace inheritance and unconditional branch preservation. MiMo/Kun isolation and preservation documentation informed the safety boundary; no clean Cursor worktree fixture or oracle runtime observation is claimed.
- explicit dependency-aware `/tasks` agent graphs over approved plan tasks, with exact grants and owned-worktree assignment, bounded append-only attempts, local evidence gates, stable ready-node scheduling, and confirmation-only start/retry/cancel. Oracle multi-agent/worktree patterns informed the product boundary; no clean Cursor agent-graph fixture or oracle runtime observation is claimed.
- explicit process-only `/multi-run` best-of-N over two to five exact provider/model routes and distinct owned worktrees, with one frozen request/base, bounded hashed local evidence, separate read-only structured judgment and fusion proposal, and no automatic attach/merge/cleanup/default/session mutation. Oracle multi-run and worktree patterns informed the product boundary; no clean Cursor multi-run fixture or oracle runtime observation is claimed.
- the compile-time-gated strong-planner/cheap-executor research harness, including the strict provider-neutral visible `reasoning_artifact.v1`, isolated six-arm orchestration, exact capped accounting, hidden verification, and body-free paired reports. This is an intentional `ca` research extension informed by primary provider documentation and planner/router research; it excludes opaque cross-model reasoning replay, leaves normal `ca` behavior unchanged, and claims no Cursor/Ante parity or runtime observation. The first paid sample stopped before traffic because no canonical OpenAI Sol/Luna route was configured.
- deterministic safe-metadata skill suggestions through exact canonical IDs, declared aliases, and bounded lexical BM25 ranking in the idle composer and `/skills`, with explicit non-activation and unchanged hash/approval/prompt/session boundaries. MiMo documentation informed the ranking vocabulary, but `ca` deliberately excludes automatic loading and claims neither MiMo runtime observation nor Cursor parity.
- the explicit process-only `/walkthrough` and `/explain-changes` flow over one complete bounded Git snapshot, including local refusal for omitted or untracked input, exact source hashing, a sole validated ordered-report schema, and visible stale-source detection. Oracle guided-change ideas informed the priority; no clean Cursor walkthrough fixture or oracle runtime observation is claimed.
- the strict process-only `ca.serve.v1` JSONL service over stdio or one authenticated literal-loopback WebSocket client, including `ca attach`, protected external token sources, explicit session/input/approval/interaction/cancel/status/shutdown operations, nested canonical runner events, and no session persistence. Ante's documented daemon/transport lifecycle informed the product boundary; the wire and authentication contracts are independently specified and no clean Cursor fixture or Ante runtime observation is claimed.
- stable local ACP v1 interoperability through `ca acp`, using the official Rust
  SDK over newline JSON-RPC stdio with CA-owned providers, sessions, tools,
  permissions, and cleanup. Cursor bundle evidence established that ACP support
  exists, but the bundle was not runnable in the clean harness; CA's wire
  behavior is characterized by official SDK fixtures and is not a Cursor
  runtime-parity claim.

- the profile-scoped expanded theme color vocabulary (diff, paste, markdown-heading-hierarchy, scrollbar, and semantic state colors), animated spinner state indicators, compact mode toggle, deep block styling config (tool/thinking/edit), auto-theme dual-mapping with RosePine sixth theme, terminal cursor color sync via OSC 12/112, and scrollbar customization. These are Grok-Build-informed `ca` extensions. The read-only Grok Build user-guide docs (`~/.grok/docs/user-guide/06-theming.md`, `23-dashboard.md`, `25-status-line.md`, `26-config-reference.md`) informed the color vocabulary, animation patterns, and customization surface; no Grok Build runtime parity is claimed and no oracle source was copied. CA-owned fixtures and snapshot tests define acceptance.
- the full-screen agent dashboard (`/dashboard`, `Ctrl+\`, `ca dashboard`) with state-grouped session rows, `+ New Agent` dispatch, worktree toggle, Open Previous, and a tabbed usage modal (`Context usage` / `Usage limit` / `Session info`). The Usage limit tab is a compact percentage bar plus caption derived from CA provider/ledger data (not a SuperGrok weekly quota). Grok Build user-guide `23-dashboard.md` and the user-attached screenshots informed layout and keys; CA still attaches one conversation at a time. No Grok Build runtime parity is claimed and no oracle source was copied. CA-owned fixtures and snapshot tests define acceptance.

Each extension has deterministic local tests and wide/narrow snapshots where visible, but remains `Unverified` against Cursor until `PAR-005` records direct evidence.

`AUTH-060` added no extension: it stopped at the official-support boundary because no documented third-party Codex/ChatGPT OAuth provider contract authorizes `ca` to own or directly use those credentials.

Do not replace `Unverified` with `Matched` from source inspection alone.

## Workflow for One Parity Slice

1. Choose one row and one reference version.
2. Record terminal dimensions, config assumptions, keystrokes/input, and exact visible/exit behavior.
3. Save a text fixture or concise observation note that contains no secrets or bundled source.
4. Add a failing deterministic `ca` test.
5. Implement the smallest behavioral change.
6. Run focused and completion validation.
7. Update the row and record acceptable deviations.

## Stop Conditions

Stop and ask for evidence when:

- clean and patched oracle artifacts disagree;
- a behavior requires online Cursor auth/service access not available to the task;
- the only path is copying proprietary implementation code;
- terminal dimensions, version, or interaction sequence are unknown enough to change the outcome.
