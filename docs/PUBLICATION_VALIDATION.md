# Publication validation

## 2026-10-23 provider, verifier and native FreeBSD update

Version: `0.1.41`. Final native verification inputs have SHA-256
`c4ca6055a1f8967fef89a0c676fb19dbd5172143d89bf16fb6ead840f492d155` (1,169 files).
The combined macOS full-gate inventory is
`69d36efaca22361e14d0281afe7b4f47e483a79dd93c51fc11917e289c01a3d1` (1,168 files).
The differences are the existing public dependency-notice generator and a
FreeBSD-only `cfg(test)` MCP shell fixture adjustment. The release runtime
prefix is byte-identical; each artifact retains its distinct actual source
identity. The helper adjustment changes no macOS behavior. Inventory digests
identify tested inputs, not Git commits; build identities still include source.

The publication retains 1,163 of those inventoried paths. Six ignored legacy
Markdown task/plan records under `crates/codex-team` remain excluded. They are
not compiled or selected by the build-identity input roots; all compiled and
build-identity inputs match the tested snapshot. The 1,169-path digest above
identifies the private native verification inventory, not a fresh Git checkout.

The update accepts Responses `response.metadata`, avoids interpreting literal
`$HOME` as a skill, and repairs provider-filter masking of schema URIs that
caused DeepSeek HTTP 400. Native FreeBSD required execution and automatic
verification use the shared authenticated jail service described in
[FreeBSD setup](FREEBSD.md).

### ca checks and limits

| Check | Recorded result |
|---|---|
| Provider / skill regressions | 40 / 29 passed |
| Accepted provider-filter repair | 62 focused tests passed; minimal live DeepSeek prompt succeeded before and after installation |
| Final native MCP module / filter regressions / format | Passed: 22 / 62 tests and formatting on final native source |
| Final native `./scripts/verify.sh`, gate v8 | Passed all eight stages/evaluator: default 4,297 passed / 0 failed / 43 ignored across 29 targets; all features 4,451 / 0 / 44 across 30; zero filtered |
| Combined macOS `./scripts/verify.sh` | Passed all eight stages and evaluator; default 4,328 passed / 0 failed / 43 ignored across 29 targets, all features 4,484 / 0 / 44 across 30; zero filtered |
| macOS native terminal fixtures | All nine original cases passed in the final combined gate |
| Final FreeBSD installation candidate and native fixtures | Passed: exact metadata/`$HOME`, automatic verification of plain/internal-document-symlink source, and native jail workflows |
| Verifier intentional-defect self-test | Passed on preceding inventory `8b2f3987`; current verifier scripts retain the same defect-detection/restoration behavior |

The preceding native v7 gate, inventory
`fbfd73edaeee9b2514c9191b0b94c5e95d348ca027a1f136f044229abd8d647a`, did not pass.
Two MCP process-environment fixtures encountered `sysctl` `ENOMEM` before their
cleanup assertions; a subsequent controlled interruption left the report
`infrastructure_failed`. Those failures were not reproduced in the exact cases, ordered
module or bounded child/query diagnostic. The accepted final delta uses
FreeBSD's base `/bin/sh` for seven POSIX test fixtures; other platforms retain
`/bin/bash`. Runtime code, assertions and provenance checks are byte-identical.
This is native fixture portability, not a proven causal fix for Bash or kernel
`ENOMEM`. The final unchanged native gate passes on the combined source, which
remains identical before and after the run. Release build and native runtime
stages also pass (603.159 and 2.373 seconds). The gate candidate SHA-256 is
`d27a6f183f061cda1925f4e1a6d9fc00a1cf75858befd087fc15b0a23cf2c726`;
the separately copied/stripped installation candidate passed its three original
native workflows. Its SHA-256 is
`60563263dde655bcb16fa949189c9c09eda425b43dc324a56a19cce3de0aa074`
(99,663,752 bytes), frozen read-only before installation. The report SHA-256 is
`310bdb5f0a4650865ed989bec794bc07cbac090155f9db326cec096bbfd25083` and
the evaluation SHA-256 is
`6de1e61d2e431b9aa255246aae8efff8a916f93f74b33223802a33248be3fea6`.

A preceding signed macOS candidate
(`3fec59c0d81b00bf170af61f030eddf826d35f327928dd5b0d7d2a66f427e837`)
passed 17 isolation tests, release build, metadata/`$HOME` tool-loop checks and
plain/internal-symlink automatic verification. Its initial concurrent terminal
attempt sent no provider request before the original 15-second deadline; the
unchanged binary passed all nine original terminal checks on its first serial
retry. The cause remains unestablished. That failed attempt is retained, and the
candidate is superseded for installation by the combined filter repair. Earlier
failed, interrupted or incompatible-baseline runs remain separate evidence;
they are not passing final gates.

The final signed macOS `ca 0.1.41` was installed exactly once. Candidate, staged
and installed SHA-256 match:
`71fb5770bec9101ffbae59d4a7e941f797723f38d5a03ecfdd0d0c6a1df2fadc`.
Executable mode `0755`, ownership, code signature, version and help readbacks
pass; the preceding executable is retained as a timestamped SHA-labelled
recovery copy. The complete gate kept its source inventory unchanged. Its report
SHA-256 is `3b7be30e9fdaab45fc701b6672005c1e2ef7ea46fde0a609ccece48d5383f483`
and evaluation SHA-256 is
`82f8266f00a4f823641b159c108f1da78abb222579eb2fb51b290ef9944cf3d0`.

