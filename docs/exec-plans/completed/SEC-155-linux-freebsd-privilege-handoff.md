# SEC-155 — Linux and FreeBSD Privilege Handoff

Status: COMPLETE
Owner/session: current Codex session
Last updated: 2026-08-23

## Goal

Extend SEC-154's exact, host-owned `run_privileged` flow to the main
interactive Linux and FreeBSD TUI without weakening its approval, credential,
process, output-redaction, or sandbox boundaries.

## Non-Goals

- No new Linux namespace/seccomp or FreeBSD Capsicum sandbox backend.
- No `su`, `doas`, `pkexec`, PATH search, shell command, PTY, askpass, retry,
  paste, unattended, service, scheduled, Plan-mode, or subagent elevation.
- No password-based sudo test and no change to generic job APIs. One later,
  explicitly authorized `sudo -n` production-helper probe may validate
  identity/policy without prompting or running a mutating target.

## Evidence and Current Behavior

- `src/privilege.rs` hard-codes macOS availability and `/usr/bin/sudo`, and
  validates only Mach-O targets.
- Linux conventionally installs sudo at `/usr/bin/sudo`; packaged FreeBSD sudo
  uses `/usr/local/bin/sudo`. Each path can retain the existing canonical
  root/setuid/non-writable ancestry and identity checks.
- `src/sandbox.rs::SandboxBackend` provides mandatory wrapping only on macOS.
  On other hosts `required` returns `SandboxError::Unavailable`, while
  `policy-only` and `off` preserve the existing direct argv path.
- SEC-154's fake-sudo, detached process, owner/nonce, zeroizing secret, and
  byte-redaction implementation is already Unix-portable.

## Scope

Allowed files: SEC-155 task/plan/harness records and the privilege registry,
preparation, TUI copy, tests, and snapshots needed for portability.

Forbidden files/areas: `oracles/`, `reference/`, runtime configuration,
credentials, saved sessions, installed binaries, and unrelated dirty changes.

Runtime state affected: process-local test state and disposable fake-helper
processes only.

## Design and Invariants

- Supported hosts are exactly macOS, Linux, and FreeBSD.
- The helper path is compile-time fixed: `/usr/bin/sudo` on macOS/Linux and
  `/usr/local/bin/sudo` on FreeBSD. Missing or insecure helpers hide the tool.
- Targets and readable helpers must have the host-native binary format: Mach-O
  on macOS and ELF on Linux/FreeBSD.
- All SEC-154 request bounds, dispatcher rejection, digest, exact approval,
  private owner/nonce, authentication, redaction, timeout, and hook invariants
  remain unchanged.
- Linux/FreeBSD `required` sandbox requests fail during preparation, before
  approval, hooks, cached-auth checks, or masked input. Users may explicitly
  select the already-visible `policy-only` or `off` mode; elevation never
  silently changes it.

## Milestones

### M1 — Characterization and platform contract

- Add failing coverage for supported OS/path/native-format selection and the
  pre-approval mandatory-sandbox refusal.

### M2 — Portable preparation and exposure

- Generalize helper selection, capability probing, native-format checks,
  errors, tool copy, and platform tests.

### M3 — Validation and records

- Run focused privilege/registry/TUI tests, local completion gates, and native
  Linux/FreeBSD checks if an existing disposable host is reachable without
  credentials or user-state mutation.
- Reconcile architecture, continuity, testing, parity, index, and task records.

## Acceptance Criteria

- A secure fixed sudo plus root-secured native target exposes and prepares the
  same exact handoff on macOS, Linux, and FreeBSD.
- Missing/insecure/wrong-format helpers and targets fail closed.
- Linux/FreeBSD mandatory sandbox mode fails before approval/authentication;
  policy-only/off retain exact argv, closed stdin, denied network request
  metadata, private process groups, and streaming secret redaction.
- Every unattended/delegated surface remains closed and no real sudo runs.

## Validation

Focused: privilege, interaction, registry, main policy/order, unsupported
surfaces, sandbox, and TUI render tests.

Completion: `TESTING.md` formatter, default/all-feature workspace tests, strict
Clippy, release build, smokes, diff/snapshot hygiene, and protected-tree checks.

Manual/visual: deterministic TestBackend snapshots only unless an existing
disposable native host is safely available.

Blocked checks: real sudo/password authentication and any unavailable physical
Linux/FreeBSD host.

## Progress

- [x] Active ExecPlan and task record created.
- [x] Failing portability characterization recorded.
- [x] Platform helper/native-format preparation complete.
- [x] Surface and TUI coverage complete.
- [x] Completion validation and harness reconciliation complete.

## Decisions

- Decision: use one fixed sudo path per supported OS.
  Evidence: the existing SEC-154 helper-identity invariant and platform package
  layout.
  Consequence: no PATH lookup or alternate elevation helper can broaden trust.
- Decision: fail Linux/FreeBSD `required` mode before consent.
  Evidence: the repository has no mandatory sandbox backend on those hosts.
  Consequence: support is explicit through existing policy-only/off modes, not
  a silent sandbox downgrade.

## Surprises and Discoveries

- The repository's mandatory sandbox implementation is macOS-only, so Linux
  and FreeBSD support must require an explicit `policy-only` or `off` selection
  rather than silently weakening `required`.
- The available FreeBSD 15.1 amd64 host has a native ELF `/usr/bin/id` but no
  `/usr/local/bin/sudo` was initially absent. After the user made it available,
  a bounded probe verified its root-owned mode-4755 ELF identity and secure
  ancestry.
- A locked FreeBSD workspace compile reached a pre-existing
  `crates/codex-team` `setpriority` i32/u32 mismatch. Per user direction this
  unrelated portability issue was not pursued. Both exact disposable source
  directories were removed; the first build populated missing user Cargo cache
  entries before interruption.

## Recovery and Rollback

Keep changes in SEC-155 hunks. Revert those hunks without destructive Git
operations; no storage migration or runtime state is created.

## Completion Record

Changed files: `src/privilege.rs`, `src/agent/registry.rs`, and SEC-155 task,
architecture, continuity, testing, parity, index, and plan records. SEC-154's
existing TUI/execution files and snapshots required no behavioral change.

Commands and results: the initial characterization failed to compile as
expected; formatter, 9 privilege tests, closed registry-schema coverage, exact
approval/order coverage, and the complete default workspace test suite pass.
The explicitly authorized FreeBSD `sudo -n -v` probe verified the fixed helper
but host policy denied user `aji`, so no target ran. Completion lint/release and
hygiene results are recorded in `TESTING.md`.

Remaining risks: no password path or successful privileged target was
exercised; the FreeBSD account is denied by sudo policy. The unrelated
`codex-team` native compile error remains outside this task. Linux and FreeBSD
have no mandatory sandbox backend, so `required` intentionally fails closed.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `PARITY.md`,
`docs/.INDEX.md`, and `NEXT_STEPS.md` now describe the supported hosts and
non-macOS sandbox boundary without claiming Cursor parity.
