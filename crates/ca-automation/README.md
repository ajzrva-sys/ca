# ca automation engine

`ca` owns admission, occurrence identity, permissions, budgets, supervision,
notifications and session delivery. This crate executes one admitted run through
`native::AutomationEngine` and durable `runtime::RunStore` services. It has no
dependency on ca provider/session internals or the legacy compatibility crates.

`ca-automation-protocol` owns runner-free contracts. Legacy V1 schema identifiers,
serialization defaults, digest inputs, and storage paths remain stable.
`codex-team` and `codex-team-protocol` reexport these implementations; their
binaries forward to the engine's compatibility CLI. The stable supervisor remains
a health/rollback boundary, while ca admits evaluation and experiment work.

Typed workflow actions journal intermediate results under the run identity.
Confirmed actions replay their saved results. Unconfirmed effects require
reconciliation before execution. Mutating workflow runs share a conservative
machine-wide resource lease, including separately named profile manifests.

The legacy role hierarchy, repository broker, typed remote adapters and CLI/MCP
interfaces remain available for compatible clients. They use the same native
implementation instead of a second embedded engine.

Native mechanistic evidence uses one sandbox at each physical verification
command, including Cargo metadata, Rust API compilation and toolchain provenance.
The ca-owned coordinator retains bounded output, cancellation, journals and
resource ownership; it projects tracked source into private run-scoped scratch.
Each command receives scrubbed credentials, private build/cache paths and required
filesystem/network isolation. macOS forbids applying a second Seatbelt sandbox
inside an already sandboxed process, so only complete typed evidence requests use
this coordinator boundary. Queries, observations, mixed requests and all other
targets retain ca's outer worker sandbox. Native evidence refuses the legacy
unisolated advisory option. Backend self-tests and real sandbox canary tests
remain required; unavailable isolation produces Needs attention.
