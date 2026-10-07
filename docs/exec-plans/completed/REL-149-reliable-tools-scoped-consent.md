# REL-149 — Reliable Tool Execution and Scoped Consent

Status: COMPLETE (2026-08-22)

## Purpose

Repair the confirmed tool-execution failures and approval friction without
weakening write, process, network, external-communication, credential, or
sandbox boundaries. The screenshots are evidence only; repository source,
tests, and command output remain authoritative.

## Constraints

- Preserve the heavily modified worktree and edit only REL-149 seams.
- Do not modify `oracles/` or `reference/`, install a binary, change runtime
  configuration, inspect credentials, or create provider traffic.
- Keep Cargo, Git, builds, tests, and source writes approval-gated.
- Valid skill activation remains approval-gated; absent skills and optional LSP
  configuration must not create impossible/noisy calls.
- Required sandbox mode remains fail closed; no login-shell or policy-only
  fallback is introduced.

## Milestones

- [x] Characterize and replace the bounded file-scoped unified-patch engine.
- [x] Derive skill/LSP tool schemas and post-edit diagnostics from runtime availability.
- [x] Repair macOS `/etc`, trusted developer-tool, Cargo-cache, and inferred-network execution.
- [x] Reclassify queued approvals after scoped authority changes and harden process-capable reads.
- [x] Reconcile deterministic UI coverage, complete validation, and harness documentation.

## Acceptance

- Empty skill registries advertise no impossible activation/view calls.
- Contextual and multi-hunk patches preserve context, deletion checks, newline
  style, EOF-newline state, undo receipts, and unchanged files on failure.
- No-LSP and extension-mismatch mutations append no status noise; configured
  reports and failures remain bounded and visible.
- A scoped current-run/path/tool/Yolo choice affects matching later queued calls;
  one-shot allow/deny remains one-shot and explicit/network/email boundaries win.
- Required-sandbox Git can traverse `/etc`, trusted Homebrew/Rustup tools resolve,
  cached Cargo validation works offline without credential/config access, and
  untrusted PATH shadowing remains gated.
- Process-capable `read_lints` is managed and approval-requiring; inferred SSH
  network intent survives distinct approval through job launch.
- Focused and complete gates in `TESTING.md` pass with actual results recorded.

## Decisions

- Use exact prepared invocation identity for current-run grants; exact path
  grants use escaped canonical paths while persistent tool rules keep their
  existing user-selected breadth.
- Reclassify unresolved prepared calls immediately before activation so saved
  rules and current-run authority affect the same ordered provider batch.
- Use a shared canonical trusted-executable resolver for AutoReview and sandbox
  read roots; arbitrary PATH entries never gain silent trust.
- Cargo registry cache access is read-only, command-scoped, and excludes config,
  credentials, and Git dependency caches. Non-network Cargo jobs run offline.
- No automatic skill, language-server, toolchain, or validation installation/run
  is added.

## Recovery

Use targeted inverse patches for REL-149 hunks only. Never reset, checkout, or
rewrite unrelated dirty-tree changes. Every failed mutation must leave the
target unchanged and produce no undo receipt.

## Progress and Discoveries

- 2026-08-22: exact macOS reproductions confirmed `/etc` symlink traversal and
  hidden `/opt/homebrew` as the Git/Cargo environment causes.
- 2026-08-22: source audit confirmed the patch parser drops/ignores context,
  pending approvals are preclassified once, optional LSP absence is appended to
  every mutation, `read_lints` runs Cargo outside the managed job boundary, and
  inferred SSH network intent is re-read incorrectly during execution.
- 2026-08-22: runtime-derived subagent skill schemas initially exposed a
  same-mutex temporary-lifetime deadlock; one snapshot read now supplies both
  selector lists and the complete subagent suite covers the boundary.
- 2026-08-22: exact-path consent must escape glob metacharacters; both current-
  run and persistent exact-path construction now do so.

## Completion Record

Implemented the 1 MiB-bounded unified patch engine, runtime-derived skill/LSP
tool availability, typed quiet post-mutation diagnostics, live ordered approval
reclassification, exact prepared invocation grants, narrow macOS `/etc` and
toolchain/Cargo sandbox reads, authoritative prepared network intent, and
managed approval-requiring `read_lints`.

Validation:

- Focused unified patch, mutation, loop guard, permission, approval/render,
  sandbox/jobs, skills, LSP, service/headless-adjacent, subagent, managed lint,
  and inferred-network suites: PASS.
- `cargo test --no-fail-fast`: PASS, 1,671 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast --quiet`: PASS,
  1,700 main, 118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D
  warnings`, `cargo build --release --all-features`, release help/version,
  `git diff --check`, snapshot hygiene, and oracle/reference immutability: PASS.

No provider traffic, credential inspection, runtime config/session mutation,
binary installation, or oracle/reference access or modification occurred.
