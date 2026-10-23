# FreeBSD

ca builds natively on FreeBSD. Required command isolation and automatic
verification use the authenticated jail service from the
[FreeBSD Codex fork](https://github.com/ajzrva-sys/codex), through its shared
client helper. ca and its provider connection run as the ordinary user outside the jail;
each isolated command runs as that user inside its own jail.

## Build

Install Git, Python, a C toolchain and the Rust version in
`rust-toolchain.toml`, including rustfmt and Clippy. FreeBSD packages install Git
under `/usr/local/bin`; ca uses that platform path for its restricted Git calls.

```sh
cargo build --release --workspace --all-features --locked
./target/release/ca --version
./target/release/ca --help
```

FreeBSD executables do not need macOS code signing. Preserve the old executable
before replacing an installed copy, and check the replacement through the normal
PATH entry point.

## Required jail service

An administrator must install the daemon separately. In a compatible checkout of
the FreeBSD Codex fork, build it as an ordinary user:

```sh
cd codex-rs
cargo build --release --locked -p codex-freebsd-sandbox --bin codex-freebsd-sandboxd
cargo build --release --locked -p codex-freebsd-sandbox-client --bin codex-freebsd-sandbox-client
```

Build the client helper from the same checkout. Then run the fork's installer
as root, supplying the account that runs ca and the absolute paths to the built
daemon and client. Follow the installer's help for these options; both helpers
belong under `/usr/local/libexec` and must be protected by root ownership.

```sh
python3 scripts/freebsd/install_sandbox.py --user ACCOUNT --daemon /absolute/path/to/codex-freebsd-sandboxd --client /absolute/path/to/codex-freebsd-sandbox-client
```

Run the installer from the fork's repository root. It installs a root-owned
daemon and account allowlist, enables its rc service, and loads nullfs, tmpfs and
fdescfs. Read the fork's FreeBSD guide for service operation and rollback. ca does
not change the user's Codex configuration or install a privileged service when
it starts.

With the service available, `--sandbox required` selects native jail isolation.
The policy grants writes only to the selected workspace and temporary roots,
reads to runtime tools and explicit dependency roots, and preserves explicit
denials. Networking follows the caller's policy. Service or capability failures
block required execution.

Ordinary commands read concrete installed runtime paths and granted workspaces.
The native jail rejects explicit full-host-root views. Session write grants are
honored until revoked; granting an export directory does not grant unrelated
directories.

FreeBSD defaults `agent.keep_awake` to `false` because there is no native
idle-inhibition backend. Explicitly enabling it reports that limitation. Other
agent defaults, including delegation and completion review, are unchanged.

Automatic verification creates a private source projection, denies the original
repository and network, and uses a scrubbed environment. Safe tracked internal
relative symlinks retain their targets in that projection. Explicit Python
virtual environments supply their interpreter and dependencies as read-only
runtime roots. Verification still rejects unsafe source aliases and unavailable
dependencies.

## Validation

Use `./scripts/verify.sh` on FreeBSD for the full host-platform gate. Its report
and independent evaluation record the actual checks and failures. Native jail
proof additionally requires the running service; a compilation check does not
prove isolation or credentialed provider behavior. See
[publication validation](PUBLICATION_VALIDATION.md) for recorded release results.