The installed normal PATH entry point also completed a live DeepSeek prompt,
returning `OK.` with URL hiding, Latin page rewriting and image filtering
preserved. The check used no saved session. It proves that request, not broad
provider qualification. Loopback fixtures do not prove live Anthropic, Gemini,
Vertex ADC or other provider behavior.

### Codex FreeBSD: installed and published

The [FreeBSD Codex fork](https://github.com/ajzrva-sys/codex) incorporates upstream
`8531184b49bcb160175a84e140b659a83a426df7`. Compiled commit
`a047c9ed59d2ac4bb0a281557cabbeb03e4ed1f6` is bound to publication commit
`d8df4524bac5123ba470d787b35aacca538efcb2`; only two Python fixture files differ.

The immutable release family passed ten native workflows, 47 jail checks and
174 code-mode unit tests. The wider scoped core run recorded 1,263 passed,
17 failed and 11 ignored; no clean complete Codex-suite claim is made.
Unsupported explicit full-root/glob policies remain documented limitations.
Unavailable Bazel maintenance checks did not pass.

The installed `/usr/local/bin/codex` resolves atomically to the root-owned
immutable family. Its CLI SHA-256 is
`2e56101de2c8926f1f0bc46d65b7fb53a6f7ee0d632975f553ea0f5c0b466714`;
version output is `codex-cli 0.0.0`, identified by the compiled commit above.
Hash, ownership/mode, version/help and ordinary-user PATH readbacks pass. Direct
and Sol-default native jail workflows also pass through the installed entry
point. The initial derived-fixture `ModuleNotFoundError` is retained; only its
original `PYTHONPATH` was restored before retry, with the binary and assertions
unchanged. The previous executable has a timestamped SHA-labelled recovery copy;
the privileged sandbox service was not replaced by this CLI installation.

The installed native entry point also completed a live provider/tool check
with its normal sandbox defaults and a supported model: the real shell ran
`/usr/bin/printf FREEBSD_OK`, exited 0, and the final response was `FREEBSD_OK`.
State remained isolated; existing account authentication was reused. This proves
that bounded request/tool loop, not general coding performance. An earlier
attempt using a model unsupported by this ChatGPT account returned service
HTTP 400; its failed receipt is retained separately. The successful receipt
SHA-256 is `55c7808659775e2975ac571de78e49a4ee66e55f682c78a3e5097938f35d7bb7`.

GitHub `main` was pushed and independently read back at
[`d8df4524`](https://github.com/ajzrva-sys/codex/commit/d8df4524bac5123ba470d787b35aacca538efcb2).

### ca delivery

| Artifact | Status |
|---|---|
| ca on FreeBSD | Complete: final stripped candidate qualified; one recoverable replacement of ca `0.1.41`, same SHA-256, `0755`, ownership and ordinary-user PATH version/help readbacks passed |
| ca on macOS | Complete: one signed replacement; gate, recovery/hash/signature/version/help and installed live DeepSeek checks passed |
| ca source snapshot | Identified by the validated input digest above; repository history identifies the associated Git commit |

The installed native ca entry point also completed a real provider/tool loop
with required jail isolation: `run_command` executed `/usr/bin/printf FREEBSD_OK`
and exited 0, followed by final response `FREEBSD_OK`. Its binary hash remains
`60563263dde655bcb16fa949189c9c09eda425b43dc324a56a19cce3de0aa074`.
State was isolated; normal existing account authentication was used. An initial
attempt using a symlinked authentication cache failed setup with `TooManyLinks`;
that receipt is retained separately. The working check changed no production
configuration or source. It proves this bounded native provider/tool path,
not coding performance.

Development and verification use disposable application state. Production
sessions/configuration and running user processes are left alone. Installation
and live-provider proof remain distinct from source tests.

### Benchmark scope

The coding benchmark was canceled before any scored native attempts ran. No
ca-versus-Codex correctness, speed or cost result is claimed. Native one-second
and sixty-second usage-export checks are accounting/flush fixture proof only.

## Historical publication evidence

The following October 6 record describes the initial public import. Its version,
counts, hashes and installation observations are historical, not current release
results.

### Initial public source publication — 2026-10-06

#### Delivery

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

This paragraph records the October 6 publication state. On October 17, 2026,
the repository owner selected MIT OR Apache-2.0 for CA-owned material. The
subsequent licensing change adds the license texts, package declarations,
contribution terms and dependency notices without rewriting the initial import
or claiming additional rights over third-party components. See
[licensing scope and provenance](LICENSING.md) and
[the licensing change record](exec-plans/completed/LIC-2026-10-17-dual-licensing.md).

#### Fresh validation

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

#### Executable identity and installation

Signed built and installed `ca` SHA-256:

```text
f20a97ab23bdec8e01c5f656bacd872cec5488181e73afc78c4d545831971fa0
```

The fresh release build is byte-identical to `/Users/aji/.local/bin/ca`.
The installed signature verifies and `--version` succeeds, so no replacement
is necessary. Existing processes and runtime configuration were left intact.
The screenshot receipt separately binds each image to that same executable.

#### Evidence boundaries

Local build/lint/test logs are retained in `scratch/publication-20261006/` and
are intentionally not pushed. This publication checks compilation, applicable
workspace tests, local executable identity, and the captured UI paths. It does
not establish a fresh live-provider check, a new platform build, an eight-hour
soak, or selected real-model retrieval validation. Those remain separately
tracked in the historical plans.

The full rerun exited 0, with no filtered workspace cases or baseline exclusions.
Subprocess writer-probe summaries are excluded from the aggregate. The completed
log SHA-256 is `1676e98870904e3dda3d48f11413664cb6c9d45e1ceea896f80644d365d01b26`.
