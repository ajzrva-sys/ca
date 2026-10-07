# IMP-227 — Safe Claude Setup Import

Status: COMPLETE
Owner/session: ORC-218 goal session
Last updated: 2026-08-29

## Goal

Add previewed, digest-bound, transactional import of inert Claude instructions,
rules, commands, and valid skill packages into native CA roots.

## Non-Goals

- Never import Claude settings, credentials, hooks, MCP configuration,
  permissions, shell initialization, account state, executable authority, or
  provider data.
- No symlink following, overwrite, collision replacement, bare-profile apply,
  provider request, runtime configuration/session mutation, oracle/reference
  edit, protected-tree edit, or binary installation.

## Evidence and Baseline

- Native rule, prompt-command, and portable-skill validators already define the
  only accepted destination formats.
- CLI dispatch and TUI modal workers already provide read-only preview followed
  by an explicit Cancel-first mutation boundary.
- ORC-218 baseline: 868 records, SHA-256
  `2dcaf84dc86d3ae27bfea593bc811ce16f46f9782382780901fdcf890ff39978`.

## Design

- Inventory at most 256 regular, single-link files, 256 KiB each and 8 MiB
  aggregate, rejecting symlinks, special files, path escape, invalid UTF-8,
  malformed frontmatter/manifests, and invalid native identities.
- Map `CLAUDE.md` and `.claude/rules` to native conditional rules,
  `.claude/commands` to inert native prompt commands, and valid
  `.claude/skills` packages to native skill roots. List excluded sensitive and
  unsupported files by category without retaining their contents.
- Bind schema, canonical source/target, scope, every source and rendered target
  digest, skipped inventory, collision state, and byte counts into one SHA-256.
  Apply only a freshly reproduced exact digest.
- Stage all output privately, publish with no-overwrite links, synchronize
  files/directories, and remove only transaction-owned paths on any failure.

## Validation

Cover CLI and slash shape, user/project mapping, sensitive skips, native
validation, symlink/hardlink/path/bounds refusal, collision/drift, digest-bound
apply, rollback, bare refusal, idempotent no-overwrite behavior, TUI preview,
and full child gates without provider traffic or installation.

## Progress

- [x] Child plan and queue entry created after EDT-226 completion.
- [x] Core bounded preview and transactional publication implemented.
- [x] Cancel-first TUI preview/apply integrated.
- [x] Focused suites and child completion gates pass.
- [x] Harness reconciled and plan completed.

## Completion Evidence

- `ca import setup claude --path DIR --scope user|project [--json]
  [--apply-digest SHA256]` and `/import setup claude DIR` use the versioned
  `ca.setup-import-preview.v1` plan. The TUI performs preview/apply on a worker,
  is Cancel-first, supports detail scrolling, and reloads native registries
  only after an exact successful apply.
- The importer inventories at most 256 regular single-link files, 256 KiB each
  and 8 MiB aggregate. It maps only `CLAUDE.md`, conditional rules, inert
  commands, and CA-valid skill packages; sensitive settings, credentials,
  hooks, MCP, permissions, shell, and account files are listed without content.
  Source/target digests, scope, canonical roots, exclusions, collision state,
  and byte counts bind the apply digest.
- Staging uses private files and directories; publication is no-overwrite and
  synchronized, and injected failure removes only transaction-owned files.
  Focused setup/security/CLI/input/render coverage passes 10/10 plus slash and
  command-registry checks. The complete CA target passes 2,370 tests with four
  documented ignores; formatting, strict CA all-feature Clippy, all-feature
  release, help/schema smokes, diff/snapshot hygiene, and direct
  oracle/reference inspection pass. No real import, provider request, runtime
  profile/session mutation, protected-tree edit, or installation occurred.

## Recovery

Reverse only IMP-227-labelled hunks and delete only IMP-created paths with
`apply_patch`. Never reset or clean, overwrite imported assets, mutate
oracle/reference or protected trees, contact a provider, alter user runtime
state, or install a binary.
