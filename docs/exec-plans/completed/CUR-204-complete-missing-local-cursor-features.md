# CUR-204 — Complete Missing Local Cursor Features

Status: COMPLETE (2026-08-29)
Owner/session: CUR-204 goal session
Last updated: 2026-08-29

## Goal

Implement, validate, document, and install all non-subscription local Cursor
capabilities tracked by CUR-204 while preserving CA safety, compatibility, and
unrelated working-tree changes.

## Non-Goals

- No hosted account, subscription, cloud-agent, telemetry, billing, remote
  marketplace, hosted PR discovery, or provider-account work.
- No runtime parity claim for Cursor `2026.07.09-a3815c0`; the clean bundle is
  implementation evidence only.
- No changes under `oracles/` or `reference/`, no provider request, and no
  credential inspection.
- No commit, reset, clean, or unrelated formatting.

## Evidence and Current Behavior

- CA already has multi-root session state, explicit worktree lifecycle,
  verified plugins, provider-safe tools, jobs, MCP, skills, sessions, planning,
  usage, permissions, and `ca.serve.v1`; the missing surfaces are narrower than
  a general capability gap.
- Launch parsing and orchestration are centered in `src/main.rs`; workspace,
  jobs, plugins, prompt commands, service, session, markdown, and TUI boundaries
  are in their named source modules.
- The pre-CUR dirty tree has 556 tracked/indexed paths and 254 untracked paths.
  The canonical `git status --porcelain=v1 --untracked-files=all` SHA-256 is
  `02fd90e56932bc57a3de507368c86d060628a48eb8ad40a3d3a83fbb400a2d29`.
  Its exact gzip/base64 payload is retained in
  `CUR-204-dirty-baseline.status.gz.b64` and decodes to 810 newline records.
- Direct status and staged/unstaged diffs for `oracles/` and `reference/` were
  empty at program start.

## Scope

Allowed files: additive modules and focused edits under `src/`, Cargo manifests
and lockfile, focused CLI/TUI/protocol tests and snapshots, `ARCHITECTURE.md`,
`CONTINUITY.md`, `NEXT_STEPS.md`, `PARITY.md`, `TESTING.md`, `docs/.INDEX.md`,
and CUR-204 plans.

Forbidden files/areas: `oracles/`, `reference/`, user configuration/sessions,
provider credentials/traces/ledgers, arbitrary plugin repositories, Git history,
and unrelated dirty bytes.

Runtime state affected: disposable tests and build output until REL-215. After
all gates pass, one recoverable atomic install to `/Users/aji/.local/bin/ca` is
authorized. No paid or hosted request is authorized.

## Design and Invariants

- Child plans execute in order: WSP-205, KNL-206, JOB-207, PLG-208, ENV-209,
  HED-210, OBS-211, TUI-212, ACP-213, CLI-214, REL-215.
- New formats are additive/versioned and older configs/sessions default without
  eager rewrites. Process-only launch flags never silently mutate persistence.
- One canonical `WorkspaceContext` and one redacted `RunEnvironment` cross all
  relevant registries and process launches; authority cannot diverge by surface.
- Host-authored job completion, permissions, environment metadata, and imported
  history cannot impersonate user input or become provider/config state.
- Every mutable manager uses native roots, preview/confirmation, digest/drift
  checks, atomic writes, private modes, and collision-safe bounded input.
- ACP is stable v1 wire over stdio only and stays byte-independent from both
  `ca.serve.v1` and SEC-158 service-v2 work.
- Only the final validated release candidate is installed; installation failure
  restores the preserved prior binary.

## Active-Plan Overlap Audit

- `REL-156`: Cargo/release/CI and final readiness. CUR-204 extends validation
  without rewriting REL-156 ownership; final install is recorded separately.
- `SEC-158`: service, approval, sandbox, CLI, and TUI orchestration. ACP remains
  a separate transport and cannot expose privileged-secret flows.
- `AGT-157`: agent budgets and completion. JOB-207 uses host-owned job events
  without weakening child budgets or attribution.
- `INT-151`, `LRN-150`, `LRN-166`, `LRN-170`: central TUI, sessions, commands,
  memory, skills, and background activity. CUR-204 preserves their schemas and
  adds defaulted fields/surfaces only.

## Milestones

