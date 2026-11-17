# Getting started

## Prerequisites

Use the Rust toolchain pinned in `rust-toolchain.toml` (currently 1.97.1), Cargo,
and a native linker. Rustup can select the pinned toolchain and its rustfmt and
Clippy components. Dependencies include bundled SQLite and native libraries;
install the normal compiler/SDK tools for your platform. The macOS build also
includes Security.framework and audio support. Git is required for repository
and worktree features. Provider-backed conversations require a configured route.

The publication build was checked on macOS. FreeBSD-specific code and historical
checks exist, but this publication does not establish a fresh FreeBSD or Linux
build result.

## Build

From the repository root:

```sh
cargo build --release --workspace --all-features --locked
```

Cargo writes release executables into `target/release/`. The default application
is `ca`; native automation and compatibility workspace executables are built
alongside it. `--all-features` also builds the reasoning-artifact experiment.
Use `--locked` to retain the checked-in dependency resolution. Add `--offline`
only after the necessary dependencies have been downloaded.

On macOS:

```sh
codesign --force --sign - target/release/ca
codesign --verify --strict --verbose=2 target/release/ca
./target/release/ca --version
./target/release/ca --help
```

## Install

Put `ca` in a directory on your PATH. Before replacing an existing executable,
keep a recovery copy. The following example assumes no existing destination;
when upgrading, back it up first:

```sh
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/ca "$HOME/.local/bin/ca"
# macOS only:
codesign --force --sign - "$HOME/.local/bin/ca"
```

Ensure `$HOME/.local/bin` is in PATH and verify `command -v ca` and `ca --version`.
Native automation workers may require companion workspace binaries; see the
[runtime guide](CA-automation-runtime.md) before enabling them. Restart an existing
ca process to use the new binary; replacing a file does not update a running process.

## Configure and launch

Follow [Configuration](CONFIGURATION.md) to create a provider/model route and
supply credentials externally. The default config location is
`~/.config/ca/config.toml`. Avoid changing an existing configuration wholesale.

```sh
ca --doctor
ca --list-models
cd /path/to/your/project
ca
```

`--doctor` checks local readiness. `--doctor --connect` additionally makes isolated
provider requests and may incur provider usage. Starting the UI does not prove a
provider connection is usable; send a small prompt after checking the route.

For an initial read-only task:

```sh
ca --mode ask
# Or one headless task:
ca --prompt "Explain the main entry points" --auto-review
```

`--auto-review` permits CA-local evaluation of read-only tools in headless mode.
Mutations still need the applicable authorization. See the [User guide](USER_GUIDE.md)
for session persistence, workspace selection, and structured output.
