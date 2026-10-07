# PLG-085 — Signed Curated Catalog

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Add an explicit profile-scoped catalog for locally supplied, Ed25519-signed curated snapshots so users can pin trust roots, inspect verified plugin versions, stage hash-checked installs/updates, and roll back without automatic download, activation, or source execution.

## Non-Goals

- Arbitrary marketplace browsing/URLs, unsigned catalogs, trust-on-first-use, remote key discovery, automatic sync/install/update/activation, dependency resolution, native/WASM/in-process code, background network traffic, or deleting user-authored plugin bundles.
- Cursor parity. This is an oracle-informed `ca` extension.

## Evidence and Current Behavior

- `PLG-084` provides strict local manifests, owner-only profile registries, deterministic bundle identity, exact-confirm enablement, and existing-extension-only composition in `src/plugin.rs`.
- The approved program requires signed manifests, pinned trust roots, staged installs, explicit updates, rollback, and no automatic activation/updating.
- Inferred: no authoritative hosted `ca` catalog/root currently exists, so this slice accepts only local catalog snapshots and package files obtained out of band. It does not invent a production URL or trust root.

## Scope

Allowed: `src/plugin_catalog.rs`, narrow `src/plugin.rs` integration, profile runtime paths, an Ed25519 verification dependency, focused core/CLI tests, and harness documents.

Forbidden: oracle/reference edits, real network/catalog services, user runtime state, installed binary, native executable loading, automatic activation, and unrelated TUI/provider/session changes.

## Design and Invariants

- `plugin catalog trust ID KEY --confirm ID` pins one bounded local 32-byte Ed25519 public key in an owner-only profile store. Replacing/removing trust requires a separately explicit exact confirmation and invalidates cached catalog metadata.
- `sync ID INDEX` verifies one strict bounded envelope whose signature covers exact payload bytes. Revisions increase monotonically; plugin IDs/versions/package-relative paths/hashes are bounded and unique. The verified payload is copied atomically into profile state; unverified source bytes are never authoritative.
- A signed entry points only to a regular non-symlink package adjacent to the supplied index. The package is a strict bounded JSON file map. Install/update verifies the signed SHA-256, validates every relative path and decoded file hash/size, writes a private staging tree, validates its ordinary PLG-084 manifest, then atomically renames the complete tree under profile data.
- Install, update, and rollback require exact plugin confirmation. They register/switch the ordinary local plugin record disabled. Updates never overwrite local-source plugins; old catalog versions remain for explicit rollback. No operation enables a plugin.
- Catalog state and safe output contain only IDs, versions, hashes, public trust keys, and local package metadata. Package content, hook/MCP commands, prompts, headers, resolved credentials, and signatures do not enter Debug/list output.

## Milestones

### M1 — Signed state and verification
- Add strict trust/catalog/package types, Ed25519 exact-byte verification, owner-only atomic profile storage, traversal/symlink/size/revision tests.

### M2 — Staged lifecycle
- Add explicit CLI trust/sync/list/install/update/rollback, private staging, signed hash/file validation, disabled PLG-084 registration, collision/local-source protection, and rollback tests.

### M3 — Completion
- Run focused/full gates and a released disposable-home signing/install/update/rollback walkthrough; reconcile docs and archive.

## Acceptance Criteria

- Wrong keys/signatures, stale revisions, hash mismatch, traversal, symlink, duplicate entries/files, oversized input, and local-source overwrite fail before registry mutation or extension execution.
- A verified package installs atomically and disabled; explicit update retains the previous tree and remains disabled; explicit rollback selects the prior verified tree and remains disabled.
- Nothing fetches or activates automatically. Bare refuses catalog state. Named profiles remain isolated.

## Validation

Focused plugin/catalog tests passed 14/14 and the signed-catalog CLI integration passed. Repository gates passed 1019 unit tests plus 33 CLI tests, formatting, strict Clippy, release build, and nested help/version smokes. A released disposable-home walkthrough proved exact-confirm trust, two signed revisions, inert install/update, zero-traffic pre-enable refusal, retained verified rollback, explicit activation, one v1-only loopback request, owner-only state, and no session; its fixture moved recoverably to `/Users/aji/.Trash/ca-plg085-walk-20260812`.

## Progress

- [x] PLG-084 archived and local plugin registry seams characterized.
- [x] Signed trust/index/package validation passes.
- [x] Explicit staged install/update/rollback passes.
- [x] Full gates/walkthrough/docs pass; plan archived.

## Decisions

- Decision: PLG-085 is local-snapshot-only until an authoritative catalog URL and root are separately established.
  Evidence: no approved production service/root exists, and arbitrary third-party marketplace URLs are explicitly out of scope.
  Consequence: users obtain signed index/package files out of band; `ca` performs no catalog network traffic.

## Recovery and Rollback

Disable the affected ordinary plugin. An explicit catalog rollback switches its disabled registry record to a retained verified version; catalog source packages and unrelated plugin/user state remain untouched.
