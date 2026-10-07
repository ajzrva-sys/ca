# INT-200 — Typed Transmission and rTorrent Automation

Status: COMPLETE
Owner/session: current CA/codex-team integration session
Last updated: 2026-08-27

## Goal

Codex-team provides one normalized, approval-governed torrent automation layer
for named Transmission and rTorrent resources, including bounded queries,
comparisons, safe workflow-only controls, and selection-aware synchronization.

## Non-Goals

- Generic RPC methods, raw XML, raw URLs, remote commands, torrent addition,
  record removal, data deletion, client shutdown, or arbitrary loading.
- Direct ordinary mutation tools; every torrent mutation remains inside an
  exact previewed and approved automation workflow.
- Live provider or remote traffic, runtime profile changes, installation,
  commits, staging, Git cleanliness gates, services, or protected-tree edits.
- Copying implementation source from `oracles/` or `reference/`.

## Evidence and Current Behavior

- `codex-team-protocol::WorkflowActionV1` currently supports read-only
  Transmission queries and torrent selection but has no normalized torrent
  inventory or typed RPC controls.
- `codex_team::remote` owns a bounded legacy Transmission RPC client with 409
  session negotiation, fixed SSH/rsync execution, private named resources, and
  digest-bound sync plans.
- `codex_team::automation_workflow` already provides offline, read-only, and
  mutation previews; exact plan approval; serialized mutation; drift checks;
  bounded repair; and post-run verification.
- `codex_team::automation_setup` already creates owner-only private manifests
  from a closed CA setup draft.
- CA already exposes Main/headless workflow tools while excluding Plan, review,
  checkpoint, service, scheduled-model, subagent, bare, and `role-run` surfaces.

## Scope

Allowed files: workspace manifests and lockfile; `codex-team-protocol`;
codex-team remote/workflow/setup modules and focused tests; CA workflow/setup
tool descriptions, routing/rendering tests when required; this plan; and one
concise `TESTING.md` completion record.

Forbidden files/areas: `oracles/`, `reference/`, protected-tree manifests,
runtime profiles/configuration, credentials, installed binaries, Git index and
history, services, and unrelated source.

Runtime state affected: disposable test homes, private manifests, Unix sockets,
loopback fixtures, workflow records, and temporary sync roots only.

## Design and Invariants

- The runner-free protocol owns closed torrent DTOs and actions. Codex-team
  alone owns networking, sockets, SSH forwarding, filesystem access, approval,
  and execution.
- Torrent identity is the lowercase 40-character info hash. All remote text is
  bounded, control-sanitized, secret-redacted, and treated only as data.
- Transmission selects legacy or JSON-RPC 2.0 through a current read-only probe;
  mutations require the probed protocol and resource identity to remain stable.
- rTorrent uses a closed XML-RPC method allowlist over owned HTTPS/loopback HTTP,
  validated local SCGI, or fixed SSH forwarding to remote loopback SCGI.
- SCGI is never accepted on a non-loopback direct TCP endpoint. Credential
  values, raw endpoints, socket paths, storage paths, and SSH destinations stay
  inside the owner-only private manifest.
- Start, stop, verify, reannounce, one configured automation label, priority,
  and named-location adoption are the only V1 controls. Location adoption never
  moves data and must depend on an approved transfer and verification.
- Selection-aware rsync consumes only a private sorted manifest of contained
  files from an exact torrent selection. Its file list, sizes, and preview
  effects are bound into the approved run-plan digest.
- Preview never mutates. Selected hashes and resource/protocol identity are
  rechecked immediately before mutation. Ambiguous sends are resolved through
  state inspection without blind replay.
- Existing repository MCP catalogs remain exactly 11/15/28 with zero resources.

## Milestones

### M1 — Protocol and characterization

- Add normalized torrent inventory, client identity, typed control, mutation,
  resource/setup, workflow, and limit contracts additively.
- Add legacy decoding and closed-schema/bounds characterization tests.

### M2 — Owned adapters

- Modernize Transmission for probed legacy/JSON-RPC 2.0 compatibility.
- Add bounded rTorrent XML-RPC over owned HTTP and SCGI transports with fixed
  SSH forwarding and hermetic framing/authentication fixtures.

### M3 — Governed workflow integration

- Route named cross-client queries/comparisons through normalized inventories.
- Add exact torrent selection, safe serialized controls, location dependency
  validation, immediate drift checks, selection-aware rsync, and post-error
  state resolution.
- Extend private setup generation and CA-visible descriptions without adding
  ordinary mutation tools or changing excluded surfaces.

### M4 — Validation and completion

- Iterate with focused protocol, adapter, workflow, setup, routing, approval,
  schedule, exclusion, and catalog tests.
- Run the required workspace Rust test pass once, then formatting, strict
  workspace Clippy, and one all-feature release build.
- Record one concise testing result and move this plan to completed.

## Acceptance Criteria

