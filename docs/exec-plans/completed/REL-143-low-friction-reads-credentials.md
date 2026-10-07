# REL-143 — Low-Friction Reads and Credential Migration

Status: COMPLETE
Owner/session: Codex / current user-authorized implementation and live delivery
Last updated: 2026-08-18

## Goal

Existing legacy profiles transparently use AutoReview for proven-safe local
reads, and locally rebuilt macOS releases stop requesting recurring Keychain
authorization after a one-time provider-credential import into ca's private
file store.

## Non-Goals

- Do not auto-authorize writes, deletions, network work, unsafe/unknown shell
  commands, explicit Ask/Deny matches, outbound email, or paths outside declared
  workspace roots.
- Do not change provider wire protocols, session formats, Codex OAuth, or the
  dedicated email-gateway Keychain.
- Do not delete legacy provider Keychain items, modify oracle/reference
  evidence, commit, or contact a paid provider.

## Evidence and Current Behavior

- `/Users/aji/.config/ca/config.toml` contains the exact legacy value
  `approval_mode = "allowlist"`; it predates the default AutoReview change.
- `permission::auto_review_safe_invocation` already classifies registry-declared
  read-only tools such as `list_dir` and closed read-only shell pipelines as
  safe, but `main::tool_needs_approval` asks for every unmatched tool in
  `ApprovalMode::Allowlist`.
- `/auto-review` currently changes only process state and does not persist.
- The installed `/Users/aji/.local/bin/ca` has an ad-hoc designated requirement
  bound to one cdhash. Rebuilt binaries therefore do not retain the same legacy
  Keychain ACL identity.
- `provider_secrets::FileProviderSecretStore` already provides owner/symlink
  checks, 0700 directories, 0600 files, bounded reads, and atomic replacement,
  but macOS currently selects only `SystemProviderSecretStore` backed by the
  legacy `ca.provider-api-key` Keychain service.
- Inferred: a file-primary composite store with Keychain fallback only on a
  missing private credential yields one import per legacy item and no later
  Keychain access while preserving rollback.

## Scope

Allowed files:

- `src/{config,main,provider_secrets}.rs` and focused tests
- standard harness pages and this plan
- after all source gates pass, the explicitly authorized live config and
  installed binary with timestamped backups

Forbidden files/areas:

- `oracles/`, `reference/`, email gateway secrets, Codex OAuth, provider
  endpoints/traffic, sessions, Git history, unrelated dirty-worktree hunks

Runtime state affected:

- On validated delivery, `/Users/aji/.config/ca/config.toml` migrates the exact
  legacy approval value, `/Users/aji/.local/bin/ca` is atomically replaced, and
  configured legacy provider keys are copied into profile-scoped private files.
  The previous binary/config and legacy Keychain items remain recoverable.

## Design and Invariants

- Exact saved `allowlist` is a legacy marker and atomically becomes
  `auto-review`; canonical `ask` remains the strict mode.
- Config mutation uses `toml_edit`, retains value decoration/comments, and
  leaves unrelated settings byte-semantically intact.
- `/auto-review` persists `auto-review` or `ask` before changing interactive
  state; bare mode remains process-only.
- Existing registry mutation metadata and the closed command parser remain the
  AutoReview authority. Workspace preparation, explicit policy, and network
  decisions retain precedence.
- macOS provider credentials use the existing profile data root and file store
  as primary. A primary miss alone consults the legacy Keychain, then requires
  a successful private atomic write before returning the imported secret.
- New saves/deletes target only the private primary. Legacy items remain inert
  rollback copies and are never read while a primary file exists.
- No diagnostic, Debug value, config field, snapshot, or test output exposes a
  credential.

## Milestones

### M1 — Characterization

- Add failing tests for exact legacy config migration, persistent command
  toggling, and composite secret-store import boundaries.
- Validation: run each exact focused filter before implementation.

### M2 — Approval Migration

- Add the comment-preserving load migration and canonical saved mode update.
- Persist `/auto-review` outside bare mode and retain all approval precedence.

### M3 — Provider Credential Migration

- Generalize the private file store to macOS and add a file-primary,
  Keychain-fallback composite.
- Cover one-time import, errors, profiles, permissions, deletion, redaction,
  and legacy rollback behavior without real Keychain access.

### M4 — Validation and Live Delivery

- Run focused and required completion gates.
- Back up and atomically install only after all source gates pass.
- Trigger offline config/credential migration, verify metadata only, and prove
  the second load does not request Keychain access.
- Reconcile harness records and move this plan to completed.

## Acceptance Criteria

- A legacy `allowlist` profile starts in AutoReview and is persisted as
  `auto-review`; `ask` remains strict.
- `/auto-review` persists its next-launch behavior, except in bare mode.
- The exact reported `list_dir` invocation does not enter approval under the
  migrated profile; mutation/network/explicit-policy boundaries still do.
- Existing provider references load from private 0600 files. When absent, each
  legacy Keychain item is imported once and never consulted on later loads.
