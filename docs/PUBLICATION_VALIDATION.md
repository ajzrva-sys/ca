# Initial public source publication — 2026-10-06

## Delivery

Destination: [ajzrva-sys/ca](https://github.com/ajzrva-sys/ca), public, branch `main`.
The starting application snapshot had no Git metadata. This publication creates
an initial source history; it does not import history from the oracle archives.

Added a README, getting-started/configuration/user/automation/development/
troubleshooting guides, generated CLI reference, and five actual TUI captures.
`tools/tui_screenshots.py` reproduces the captures from VT output of the release
binary using disposable runtime storage and a loopback fixture provider.

The import includes application/workspace source, manifests and Cargo.lock,
fixtures, prompts, policies, tools, and existing engineering records. Build
output, scratch receipts, local runtime state, credentials, and oracle/reference
archives are excluded. Historical records retain dated paths and references to
omitted files; current user guides distinguish that history from portable setup.
No project-wide license is invented by this publication.

## Fresh validation

| Check | Result |
|---|---|
| `cargo build --release --workspace --all-features --locked` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed |
| `cargo test --workspace --all-features --locked --no-fail-fast -- --test-threads=1` | Passed; 3,958 passed, zero failed, 44 ignored |
| Signed release `--version` / `--help` | Passed; `ca 0.1.0` |
| Screenshot tool / visual inspection | Five PTY captures; inspected PNG layout |
| Portable guide relative links | Checked |
| Compiled-source publication inventory | 882 regular build-input files checked; none omitted |
| Credential-pattern and large-file scan | No detected credential files or oversize blobs; one synthetic test-token pattern verified in `src/codex_learning.rs` |

The first restricted-runner test attempt failed: 2,823 passed, 173 failed,
42 ignored in the main ca test target. Loopback socket fixtures reported
`Operation not permitted`; subprocess/platform fixtures also require access the
restricted runner did not provide. The subsequent complete one-thread run uses
an environment permitting those fixtures. The initial failure log is retained,
not counted as a passing run.

Initial-import whitespace checks identify existing Markdown hard-break spaces
and historical source whitespace. These are preserved rather than reformatted;
Terminal snapshot text also retains full-width space padding. Rust formatting
and Clippy independently pass.

## Executable identity and installation

Signed built and installed `ca` SHA-256:

```text
f20a97ab23bdec8e01c5f656bacd872cec5488181e73afc78c4d545831971fa0
```

The fresh release build is byte-identical to `/Users/aji/.local/bin/ca`.
The installed signature verifies and `--version` succeeds, so no replacement
is necessary. Existing processes and runtime configuration were left intact.
The screenshot receipt separately binds each image to that same executable.

## Evidence boundaries

Local build/lint/test logs are retained in `scratch/publication-20261006/` and
are intentionally not pushed. This publication checks compilation, applicable
workspace tests, local executable identity, and the captured UI paths. It does
not establish a fresh live-provider check, a new platform build, an eight-hour
soak, or selected real-model retrieval validation. Those remain separately
tracked in the historical plans.

The full rerun exited 0, with no filtered workspace cases or baseline exclusions.
Subprocess writer-probe summaries are excluded from the aggregate. The completed
log SHA-256 is `1676e98870904e3dda3d48f11413664cb6c9d45e1ceea896f80644d365d01b26`.