- Legacy Transmission and Transmission JSON-RPC 2.0 fixtures normalize to the
  same bounded torrent inventory and address mutations only by info hash.
- HTTPS/loopback HTTP, Unix SCGI, loopback TCP SCGI, and named-SSH remote
  loopback SCGI rTorrent fixtures produce valid bounded XML-RPC while unsafe
  endpoints, arbitrary methods, malformed XML, and credential leakage fail.
- Cross-client comparison is stable by info hash and reports malformed,
  missing, duplicate, completed, checking, stopped, and errored entries safely.
- Workflow previews never mutate; exact approval, protocol/resource identity,
  selected hashes, configured locations, limits, and verification gates bind
  every mutation.
- Selection-aware rsync includes only contained selected torrent files and
  blocks traversal, out-of-root paths, excessive files, and stale inventories.
- Start/stop/label/priority/location ambiguous sends are state-resolved;
  verify/reannounce are never blindly replayed.
- CA exclusions and exact 11/15/28 zero-resource MCP catalogs remain unchanged.

## Validation

Focused: protocol round trips and bounds; Transmission protocol fixtures;
rTorrent HTTP/SCGI/SSH fixtures; normalized comparisons; workflow preview/run,
approval, drift, repair, selection-aware sync, setup, schedule, and CA surface
tests.

Completion: one `cargo test --workspace --no-fail-fast -- --test-threads=1`,
`cargo fmt --all -- --check`, strict all-target/all-feature workspace Clippy,
and one all-feature workspace release build.

Blocked checks: live torrent clients, remote hosts, credentials, provider
traffic, installation, and service activation are intentionally out of scope.

## Progress

- [x] Existing protocol, remote, workflow, setup, CA facade, and validation seams inspected.
- [x] Normalized protocol and backward-compatible setup/workflow contracts implemented.
- [x] Transmission and rTorrent owned adapters implemented and hermetically tested.
- [x] Workflow controls, exact selection-aware sync, drift/approval semantics, and CA presentation integrated.
- [x] Focused and completion validation recorded.

## Decisions

- Decision: keep codex-team's Transmission client owned and use the `xmlrpc`
  crate only as a codec under codex-team-owned rTorrent transports.
  Evidence: the approved INT-200 plan.
  Consequence: transport, authentication, bounds, and execution authority remain
  under one governor.
- Decision: treat `transmission-rpc` and `rtorrent-xmlrpc-bindings` as pinned
  development conformance references only.
  Evidence: the approved INT-200 plan.
  Consequence: third-party clients cannot widen runtime authority.
- Decision: preserve legacy workflow/setup decoding additively.
  Evidence: current installed manifests and workflow revisions must remain valid.
  Consequence: all new fields are optional/defaulted until torrent-backed
  mutation requires their stronger limits.

## Surprises and Discoveries

- The first rTorrent multicall fixture exposed an off-by-one mapping between
  message, directory, and custom-label columns; the corrected indices now have
  normalization coverage.
- A generated rTorrent setup initially flattened its transport tag, which the
  private runtime schema correctly rejected. Generation now emits the nested
  closed transport table consumed by the runtime.
- Endpoint validation needed an explicit same-source rule: a selection from one
  torrent client can no longer drive a control on another client, even when
  both happen to contain the same info hash.
- Public receipt sizing originally counted rendered entries but not the new
  typed inventory. The final receipt is now measured as serialized bytes and
  sheds file detail before torrents while retaining truncation evidence.

## Recovery and Rollback

All network/process tests use disposable loopback fixtures and kill-on-drop
owners. Private writes use existing atomic owner-only helpers. Source rollback
is by narrow inverse edits only; destructive Git recovery is forbidden.

## Completion Record

Changed files: the shared protocol contract; codex-team's torrent, remote,
workflow, setup, and library modules; the remote CLI compatibility fixture;
CA's workflow/setup presentation and Main-agent guidance; dependency manifests;
this plan; and one concise root `TESTING.md` result.

Commands and results:

- Focused protocol, adapter, remote, workflow, setup, CA routing/rendering,
  schedule, exclusion, and real MCP compatibility tests passed.
- The one required workspace `cargo test --workspace --no-fail-fast --
  --test-threads=1` pass found one obsolete legacy Transmission fixture
  expectation after every preceding target passed. The fixture now explicitly
  selects the legacy protocol and asserts the normalized session receipt; its
  focused target passes. The broad suite was not repeated.
- `cargo fmt --all -- --check` passed.
- Strict all-target/all-feature workspace Clippy passed with warnings denied.
- `cargo build --workspace --release --all-features` passed.
- The real release-binary CA MCP fixture passed with exact 11/15/28 catalogs
  and zero resources.

Remaining risks: no live Transmission, rTorrent, SSH, SCGI, credential,
provider, installation, or service test was performed. Transport behavior is
covered by hermetic loopback/socket fixtures, and unsafe direct SCGI remains
fail-closed.

Harness updates: this completed plan and one concise `TESTING.md` record only.
