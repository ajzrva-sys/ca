# MEM-068 — Project and User Memory

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-11

## Goal

Add separate, profile-scoped user and workspace memory stores that are disabled by default, safely inspectable and editable through `/memory`, and injected only when explicitly enabled, while model-generated entries remain proposals requiring user approval.

## Non-Goals

- Automatic default-on memory, opaque embeddings/vector databases, remote sync, cross-profile sharing, or silent model writes.
- Storing credentials, active task state, chat transcripts, approval state, tool results, or repository facts that can be reread from the workspace.
- Treating recalled paths, versions, or facts as current without revalidation.
- Cursor parity. This is an oracle-informed `ca` extension.

## Evidence and Current Behavior

- `RuntimePaths` owns one profile-derived data root for sessions but exposes no memory paths (`src/runtime.rs`).
- Request instructions are assembled from project/global instructions and active skills; no memory component exists (`src/main.rs`, `src/context.rs`).
- `ConfigStore` performs comment-preserving atomic raw-document patches and bare mode already denies persistent writes (`src/config.rs`, `src/runtime.rs`).
- The typed slash registry and manager state/render/input boundaries support searchable idle-only hubs, but `CommandId::Memory` does not exist (`src/tui/commands.rs`, `src/app.rs`, `src/tui/input.rs`, `src/tui/render.rs`).
- Ante documentation describes automatic project memory, but no Ante runtime fixture or clean Cursor memory fixture is available. This plan deliberately makes use and generation opt-in and approval-gated.

## Scope

Allowed files: runtime/config/context/request assembly; a new memory storage/policy module; command/app/TUI manager state and orchestration; focused temp-root tests and snapshots; this plan and required harness documents.

Forbidden: oracle/reference trees, user runtime state, real credentials/Keychain/sessions, remote services, vector databases, automatic proposal acceptance, and installed-binary mutation.

Runtime state affected: profile-scoped Markdown/metadata files under the injected data root only after explicit user mutations. Tests and walkthroughs use temporary homes/workspaces.

## Design and Invariants

- Default configuration has both user/workspace recall and proposal generation off. Merely starting `ca`, opening `/memory`, previewing context, or preparing a request creates no file.
- Default-profile user memory lives under the legacy data root; named profiles use their existing profile data namespace. Workspace memory is further keyed by a stable digest of the canonical workspace root, never a raw absolute path component. Bare may inspect normal-profile memory only when explicitly enabled in the normal config but cannot mutate it.
- Persist a versioned, bounded, deny-unknown-fields record collection using atomic regular-file replacement and symlink refusal. Stable record IDs, scope, timestamps, source/provenance, and text are safe to render; no runtime authority or generated embeddings are stored.
- Validate every entry locally before proposal or save. Reject credential-like material, control characters, task/transcript state, user-scope project facts, and workspace facts that are cheaply rereadable. Bound record count, text length, file size, and rendered search results.
- `/memory [filter]` opens a scope-aware searchable manager for status, records, pending proposals, edit, export, and confirmed deletion. User approval is explicit per proposal; Cancel is selected by default for acceptance/deletion, busy mutations lock input, and failures remain inline without losing selection.
- Model-generated entries enter only a process-local proposal queue. No provider/tool may write memory directly; accepted proposals traverse the same local validator and atomic store mutation as manual edits.
- Request injection is independently enabled per scope and capped at 200 lines and 25 KiB for each index. The prompt labels recalled paths, versions, and facts as potentially stale and instructs revalidation before use. Prompt bytes are unchanged when recall is disabled or no records exist.
- Memory content is excluded from sessions, `/request` credential authority, usage state, debug output beyond bounded safe text, and headless structured lifecycle events. The request inspector may attribute a bounded instruction category without exposing hidden data beyond what the provider received.

## Milestones

### M1 — Storage, validation, and path characterization

- Add failing tests for default-off config, default/named/bare path derivation, workspace digest isolation, missing-file no-write behavior, symlink/traversal refusal, bounds, atomic rollback, and secret/task/repository-fact rejection.
- Implement typed records, proposals, safe summaries, and temp-root stores without TUI integration.

### M2 — Bounded recall and proposal boundary

