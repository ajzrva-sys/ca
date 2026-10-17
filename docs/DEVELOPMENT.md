# Development

## Repository map

| Path | Responsibility |
|---|---|
| `src/main.rs` | CLI launch, TUI/headless orchestration, stream/tool coordination |
| `src/app.rs` | Shared conversation and UI state |
| `src/agent/` | Provider adapters, tools, registry, subagents |
| `src/tui/` | Input, terminal lifecycle, rendering, transcript caching |
| `src/config.rs`, `src/runtime.rs` | Configuration, profiles, runtime paths/capabilities |
| `src/session.rs`, `src/session_access.rs` | Durable history and writer ownership |
| `src/approval.rs`, `src/permission.rs`, `src/sandbox.rs` | Execution authority boundaries |
| `src/recovery.rs` | Durable edit intent and recovery evidence |
| `src/automation*.rs` | Native automation admission, dispatch, UI and workers |
| `crates/ca-watch` | Local/SSH process observation |
| `crates/ca-automation*` | Native execution and protocol contracts |
| `crates/codex-team*` | Compatibility packages and their own tests/docs |
| `tests/`, `tools/` | Fixtures, scripted cases, terminal validation helpers |
| `build_support/` | Explicit source/build identity hashing |

Read [AGENTS.md](../AGENTS.md), [ARCHITECTURE.md](../ARCHITECTURE.md), and
[the knowledge index](.INDEX.md) before changing behavior. Nested package
instructions apply to their directories. Historical `.agent/PLANS.md` links
refer to a file absent from this initial source snapshot; existing ExecPlans
remain available under `docs/exec-plans/`.

## Validation

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo test --workspace --all-features --locked -- --test-threads=1
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --release --workspace --all-features --locked
```

The one-thread all-feature command reduces known timing contention; it does not
exclude tests. Some tests create loopback listeners, subprocesses, Git repositories,
and platform sandbox fixtures. Run them in an environment allowing those
operations. A sandbox denial is a failed validation attempt, not a passing test.
Network-backed `.ca` scripted cases need separate credentials/services.

Run focused regression tests for behavior changes first. TUI changes need render
coverage and actual terminal interaction checks; helpers such as
`tools/compaction_pty.py` and `tools/unavailable_tool_pty.py` exercise signed binaries
with disposable fixtures. Their dependencies and receipts are separate from
Cargo's unit suite. Long-duration soak checks are separate again.

## Change discipline

Keep behavior changes local. Preserve async responsiveness, exact tool catalog
and execution alignment, writer ownership, frozen retry authority, and approval
classification. Do not introduce synchronous long work on the UI event loop.
Record actual outcomes, failures, and unresolved gates in the applicable plan
and continuity records. Retain failed evidence when a rerun succeeds.

The oracle/reference archives used during historical development are not part
of this Git import. Existing parity records describe their evidence boundaries;
do not claim a fresh oracle comparison from their historical notes.

## Publication boundary

CA-owned material is licensed under MIT OR Apache-2.0. See
[licensing scope and unresolved provenance](LICENSING.md),
[contribution terms](../CONTRIBUTING.md), and the
[dependency notices](../THIRD_PARTY_NOTICES.md). Before distributing a binary,
include the applicable license texts and notices and the source-availability
information for MPL-covered dependencies. Verify each independently packaged
crate contains both project license texts; dependency metadata alone is not a
record of the native libraries and assets in a particular binary.

`.gitignore` excludes build output, scratch work, local state, oracle archives,
Finder files, and credential files. The initial import deliberately retains
historical engineering docs, including dated paths and references to omitted
receipts. The new guides provide the portable entry point. No generated binary
or GitHub release asset is included in the source push.

Build identity uses explicit application/workspace source and dependency inputs.
Runtime config, repository history, and build output are not identity inputs.
A new Git commit does not itself change the compiled source contract.