- The installed release and live default profile exhibit the behavior without
  paid provider traffic, and rollback artifacts exist.

## Validation

Focused:

- The pre-fix characterization failed to compile with the expected missing
  `update_approval_mode` and `MigratingProviderSecretStore` boundaries.
- All 72 configuration-store tests, 10 provider-secret tests, 10 permission
  tests, six focused main AutoReview tests, one CLI AutoReview policy test, and
  the exact SES-142 exit-handoff regression pass.

Completion:

- Default suite: 1,610 passed main tests and 37 passed CLI tests; one ignored.
- All-target/all-feature suite: 1,639 passed main tests, 118 passed experiment
  tests, and 37 passed CLI tests; two ignored.
- Formatting, strict all-target/all-feature Clippy, all-feature release,
  released help/version, diff/snapshot hygiene, and oracle/reference
  immutability pass.

Manual/live:

- The release was installed atomically after timestamped binary/config backups.
- The live config migrated to `auto-review`; two configured legacy provider
  credentials imported with user authorization into regular 0600 files below
  a 0700 directory. Contents were not inspected.
- A second secret-resolving load completed in 0.01 seconds without another
  Keychain prompt. Offline doctor reports Ready, and no provider request ran.

Blocked checks: none.

## Progress

- [x] Traced live config, installed code identity, approval routing, and secret
  store selection.
- [x] Added failing characterizations.
- [x] Implemented approval migration and persistence.
- [x] Implemented private provider credential migration.
- [x] Passed focused and completion gates.
- [x] Backed up, installed, migrated, and verified live behavior.
- [x] Reconciled harness and completed the plan.

## Decisions

- Decision: migrate only exact legacy `allowlist`; retain `ask` as strict.
  Evidence: the live file predates AutoReview and the user selected legacy
  migration rather than changing every mode's semantics.
  Consequence: security-conscious strict profiles have a canonical opt-in.
- Decision: use private files for locally rebuilt macOS provider credentials.
  Evidence: Apple legacy Keychain ACLs track the binary designated requirement,
  while this local release is ad-hoc/cdhash-bound; the repository already owns
  a hardened private file store.
  Consequence: provider secrets are plaintext-at-rest but owner-only, and local
  rebuilds no longer create repeated authorization prompts.
- Decision: retain legacy Keychain items.
  Evidence: the user requested recoverable delivery and the old binary depends
  on those items.
  Consequence: rollback remains functional; cleanup is a separate explicit task.

## Surprises and Discoveries

- The installed binary already contains the SES-142 handoff and current
  AutoReview classifier; delivery/config migration, not classification, caused
  the screenshots.
- The shared private file store required no new persistence format; macOS only
  needed to select it as primary and retain the prior Keychain adapter as a
  load-only fallback through the composite.

## Recovery and Rollback

The original installed binary is retained at
`/Users/aji/.local/bin/ca.backup-rel143-20260818-211356` with SHA-256
`408012008cad5712507975d7892d85277c3e8cee64f303141f8721546f6260bd`.
The original config is retained at
`/Users/aji/.config/ca/config.toml.backup-rel143-20260818-211356` with SHA-256
`6411925554bd3bc1ffc3561f49e91027f3d4fce3be295f1f10026ae395ddd720`.
An intermediate installed artifact is retained at
`/Users/aji/.local/bin/ca.backup-rel143-prehardening-20260818-212102`.
The legacy Keychain items remain intact, so the original binary can be restored
by an atomic same-directory replacement if required.

## Completion Record

Changed files:

- `src/config.rs`: exact legacy migration and canonical saved-mode update.
- `src/main.rs`: persistent `/auto-review` and focused approval/session tests.
- `src/provider_secrets.rs`: private-file primary, one-time Keychain fallback,
  and migration/storage boundary tests.
- `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `TESTING.md`,
  `NEXT_STEPS.md`, and `docs/.INDEX.md`: durable contract, evidence, and status.

Commands and results: all focused and completion commands listed in Validation
passed. The final installed and tested release SHA-256 is
`a6be8911f5ef4317bc21257708886d482d5849da95d69946bd99bc5fc7a9036f`;
the migrated config SHA-256 is
`c886a9aab0d7dc7d4f3381bfd2a180f379659b07ab840dd51ac3f92d5904fc27`.

Remaining risks: provider secrets are intentionally plaintext at rest inside
owner-only files rather than encrypted by Keychain. This removes rebuild-bound
ACL prompts while retaining filesystem permission, ownership, no-follow,
bounded-read, atomic-replacement, and rollback protections. Legacy Keychain
cleanup, if ever desired, remains a separate explicit destructive task.

Harness updates: architecture, environment, continuity, testing, next-step,
and knowledge-index records now describe the implemented and installed state.
This is a ca-specific ergonomics/storage change rather than Cursor parity, so
`PARITY.md` required no update. Oracle/reference files remained unchanged.