- Add exact prompt-byte preservation tests for disabled/empty memory and deterministic scope ordering/caps for enabled recall.
- Add process-only proposal generation/approval state and prove no model/tool path persists before explicit acceptance.

### M3 — `/memory` manager

- Add command discovery/filtering and searchable scope/list/detail/proposal views with manual add/edit, export, Cancel-first accept/delete, busy locking, stable selection, and bare read-only guidance.
- Add wide/narrow empty, populated, proposal, confirmation, busy, failure, and bare snapshots.

### M4 — Orchestration and gates

- Wire config/save reload, context/request attribution, process-only invalidation, and temporary-home export/walkthrough flows.
- Reconcile the harness and run focused/full/format/strict-Clippy/release/smoke gates.

## Acceptance Criteria

- Default startup and request bytes are unchanged and create no memory files.
- Default and named profiles cannot read, mutate, or delete each other's user/workspace records; distinct canonical workspaces remain isolated.
- Each enabled injected scope is deterministic and independently capped at 200 lines/25 KiB with a stale/reverify label.
- Secret-like, control-laden, active-task/transcript, rereadable repository, and project-to-user entries fail before persistence without echoing rejected content.
- A generated proposal remains process-only until an explicit Cancel-first acceptance; reject/cancel/process exit never changes disk.
- `/memory` supports bounded inspection/search/manual edit/export/confirmed deletion and stays view-only under bare mode.
- Sessions, snapshots, debug output, structured events, config, and Keychain never receive secret material or unbounded memory content.

## Validation

Focused: PASS. `cargo test memory --no-fail-fast` ran 28 matching tests covering config/runtime paths, exact storage, validators, prompt preservation, proposal boundaries, central tool selection, input/render state, and blocking-worker orchestration.

Completion: PASS. `cargo test --no-fail-fast` ran 791 unit tests plus 28 CLI tests; `cargo fmt --check`, snapshot hygiene, `cargo clippy --all-targets -- -D warnings`, and `cargo build --release` all passed.

Manual/visual: PASS. Eight generated manager/editor/proposal/bare/failure snapshots were reviewed at wide and narrow sizes. A released TUI under a temporary home enabled proposals, received one process-only tool proposal, showed Cancel-first approval, persisted/exported after confirmation, and injected the stale-labelled recall into a later request.

Blocked checks: none.

## Progress

- [x] Current runtime/config/prompt/manager seams characterized.
- [x] Failing storage/path/validator tests added.
- [x] Bounded recall and proposal approval boundary implemented.
- [x] `/memory` manager and snapshots implemented.
- [x] Repository-wide gates and isolated walkthrough passed.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: store explicit bounded records rather than one free-form mutable memory prompt.
  Evidence: inspection, per-entry approval, deletion, provenance, and scope separation require stable identity.
  Consequence: imports/edits validate individual records and deterministic indexes are derived in memory.
- Decision: derive workspace storage from the canonical root digest.
  Evidence: raw paths are unsafe filename components and basename-only keys collide.
  Consequence: the manager renders a safe workspace label while the path remains runtime authority.
- Decision: keep generated proposals process-only until confirmation.
  Evidence: model output is untrusted and the roadmap explicitly requires approval.
  Consequence: a crash or cancellation safely loses unaccepted proposals.

## Surprises and Discoveries

- Named profiles currently scope session paths under the data root but do not expose a reusable profile data-directory accessor; MEM-068 must centralize that derivation without changing existing session paths.

## Recovery and Rollback

Disable recall/generation flags, remove the `/memory` manager and memory module, and leave any versioned memory files inert. No config or session migration is required and rollback must not delete user-created memory files automatically.

## Completion Record

Changed files: `src/memory.rs`, runtime/config/context/request assembly and tool orchestration, app/command/input/render state plus snapshots, and the required harness documents.

Commands and results: focused 28-test memory slice PASS; full 791-unit/28-CLI suite PASS; format/snapshot hygiene/strict Clippy/release PASS; help/version/named-profile smoke PASS; isolated released-TUI walkthrough PASS.

Remaining risks: validators are intentionally conservative heuristics rather than semantic secret detection; memory facts remain untrusted prompt input and are explicitly labelled for revalidation; pending proposals are deliberately lost at process exit.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, `ENVIRONMENT.md`, and `docs/.INDEX.md` reconciled for MEM-068.
