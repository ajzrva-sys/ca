# EXT-088 — Signed External Runtime Manager

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-12
Archive: completed after all focused, repository-wide, release, and signed-loopback lifecycle gates passed.

## Goal

Add an explicit profile-scoped `ca runtime` CLI that pins exact catalog authorities, fetches only an explicitly requested signed catalog or artifact, previews exact disk/network changes, installs and updates hash-verified external runtime files through private staging, retains verified rollback versions, resolves a verified entrypoint, and removes only manager-owned runtime trees after exact confirmation.

## Non-Goals

- Automatically checking, downloading, updating, executing, or activating a runtime.
- Installing `ca`, browsers, tmux, LSP servers, models, package managers, system services, or privileged files.
- Making current LSP, tmux, browser, provider, or voice discovery consult managed runtime paths.
- Archive extraction, install scripts, post-install hooks, dependency resolution, arbitrary headers, URL credentials, mirrors, or non-HTTP transports.
- Claiming Cursor or Ante runtime parity.

## Evidence and Current Behavior

- `src/plugin_catalog.rs` already proves local Ed25519 root pinning, monotonic signed snapshots, private atomic state, staged content verification, retained rollback, and profile isolation. Its catalog/package types and helpers are private and plugin-specific; external executables need a separate closed manifest and cannot be registered as plugins.
- `src/runtime.rs::RuntimePaths` derives profile-owned data roots but has no external-runtime location.
- `src/main.rs::parse_launch_args` reserves only leading service/schedule/plugin tokens, preserving ordinary positional prompts; `runtime` needs the same leading-only treatment and root help discovery.
- Existing LSP, tmux, browser, and local-provider paths discover user-installed executables or services. They must remain unchanged in this slice.
- Ante documentation describes pinned checksum-verified llama.cpp builds and self-updates, but no Ante runtime was observed and its implementation is not evidence for a generic `ca` manager. This plan adopts only the explicit verification/staging lesson.

## Scope

Allowed files:

- `src/external_runtime.rs`, `src/main.rs`, `src/runtime.rs`, CLI tests, and focused snapshots/fixtures if required.
- `docs/exec-plans/active/ext-088-signed-external-runtime-manager.md` and the standard harness documents.

Forbidden files/areas:

- `oracles/`, `reference/`, real user configuration/data, Keychain, saved sessions, installed binaries, system package managers, and current executable-discovery behavior.

Runtime state affected:

- Only an explicitly selected profile's `~/.local/share/ca[/profiles/<name>]/external-runtimes` tree after a successful confirmed command. Tests and walkthroughs use an injected temporary home and loopback server only.

## Design and Invariants

- Add leading `ca runtime --profile NAME <command>`; bare is rejected because the manager is persistent. Root help lists it without changing non-leading positional prompt parsing.
- Trust pins one lowercase catalog ID to one base URL and one local Ed25519 public key after `--confirm <catalog-id>`. Sources must be HTTPS, except literal-loopback HTTP for local development/tests; userinfo, query, fragment, non-loopback cleartext, proxies, and redirects are rejected.
- `sync` explicitly GETs only `<pinned-base>/catalog.json`, with connect/request/body caps, verifies a closed base64 Ed25519 envelope, exact catalog ID, monotonic revision, current-target entries, file sizes/hashes, relative source/path values, entrypoints, and duplicate identities before atomically saving safe metadata. HTTP bodies are never retained on failure.
- A signed runtime entry contains an ID, version, target, license label, one to 32 files, exact per-file and total byte sizes, SHA-256 hashes, executable flags, and a closed entrypoint map. No archive or command can enter the manifest.
- `preview` and every mutating command print the exact runtime/version, download bytes, installed bytes, retained-version effect, and destination root before mutation. Install/update additionally require `--confirm <runtime-id>`.
- Install/update download files sequentially only after confirmation. Every response has a whole-request timeout, a post-decompression exact-size cap, no redirects/proxy/auth, and same-origin resolution under the pinned base path. Bytes are streamed directly into a private UUID staging directory while hashing, never held as a whole package, then renamed into a version-plus-entry-digest directory and registered atomically. Failure removes staging and does not change current state.
- Installed receipts retain the exact verified semantic entry. Entry points resolve only after rechecking the receipt, complete file set, sizes, hashes, permissions, and manager-root containment. Nothing is executed by this plan.
- Update cannot change a runtime's catalog owner, rewrite a retained version, or replace an installed same-version digest. Rollback selects one already retained and reverified version without network. Remove moves exactly one validated manager-owned runtime tree aside, atomically removes registry state, then deletes it; failure restores the prior state/tree where possible. Trusted catalogs and other runtimes remain untouched.
- State, directories, files, and executables are owner-only; symlink/special-file/traversal cases fail closed. Debug output omits public-key material and source URLs. No credentials, headers, response bodies, executable bytes, or URLs enter snapshots, sessions, config, or ordinary diagnostics.

## Milestones

### M1 — Characterize CLI, trust, catalog, and storage

- Add failing tests for leading-only parsing/help, profile/bare behavior, runtime paths, URL validation, public-key/signature/revision/target validation, safe Debug, state permissions, symlinks, and no-write failures.
- Implement the closed CLI/types, profile path, private atomic state, explicit loopback/HTTPS catalog fetch, and signed catalog verification.
- Validate with `cargo test external_runtime --no-fail-fast` plus parser/runtime filters.

### M2 — Implement previews and lifecycle