### M1 — Workspace and knowledge foundations

Execute WSP-205 and KNL-206, proving catalog safety, multi-root propagation,
conditional rule semantics, and guarded native command/rule management.

### M2 — Lifecycle, plugins, and environment

Execute JOB-207, PLG-208, and ENV-209, proving exactly-once background delivery,
process-only plugin loading, and redacted approved environment propagation.

### M3 — Launch, observability, and terminal UX

Execute HED-210, OBS-211, and TUI-212 with deterministic CLI, PTY, watchdog,
snapshot, Mermaid, theme, link, focus, and title coverage.

### M4 — Interoperability and automation

Execute ACP-213 and CLI-214 with official SDK fixtures, stable schemas,
permission closure, import/diff bounds, and opt-in output compatibility.

### M5 — Integrated release

Execute REL-215, reconcile documentation and baseline ownership, run every
completion gate, install once recoverably, and verify exact installed bytes.

## Acceptance Criteria

- Every child plan's public interfaces and scenario matrix pass as specified in
  its plan; no placeholder command or silently ignored flag counts as complete.
- Default behavior and existing outputs remain compatible when opt-in flags are
  absent; old persistence loads through defaults.
- Protected evidence remains unchanged and every post-program dirty path is
  attributable to the initial baseline or CUR-204.
- Both workspace matrices, formatting, strict all-feature Clippy, all-feature
  release, smokes, signature, diff/snapshot hygiene, and protected-tree verifier
  pass before installation.
- Candidate, staged, and installed SHA-256 values match; executable ownership,
  mode, path lookup, help, version, and representative local commands pass.

## Validation

Focused: recorded in each child plan.

Completion:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`
- `cargo test --workspace --all-targets --all-features --no-fail-fast -- --test-threads=1`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo build --workspace --release --all-features`
- release help/version/new-subcommand smokes, strict signature, `git diff
  --check`, zero `.snap.new`, protected-tree direct checks and verifier, and
  exact baseline comparison.

Manual/visual: reviewed wide/narrow manager, copy, Mermaid, worktree, theme,
logs, and debug-render states plus PTY behaviors.

Blocked checks: none currently. Provider and hosted checks are deliberately
excluded.

## Progress

- [x] Goal created with the exact CUR-204 objective.
- [x] Exact dirty baseline and protected-tree state captured.
- [x] Active-plan overlaps recorded.
- [x] Umbrella and child ExecPlans plus queue entries created.
- [x] WSP-205 complete.
- [x] KNL-206 complete.
- [x] JOB-207 complete.
- [x] PLG-208 complete.
- [x] ENV-209 complete.
- [x] HED-210 complete.
- [x] OBS-211 complete.
- [x] TUI-212 complete.
- [x] ACP-213 complete.
- [x] CLI-214 complete.
- [x] REL-215 complete and installed.

## Decisions

- Decision: use source evidence, not Cursor runtime parity language.
  Evidence: the clean Cursor bundle is not runnable in the clean harness.
  Consequence: implemented items are documented as CA features and evidence-
  based comparisons.
- Decision: preserve exact initial status as a compressed canonical artifact.
  Evidence: 810 pre-existing paths make prose summaries insufficient for final
  attribution.
  Consequence: REL-215 can compare exact records without resetting user work.

## Surprises and Discoveries

- The baseline grew from the earlier review to 810 paths, including seven active
  untracked plans and extensive snapshot work; central-file edits require narrow
  patching and focused retesting.
- WSP-205 confirmed that the existing `WorkspaceContext` was already the right
  authority boundary. Catalog resolution now occurs before provider and registry
  construction, while session resumes remain authoritative and conflict with
  launch-only root flags.
- KNL-206 preserved legacy rule headings while adding stable scoped IDs. Rules
  now activate only from host-supplied explicit paths or saved manual state;
  ordinary prompt prose and model requests cannot activate them. Native rule
  and command writes share one digest-bound, drift-rejecting mutation boundary.
- JOB-207 made origin and delivery separate job invariants. Foreground
  promotion is process-preserving; explicit await and automatic completion use
  one exactly-once marker; headless runs use independent job owners so fixed
  report/run identifiers cannot leak completion state across runs.
- PLG-208 composes process-only plugin roots through the existing declarative
  validation and collision boundary without registry persistence. The TUI and
  CLI now share one signed-catalog result path, keeping trust and staging rules
  identical across both interfaces.
