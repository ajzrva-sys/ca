# INT-198 — Guided Typed Automation Setup

Status: COMPLETE
Owner/session: current CA/codex-team integration session
Last updated: 2026-08-27

## Goal

A normal CA Main/headless agent can turn a natural-language setup request into
one codex-team-validated automation proposal, ask the user only for missing
non-secret details, and create the active profile wiring plus private automation
manifest after one explicit approval. The same setup tools accept future typed
templates without adding another CA tool family.

## Non-Goals

- Arbitrary config text, shell commands, credential bytes, live login/probing,
  remote mutation, provider switching, or a new service.
- Automatic approval, default/bare/excluded-surface writes, installed-binary
  changes, or changes to the existing MCP catalogs/resources.
- A general-purpose file writer or untyped plugin installer.

## Evidence and Current Behavior

- `src/interaction.rs` and `request_user_input` already provide ordered typed
  user questions and zeroizing masked answers.
- `src/agent/registry.rs` and the central approval broker already classify and
  resolve mutating tools exactly once.
- INT-197's `codex-team remote execute` validates a private named manifest, but
  users must currently author that manifest and `[mcp.mechanistic.remote]`
  manually.
- `ConfigStore` already uses private atomic profile writes. Inferred: a typed
  codex-team preview/apply protocol can extend that safety without parsing
  arbitrary model-generated TOML.

## Scope

Allowed files:
- shared codex-team protocol and codex-team typed setup module/CLI/tests
- CA config-store transaction, MCP subprocess bridge, registry/selection,
  mechanistic routing/system guidance, and focused tests
- this plan, one `NEXT_STEPS.md` entry, and one concise `TESTING.md` result

Forbidden files/areas:
- `oracles/`, `reference/`, user runtime configuration, credentials, installed
  binaries, Git staging/commits, services, and unrelated files

Runtime state affected: temporary test profiles only.

## Design and Invariants

- CA advertises generic `list_automation_templates`,
  `preview_automation_setup`, and `apply_automation_setup` only in ordinary
  Main/headless runs with mechanistic delegation and config writes available.
- The first template is `remote_automation`; its draft contains bounded named
  SSH hosts, read-only Transmission endpoints, and sync profiles. Values are
  structural fields, never TOML or command text. Credentials are uppercase
  environment-variable names only.
- CA supplies the active profile path, pinned repository, and exact validated
  codex-team binary out of band. The model cannot choose write destinations or
  executables.
- Preview performs no writes and returns redacted effects plus a SHA-256 digest
  bound to current profile bytes, target absence/content, binary, repository,
  and normalized draft.
- Apply recomputes the preview, requires the exact digest, writes a private
  manifest and profile patch atomically, and rolls back the manifest if the
  profile write fails. Existing unrelated profile bytes and tables are
  preserved; collisions, symlinks, hardlinks, drift, and bare mode fail closed.
- Setup does not start the delegate server or contact a remote endpoint. The
  configured automation remains subject to INT-197's ordinary network/mutation
  approval and dry-run digest.

## Milestones

### M1 — Typed template and preview
- Add the generic setup draft/request/receipt contract and remote template.
- Generate deterministic private manifest/profile changes without writing.
- Test bounds, invalid destinations/URLs/names, redaction, and drift digests.

### M2 — Approved transactional apply
- Add exact-digest application and rollback-safe private writes.
- Add CA generic setup tools, routing, approval metadata, and exclusions.
- Test no-write preview, one approved apply, collision/drift refusal, and
  current-profile reload guidance.

### M3 — Natural-language handoff and validation
- Guide the model to discover the template, ask for missing fields, preview,
  summarize, and apply only after approval.
- Prove one hermetic CA-to-codex-team setup using temporary runtime paths.
- Run focused tests, formatting, strict Clippy, and one release build.

## Acceptance Criteria

- “Set up my server and Transmission seedbox” causes the model-visible template
  tools to gather names/addresses/paths and environment-variable names, not
  secret values or TOML.
- Preview changes no bytes and describes the exact named capabilities.
- Approved apply creates one mode-0600 manifest and narrowly patches the active
  non-bare profile; a repeated identical apply is idempotent.
- Profile/manifest drift, collision, invalid fields, missing base delegation,
  or denial produces no partial write.
- Plan/review/checkpoint/service/scheduled/subagent/bare/role-run surfaces omit
  the tools; exact 11/15/28 MCP catalogs and zero resources remain unchanged.
- The resulting profile loads and INT-197 can list the generated remote names
  without network or credentials.

## Validation

Focused: protocol/setup/config/registry/routing/approval/exclusion and one real
temporary CA-to-codex-team setup fixture.

Completion: formatting, strict workspace Clippy, one all-feature release build,
release help, and existing MCP compatibility. No live remote/provider traffic,
installation, full workspace suite, Git gate, or protected-tree check.

## Progress

- [x] Existing typed interaction, config-write, approval, and remote boundaries inspected.
- [x] Generic setup contract and codex-team preview/apply implemented.
- [x] CA guided setup tools and exclusions implemented.
- [x] Focused and completion validation recorded.

## Decisions

- Decision: keep detail gathering in the existing CA model turn and make
  codex-team the deterministic validator/writer.
  Evidence: CA already owns typed human interaction and codex-team owns the
  mechanistic execution contract.
  Consequence: no nested provider loop or recursion path is introduced.

## Surprises and Discoveries

- The current remote tool family is hidden until remote automation is already
  enabled, so setup tools need a distinct selection gate based on base
  mechanistic delegation plus profile write capability.

## Recovery and Rollback

Tests use injected temporary profile roots. Runtime application is exact-digest
and transactional; a failed profile patch removes a newly created manifest or
restores its prior verified bytes.

## Completion Record

Changed files: shared setup protocol; codex-team setup generator/CLI and remote
manifest validation seam; CA MCP bridge, registry/selection, guided routing,
approval and excluded-runner constraints; focused tests; allowed harness records.

Commands and results: all 9 protocol tests; 2 setup transaction tests; 11 CA
registry tests; 17 tool-selection tests; explicit AutoReview approval regression;
8/8 existing MCP SDK tests; and real debug/release CA-to-codex-team temporary
profile handoffs passed. Formatting, strict all-target/all-feature workspace
Clippy, one all-feature release build, release setup help, and CA version passed.

Remaining risks: the current template covers named SSH, read-only Transmission,
and rsync automation. Adding another domain requires a new closed protocol
template and codex-team generator, but no new CA setup tools. The active process
must restart after apply because its model tool catalog is frozen. Live hosts,
credentials, and remote writes were not exercised.

Harness updates: `NEXT_STEPS.md` marks INT-198 complete and `TESTING.md` records
one concise result. The full workspace test suite, installation, live traffic,
Git gates, protected-tree checks, oracles, and reference files were not used.
