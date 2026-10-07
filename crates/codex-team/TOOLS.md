# TOOLS

## Package and Build System

- Language: Rust 2021.
- Package manager/build tool: Cargo.
- Manifest: `Cargo.toml`.
- Lockfile: `Cargo.lock`.

## Repo-Derived Commands

| Command | Source | Status | Notes |
|---|---|---|---|
| `cargo fmt -- --check` | Rust/Cargo convention | Verified during harness pass | Formatting gate; exited 0 with no output |
| `cargo test` | Cargo project and local validation | Verified during harness pass | Use as full-suite validation for source changes; 119 unit tests passed |
| `cargo test queue::next_steps` | Extracted module tests | Covered by full `cargo test` during harness pass; focused command verified before harness pass | Use for queue parser changes |
| `cargo test legacy_intern_support_contract` | `src/app_tests.rs` test module | Covered by full `cargo test` during harness pass; focused command verified before harness pass | Use for legacy Intern/support packet changes |
| `cargo build --release` | `README.md` Build section | Inferred, not run in harness pass | Required before release/install claims |
| `codex-team doctor --repo <repo>` | `README.md`, `src/cli.rs` | Inferred, not run in harness pass | Requires built/installed binary and Codex environment |
| `codex-team commands --repo <repo>` | `README.md`, `src/cli.rs` | Inferred, not run in harness pass | Writes `.orchestrator/manual-commands`; use intentionally |

## CLI Subcommands

Defined in `src/cli.rs`:

- `init`
- `step`
- `run`
- `status`
- `cost`
- `doctor`
- `commands`
- `watch`
- `observe`
- `learn`

## Generated and Runtime Paths

- `target/` - Cargo build output, ignored.
- `.orchestrator/` - runtime state, prompts, schemas, tasks, runs, recipes, evidence, events, and cost ledger when this repo is used as a target; ignored.
- `.build/codex-team-caches/` - repo-local Swift/Clang module caches used by role subprocesses.

## Commands Not to Run Without Approval

- `cargo install --path .` - installs/replaces a user binary.
- `codex-team run --auto-continue` against a real repo - can execute model-driven work and commits.
- `codex-team commands` against a real repo unless script generation is intended.
- Any destructive Git or filesystem command.