- Add loopback fixtures for exact one-shot catalog/file requests, missing/wrong length, hash mismatch, HTTP errors/body-marker exclusion, redirects, timeout, oversize, traversal, cross-origin resolution, and cleanup.
- Implement safe list/preview, staged install/update, retained verification/rollback, exact entrypoint resolution, and confirmed removal/rollback recovery.
- Prove profile isolation, unchanged current state on failure, complete-file-set verification, owner-only modes, no background request, and no integration activation.

### M3 — Complete delivery gates

- Add released CLI lifecycle coverage using a generated key, signed loopback catalog, and tiny fake executable files under a temporary home.
- Run focused and repository-wide tests, formatting, strict Clippy, release build, and help/version/profile/runtime smokes.
- Reconcile `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md`; archive this plan only after actual results are recorded.

## Acceptance Criteria

- Nothing is fetched before explicit `sync`, `install`, or `update`; list/preview/rollback/path/remove are offline.
- No artifact request occurs before a valid signature, target, preview, and exact confirmation.
- HTTPS or literal-loopback HTTP is required; proxy, redirect, credentials, query secrets, base-path escape, cross-origin, and undeclared files fail before persistence.
- Install/update success leaves only exact verified files plus one private receipt; interrupted/failed downloads do not change the active version and leave no staging tree.
- Rollback and entrypoint resolution reverify every retained byte; drift, extra files, symlinks, special files, and permission widening fail closed.
- Remove affects only one exact registered runtime and supports a Cancel-by-omission confirmation boundary. Other runtimes, catalogs, profiles, config, sessions, and discovered executables remain unchanged.
- All CLI errors are bounded and secret/body-free; state/Debug never exposes public keys or base URLs.
- Focused tests, full tests, formatting, strict Clippy, release build, and disposable CLI walkthrough pass.

## Validation

Focused:

- `cargo test external_runtime --no-fail-fast`
- targeted `runtime` and leading-command parser tests

Completion:

- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`
- release help/version/profile/runtime smoke tests

Manual/visual:

- No TUI surface is added. Exercise trust, no-network list, explicit sync, preview, install, update, rollback, path, and remove with a generated test key and literal-loopback fixture in a temporary home; verify request counts, modes, retained versions, cleanup, and no session/config creation.

Blocked checks:

- No real catalog, public service, runtime executable, system installation, or external integration is used.

## Progress

- [x] Verify the current signed-catalog, runtime-path, CLI, and oracle boundaries.
- [x] Add failing command/storage/catalog characterization tests.
- [x] Implement signed catalog and safe source handling.
- [x] Implement preview/install/update/rollback/path/remove with transactional cleanup.
- [x] Run focused and repository-wide gates plus disposable CLI lifecycle.
- [x] Reconcile harness documents and archive the completed plan.

## Decisions

- Decision: packages are closed multi-file manifests, not archives.
  Evidence: later managed runtimes may need several entrypoints, while archive extraction adds traversal, link, decompression, and executable-install-script surfaces not needed for the foundation.
  Consequence: catalogs publish each exact file separately and every file is independently size/hash verified.
- Decision: the manager is a leading CLI command and does not alter TUI/runtime discovery.
  Evidence: this slice is an install/update/remove trust boundary; integration is separately scoped to LOC-089 and existing discovery behavior is stable.
  Consequence: installed bytes remain inert until a later explicit integration selects them.
- Decision: source allowlisting is exact user-pinned origin plus base path.
  Evidence: an open catalog URL or redirect policy would permit authority changes after trust was granted.
  Consequence: every catalog/artifact URL is derived from and contained by that exact source, with no redirects, proxy, or credentials.

## Surprises and Discoveries

- The initial characterization produced the intended red result: leading-command routing and redacted CLI Debug passed, while bare-profile parsing incorrectly accepted persistent runtime management. The parser guard now fails before runtime path discovery.
- A manifest needs multiple exact files and named entrypoints for later runtimes such as llama.cpp, but archives are unnecessary: streaming each signed file independently provides a smaller traversal/decompression boundary and makes the disk preview exact.
- The first disposable walkthrough command used the local OpenSSL `rand` arguments in the wrong order and stopped before launching its server or invoking `ca`; its partial root was moved to Trash. The corrected generator completed the full released lifecycle with four exact GETs.

## Recovery and Rollback

- Failed staging is deleted only inside the injected exact `external-runtimes/staging/<uuid>` path.
- Registry writes use owner-only create-new temporary files and atomic rename; update current pointers only after all files are verified and promoted.
- Confirmed removal first renames one validated runtime root inside the manager's own tree so an unsuccessful state update can restore it without Git or broad filesystem operations.

## Completion Record

Completed on 2026-08-12. `src/external_runtime.rs` adds the safe `ManagedRuntime` summary, exact-source/Ed25519 catalog store, closed direct-file manifests, streaming verification, generation cleanup, retained-version receipts, reverified entrypoint/rollback, and transactional confirmed removal. Leading `ca runtime` routing and profile paths were added without changing ordinary prompts or existing discovery. Ten focused tests, the parser and released-CLI filters, 1065 unit tests plus 34 CLI tests, formatting, strict Clippy, release build, help/version/profile smokes, and a released signed-loopback lifecycle passed. The successful fixture moved to `/Users/aji/.Trash/ca-ext088-walk-20260812`; no real runtime state, external service, config, Keychain, session, oracle, or installed binary was touched.
