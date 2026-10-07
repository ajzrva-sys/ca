# INT-199 — Dry-Run Workflow Synthesis, Permanent Correction, and Governed Execution

Status: COMPLETED
Owner/session: current CA/codex-team integration session
Last updated: 2026-08-27

## Goal

CA can synthesize a bounded typed remote workflow, have codex-team preview it
without mutation, permanently apply proven policy-tightening corrections, and
execute an exactly approved revision with deterministic verification and repair.

## Non-Goals

- Arbitrary shell or model-supplied executables, URLs, credentials, or remote
  commands.
- New HTTP, container, service-manager, storage, database, MCP, or resource
  authority.
- Live provider/remote traffic, installed-binary changes, commits, staging,
  cleanliness gates, or protected-tree edits.
- A new daemon or background-service registration.

## Evidence and Current Behavior

- `codex-team-protocol` already owns closed runner-free setup and remote action
  contracts.
- `codex-team::remote` already provides fixed SSH/rsync preview/apply and
  read-only Transmission RPC with exact sync-plan digests.
- `codex-team::automation_setup` already owns private mode-0600 automation
  manifests and digest-bound profile updates.
- CA already exposes Main-only remote/setup tools through transparent routing,
  with normal network/mutation approval and all recursion exclusions.
- Schedule V2 already executes repository mechanistic query/evidence payloads
  without a provider; it has no approved remote-workflow target.

## Scope

Allowed files: shared protocol; codex-team workflow/remote/CLI modules and
focused tests; CA registry/routing/MCP/approval/schedule/session/evidence seams
and focused tests; this plan and one concise `TESTING.md` result.

Forbidden: `oracles/`, `reference/`, runtime profiles/configuration, secrets,
installed binaries, Git index/history, services, and unrelated source.

Runtime state affected: temporary test homes, manifests, workflows, policies,
and hermetic remote fixtures only.

## Design and Invariants

- Workflow wire data is a closed 1–16-node DAG over named SSH, Transmission,
  rsync, inventory, free-space, SHA-256, and assertion actions. The protocol
  crate contains no runner.
- Offline preview never starts a process. Probe and mutation preview may use
  only fixed typed adapters and never mutate their targets.
- Corrections are classified independently by codex-team. Only monotonic
  restrictions may persist without a user prompt; widening always requires an
  exact approval digest and conflicts write nothing.
- Policies are owner-only machine-global records with structural match
  contracts. Human decisions outrank learned/CA decisions.
- The first run binds exact workflow, policy, resource, and run-plan digests.
  Later dynamic runs auto-authorize only within the stored limits; drift or
  broader effects stop before mutation.
- Deterministic repairs remain inside the typed adapter. Unknown failures return
  evidence to CA. No polling, status, schedule, or repair event creates provider
  traffic by itself.
- The repository delegate remains network-disabled and the 11/15/28 MCP
  catalogs plus zero resources remain byte-for-byte compatible.

## Milestones

### M1 — Protocol and characterization

- Add workflow request/receipt/correction schemas and strict structural tests.
- Characterize existing remote plan/apply, tool selection, approval, and
  Schedule V2 exclusions before behavioral edits.

### M2 — Codex-team workflow engine

- Add offline/probe/mutation previews, correction classification/vault,
  exact approval records, bounded execution, verification, and typed repairs.
- Extend fixed adapters only where required for bounded selection, free-space,
  inventory, and SHA-256 verification.

### M3 — CA facade and durable coordination

- Add the five Main/headless workflow tools, host-owned approval recognition,
  session/job/evidence receipts, and approved Schedule V2 workflow targets.
- Preserve every excluded surface and avoid exposing absolute private bindings.

### M4 — Validation and completion

- Run focused hermetic tests while iterating, one required Rust test pass at
  completion, formatting, strict Clippy, and one all-feature release build.
- Record one concise testing result and move this plan to completed.

## Acceptance Criteria

