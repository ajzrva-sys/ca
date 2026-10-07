# PLG-084 — Local Plugin Bundles

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Add explicit profile-scoped local plugin bundles that declaratively package existing skill roots, MCP server definitions, hooks, workflow files, and built-in theme aliases; present exact capabilities and collisions before activation; pin content identity; and never load native in-process code.

## Non-Goals

- Remote catalogs/downloads/updates, arbitrary marketplace URLs, native libraries, WASM, in-process callbacks, plugin-defined Rust/tools/slash commands, embedded scripts, automatic activation, hot activation during an active turn, secret values in manifests, or weakening existing skill/MCP/hook/workflow/theme authority.
- Cursor parity. This is an oracle-informed `ca` extension.

## Scope

Allowed: new `src/plugin.rs`; leading `ca plugin` CLI routing; narrow runtime config composition for normal profiles; profile data paths; focused CLI/core tests; required harness docs.

Forbidden: oracle/reference edits, user runtime state, installed binary, remote plugin traffic, native executable loading, arbitrary theme code, persistent secret values, unrelated TUI/session/provider changes.

## Design and Invariants

- `ca plugin add <local manifest>` reads one strict bounded non-symlink `plugin.toml`/JSON, canonicalizes all bundle-relative resources inside one canonical bundle root, hashes every declared file/tree deterministically, validates existing schemas, and stores only a disabled profile-scoped registry record. Prompts/commands/headers are redacted from Debug/list output.
- `list` and `show ID` expose safe metadata, requested process/network/filesystem/hook capabilities, exact contributions, and every collision. `enable ID --confirm ID` is required; activation is rejected on drift or collision. `disable` and exact-confirm `remove` never delete source bundles.
- Manifests have `deny_unknown_fields` and only these declarative contribution types: skill roots, named MCP configs, named hook configs, named workflow paths, and named aliases to an existing built-in theme. No native-code/path/entrypoint field exists.
- Enabled records are composed into a cloned runtime `CaConfig`: skill roots append, MCP servers merge by exact unique name, hooks append, and plugin workflow/theme catalogs remain safe metadata for explicit selection. Existing managers retain their current trust, network, approval, sandbox, timeout, and process cleanup boundaries. Bare ignores the registry and refuses mutations.
- Registry files/directories are owner-only, bounded, atomic, profile-scoped, symlink-rejecting, revisioned, and never rewrite source bundles. Runtime composition revalidates exact digest before starting extensions and fails closed on drift.

## Milestones

1. Strict manifest/registry/digest/collision/capability tests and leading CLI parsing.
2. Runtime composition through existing skill/MCP/hook boundaries plus explicit workflow/theme catalog resolution.
3. Full gates, released disposable-home/fake-executable walkthrough, documentation reconciliation, archive.

## Acceptance

- New bundles are inert; enable requires exact confirmation and no collision/drift.
- Enabled contributions remain declarative subsets of existing extension schemas and cannot introduce in-process code or implicit authority.
- Named profiles are isolated, bare is extension-disabled, source bundles remain untouched, and safe output contains no manifest commands, secret references' values, headers, or prompts.
- Existing configs and runs without enabled plugins are byte/behavior compatible.

## Validation

Focused plugin tests passed 8/8 and the released CLI integration passed. Repository gates passed 1013 unit tests plus 32 CLI tests, `cargo fmt --check`, strict Clippy, release build, and help/version/plugin smokes. A disposable-home released-binary walkthrough proved inert registration, safe preview, wrong-confirm refusal, exact enable, one skill-bearing loopback request, workflow/theme resolution, disable/remove, intact source, owner-only registry state, and no session; its inspected fixture was moved recoverably to `/Users/aji/.Trash/ca-plg084-walk-20260812`.

## Progress

- [x] SCH-083 archived and existing extension seams traced.
- [x] Strict manifest/registry and collision tests pass.
- [x] Runtime composition and CLI controls pass.
- [x] Full gates/walkthrough/docs pass; plan archived.

## Recovery

Disable the exact bundle or move the profile registry aside; source bundles and normal config remain unchanged. No automatic install/update, native module, or remote state exists.