- ENV-209 establishes one redacted process-local environment authority across
  providers and child surfaces. Direnv is explicit, trusted, digest-bound,
  sandboxed, and network-denied; overlays are workspace-bound and cannot follow
  a primary-root change. Zsh integration is CA-native, exactly marked, atomic,
  recoverable, and was tested without mutating the user's shell profile.
- HED-210 adds fresh plan/ask startup, process-effective sandbox and headless
  trust, registered launch worktrees, typed native/compatibility setup, exact
  headless digest authority, an asynchronous Cancel-first TUI preview, and the
  preview-only `ca repo worktree-setup` command. Worktree creation replaces only
  the primary root before workspace registries are built; supplemental roots
  remain ordered and setup failure retains the registered tree.
- TUI-212 adds sanitized message/part copy selection, bounded off-loop Mermaid
  rendering, destination-preserving Markdown with fail-closed OSC 8 links,
  focus-gated notifications, optional restoring terminal titles, and bounded
  asynchronous OSC 11 theme detection that never overrides a manual theme.
  Both workspace matrices and every child gate pass.
- ACP-213 adds stable ACP v1 newline JSON-RPC stdio through the exact official
  SDK 2.0.0 dependency, with local session/provider/tool reuse, agent/plan/ask
  modes, streamed updates, permission closure, cancellation, and disconnect
  cleanup. The SDK's transitive JSON object-order feature exposed five existing
  byte assumptions; explicit sorted compatibility serialization preserves the
  old CA contracts. Both workspace matrices and every child gate pass.
- CLI-214 adds strict pre-initialization SemVer gating, bounded ephemeral
  session-export history, collision-checked caller UUIDs, workspace-context
  exclusion with exact explicit-skill retention, bounded host Git context,
  redacted key-only structured environment observations, and versioned local
  repository metadata. Its third exact all-feature workspace run passed every
  target after two documented protected timing flakes.
- REL-215 reconciled every child and harness document, accounted for all 810
  baseline records plus exactly 58 CUR-owned additions, passed both exact
  matrices and all release/hygiene/protected gates, and installed one verified
  candidate recoverably. The failed smoke-harness attempts restored the prior
  binary before the corrected final transaction passed.

## Recovery and Rollback

Reverse only CUR-204-labeled hunks and delete only CUR-204-created files with
`apply_patch`; never reset or clean. Persistence tests use disposable roots.
Installation keeps a timestamped SHA-labeled recovery binary and restores it if
any integrity check fails.

## Completion Record

Changed areas: additive/local feature modules under `src/`; focused updates to
central CLI, session, provider, permission, job, Markdown, TUI, plugin, skill,
workspace, and headless boundaries; Cargo dependency metadata; 26 reviewed
snapshots; and the repository harness plus eleven completed child plans. Exact
inventory attribution is recorded by REL-215. No oracle, reference, protected
codex-team, provider configuration, or user session path changed.

Validation: focused child suites; clean exact default and all-feature workspace
matrices; formatting; strict all-feature Clippy; all-feature release; local
CLI/ACP/repository/shell smokes; strict code signature; diff and snapshot
hygiene; exact protected status/diff; protected-tree verifier; and exact dirty
inventory audit all pass. Repeated matrix timing flakes in unchanged protected
tests and their successful isolated reruns are retained in the child/REL records.

Installed release SHA-256:
`30660c4e21598363feb3c99d11f271716f84e6f3e95907d07f633faa0af83111`.
Recovery SHA-256:
`82c72a53a0092c1a274a92b550d5790d530fef39515247bd525144ec1fd603ea`.

Remaining risk at CUR-204 completion: the protected five-client scheduler
timing test remained flaky under repeated full-suite load, and CUR-204 did not
own or modify that subtree. Post-completion TST-216 received explicit ownership
and replaced the fixed-delay assumption with a deterministic five-consumer
barrier without changing production scheduler behavior. Post-completion
TST-217 then repaired the separate shared-query cache-publication `EEXIST` race
with monotonic temporary-name sequences and bounded collision retry. Both
complete workspace matrices pass, leaving no remaining CUR-204 timing/cache
risk from those cases and no implementation, validation, documentation, or
installation work.
