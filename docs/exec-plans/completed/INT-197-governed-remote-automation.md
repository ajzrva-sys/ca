# INT-197 — Governed Remote Automation

Status: COMPLETE
Owner/session: current CA/codex-team integration session
Last updated: 2026-08-27

## Goal

Normal CA Main/headless agents can automatically hand named remote comparison,
read-only RPC, sync planning, and explicitly approved sync application to the
Rust `codex-team` binary without constructing SSH commands or handling secrets.

## Non-Goals

- Arbitrary remote shell, model-supplied hosts/URLs/paths, port forwarding, or
  remote package installation.
- Silent deletion/overwrite, RPC mutation, default-profile edits, installation,
  service activation, or live access to the user's servers during tests.
- Changing the existing codex-team MCP tool catalogs or giving the repository
  delegate process network access.

## Evidence and Current Behavior

- `ca-watch` already performs bounded SSH PID observation through fixed
  `/usr/bin/ssh` arguments, but it does not compare or synchronize content.
- The mechanistic delegate is deliberately sandboxed with `network = false` and
  CA validates its exact 15-tool delegate catalog.
- Learned history contains recurring rsync, Transmission RPC, and seedbox
  procedures, but learned command text remains inert.
- Inferred: a separate typed subprocess protocol using the already validated
  codex-team executable preserves the existing MCP boundary while allowing one
  ordinary CA tool to request an approved network grant.

## Scope

Allowed files:
- `crates/codex-team-protocol/src/lib.rs`
- `crates/codex-team/src/{cli.rs,lib.rs,remote.rs}` and its manifest
- CA config, tool registry/selection, mechanistic routing, remote executor, and
  focused tests
- this plan, one `NEXT_STEPS.md` link, and one concise `TESTING.md` result

Forbidden files/areas:
- `oracles/`, `reference/`, user profiles/configuration, credentials, installed
  binaries, Git staging/commits, and unrelated dirty files

Runtime state affected: none during implementation and hermetic validation.

## Design and Invariants

- Remote hosts, RPC endpoints, roots, and sync direction come only from a
  private mode-0600 named manifest; model calls contain identifiers/enums only.
- SSH uses fixed BatchMode/no-forwarding arguments. Sync uses fixed rsync
  arguments and a dry-run plan digest. No model or manifest shell text runs.
- RPC is read-only and protocol-specific; the first slice supports bounded
  Transmission session/torrent/free-space queries and normalized comparison.
- Sync application requires the exact fresh plan digest and CA classifies the
  apply tool as mutating/networked. Plan drift fails closed.
- The remote subprocess receives a bounded network grant and only the configured
  environment entries; receipts redact and bound output and never contain
  credentials or canonical local paths.
- Plan/review/checkpoint/service/scheduled-model/subagent/bare/role-run surfaces
  remain excluded. Existing MCP catalogs/resources remain byte-for-byte stable.

## Milestones

### M1 — Typed contract and hermetic runner
- Add remote request/action/receipt and strict validation.
- Implement private manifest loading, fixed SSH/rsync planning/application, and
  read-only Transmission RPC with injectable/fake process and HTTP seams.
- Validation: focused protocol and codex-team remote tests.

### M2 — CA automatic handoff
- Add disabled-by-default remote config nested under mechanistic delegation.
- Advertise ordinary remote tools only on normal Main/headless surfaces.
- Route calls to the validated codex-team executable with existing approval,
  ordering, retry, cancellation, and output behavior.
- Validation: focused config/selection/approval/routing/exclusion tests.

### M3 — Completion
- Run focused integration tests, formatting, strict Clippy, and one release
  build. Record actual results without installation or Git gates.

## Acceptance Criteria

- A fake CA Main run calls `compare_remote_files` or `compare_remote_rpc` and
  receives one bounded codex-team receipt without any SSH/RPC arguments from the
  model beyond configured names and typed query enums.
- `plan_remote_sync` returns a digest; `apply_remote_sync` rejects missing,
  stale, or mismatched digests and executes only after the normal mutating
  network approval path.
- Five excluded surfaces advertise and execute none of the remote tools.
- Cancellation terminates the owned codex-team/SSH/rsync process group only.
- Existing 11/15/28 MCP catalogs and zero-resource tests remain unchanged.

## Validation

Focused: protocol, remote module, CA config/tool selection/approval/routing.
Completion: `cargo fmt --all -- --check`, strict workspace Clippy, one all-feature
release build. No live network, full workspace suite, install, or Git gate.

## Progress

- [x] Existing SSH, network sandbox, delegate catalog, and tool-routing boundaries inspected.
- [x] Typed remote contract and codex-team runner implemented.
- [x] CA automatic handoff and approval integration implemented.
- [x] Focused and completion validation recorded.

## Decisions

- Decision: keep the repository MCP delegate network-disabled and use the same
  validated codex-team executable through a separate typed subprocess protocol.
  Evidence: CA currently refuses a network-bearing mechanistic server and its
  sandbox policy would otherwise grant all repository delegation network access.
  Consequence: existing profiles and MCP catalogs stay compatible; remote
  automation is an explicit nested opt-in.

## Surprises and Discoveries

- The repository already contains stable learned evidence for rsync,
  Transmission RPC, and seedbox workflows, but only as inert prompt/learning
  artifacts; no typed execution authority exists yet.
- Keeping remote execution outside the repository MCP server preserved its
  exact network-disabled delegate catalog while still reusing the validated
  codex-team binary and CA approval path.

## Recovery and Rollback

All changes are additive. Disable the nested remote configuration or remove the
new virtual tools/protocol variants; no repository or remote content is mutated
by tests.

## Completion Record

Changed files: shared protocol, codex-team remote runner/CLI, CA configuration,
tool registry/selection, mechanistic routing, sandboxed subprocess execution,
focused tests, and the three allowed harness records.

Commands and results: focused protocol/runner/config/routing/approval/exclusion
tests passed; codex-team's full MCP SDK fixture passed 8/8; a real hermetic
CA-to-release-codex-team Transmission handoff passed; formatting, strict
all-target/all-feature workspace Clippy, and one all-feature workspace release
build passed. The recurring macOS debug-linker compact-unwind warning remained
non-fatal.

Remaining risks: this slice intentionally supports fixed SSH/rsync and
read-only Transmission RPC only. Live host compatibility, credentials, and
remote mutation were not exercised. No runtime profile or endpoint manifest was
created.

Harness updates: `NEXT_STEPS.md` marks INT-197 complete and `TESTING.md` contains
one concise completion result. No continuity, architecture, oracle, reference,
or protected-tree file changed.