- A completed-download seedbox workflow receives all three truthful previews,
  an exact bounded first-run plan, and no mutation during preview.
- A no-delete or stronger-hash correction persists automatically; a new target,
  deletion, or higher limit requires exact approval; conflicting correction
  leaves state unchanged.
- An approved revision runs again without a prompt only when resource identity,
  policy, limits, and effects remain compatible.
- Execution verifies the declared outcome, performs only bounded deterministic
  repairs, and blocks on authentication, identity, policy, or space failures.
- Scheduled execution is model-free; restart recovery never replays a completed
  mutation; all recursion exclusions and MCP catalogs remain unchanged.

## Validation

Focused: protocol, workflow engine, remote fixtures, CA registry/routing,
approval, schedule, session/evidence, and exclusion tests.

Completion: required Rust tests once, `cargo fmt --all -- --check`, strict
workspace/all-target/all-feature Clippy, and one all-feature release build.

Blocked: live hosts, credentials, provider requests, installation, and service
activation are intentionally outside this task.

## Progress

- [x] Existing protocol, remote, setup, approval, workflow, schedule, and learning seams inspected.
- [x] Workflow protocol and strict validation implemented.
- [x] Codex-team previews, corrections, execution, verification, and repair implemented.
- [x] CA facade, approval, schedule, recovery, and evidence integration implemented.
- [x] Focused and completion validation recorded.

## Decisions

- Decision: automatically persist only monotonic tightening corrections.
  Evidence: current user choice.
  Consequence: CA can learn safe constraints without silently expanding effects.
- Decision: approve a workflow revision once, then require fresh approval only
  for drift or broader effects.
  Evidence: current user choice.
  Consequence: repeated automation is practical but remains bounded.
- Decision: store correction records machine-wide with narrow structural match
  contracts.
  Evidence: current user choice.
  Consequence: useful safety learning crosses profiles without indiscriminate matching.

## Surprises and Discoveries

- The required workspace test pass exposed a broker startup race under five
  concurrent stdio clients: the private broker could apply its test idle timer
  before serving an authenticated request, and hashing a large debug binary
  could exceed the prior three-second startup allowance. The broker now waits
  for its first authenticated request before applying idle shutdown and keeps
  startup bounded at ten seconds. The complete MCP SDK target subsequently
  passed 8/8, including both five-client broker cases.

## Recovery and Rollback

All tests use disposable private roots. Runtime records use atomic owner-only
writes. Failed preview/correction/approval transactions leave prior records
unchanged. No destructive Git recovery is permitted.

## Completion Record

Changed files: workspace manifests and lockfile; `codex-team-protocol`; the
codex-team workflow, remote-adapter, broker, CLI, and library modules; CA's Main
tool registry, mechanistic facade, MCP child, approval/permission, scheduling,
tool-selection, workspace, and system-guidance seams; this completed plan and
one `TESTING.md` result.

Commands and results:

- Focused protocol/workflow/remote/CA routing, approval, scheduling, recovery,
  feedback, exclusion, broker, and MCP tests passed.
- The single `cargo test --workspace --no-fail-fast -- --test-threads=1` pass
  passed every target except two five-client broker races in the MCP SDK target.
  After the bounded startup fix, both cases passed together and the complete
  focused MCP SDK target passed 8/8. The broad workspace suite was not repeated.
- `cargo fmt --all -- --check`, strict all-target/all-feature workspace Clippy,
  and `cargo build --workspace --release --all-features` passed.

Remaining risks: live SSH, Transmission, rsync, credentials, and remote identity
behavior were intentionally not exercised. Remote SHA-256 verification requires
an rsync build supporting SHA-256 checksum choice. Torrent selection validates
and counts the bounded RPC selection, while the named private rsync profile
remains responsible for scoping the actual transfer source. No installation or
runtime-profile mutation was performed.

Harness updates: this plan and one concise `TESTING.md` record only.
