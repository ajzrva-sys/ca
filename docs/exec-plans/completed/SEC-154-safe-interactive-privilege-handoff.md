# SEC-154 — Safe Interactive macOS Privilege Handoff

Status: COMPLETE (2026-08-23)
Owner/session: current Codex session
Last updated: 2026-08-23

## Goal

Provide a one-use, exact-command `run_privileged` flow in the main macOS TUI without exposing a sudo password to the model, composer, generic job controls, hooks, persistence, traces, or raw output capture.

## Non-Goals

- No Linux, FreeBSD, Windows, service, headless, scheduled, Plan-mode, or subagent privilege execution.
- No shell strings, PTYs, askpass, password paste/retry, background execution, network grant, environment override, or arbitrary stdin.
- No persistent or Yolo authorization for elevation and no weakening of the selected sandbox.

## Evidence and Current Behavior

- `src/jobs.rs` gives ordinary jobs piped stdin and model-visible bounded stdout/stderr, but generic job IDs remain writable through `write_shell_stdin`.
- `src/interaction.rs` has a host-only masked `SecretHandle`, but current masked outcomes have no privilege consumer.
- `src/main.rs` stores ordinary composer submissions in process history and disables submission during an active turn.
- `src/service.rs` rejects masked interactions; headless and scheduled runs have no host secret callback.
- `src/sandbox.rs::prepare_process` preserves argv boundaries and can wrap a direct target in the existing macOS sandbox.
- The OpenCrabs oracle shows only the broad masked-callback pattern. It is read-only optional evidence, not a source or a Cursor parity requirement.

## Scope

Allowed files: active/completed ExecPlan and harness docs; Rust source, tests, snapshots, and direct dependency metadata needed for the privilege protocol.

Forbidden files/areas: `oracles/`, `reference/`, runtime configuration, saved user sessions, credentials, and installed binaries.

Runtime state affected: process-local interactive state and short-lived fake-sudo test processes only. Validation must never invoke real sudo.

## Design and Invariants

- `run_privileged` accepts only a bounded absolute root-secured native program, ordered argv, and optional prepared workspace cwd.
- A versioned digest and nonce bind canonical program/metadata, argv, cwd, sandbox policy, and helper identity to one exact approval.
- Explicit deny rules remain authoritative. Allows, session grants, AutoReview, Yolo, and `run_command` permissions never authorize privilege.
- Exact approval, cached-auth check, optional masked password validation, and target execution are separate host-owned stages.
- Password validation uses detached `sudo -S -k -p '' -v`; the target uses detached `sudo -n -- <sandbox-prepared argv>` with stdin closed.
- Privilege processes are private and capability-bound; generic job APIs cannot list, await, write, or kill them.
- Output is byte-redacted across chunk boundaries before buffering or broadcasting. Secret buffers are zeroized on every terminal path.

## Milestones

### M1 — Characterization and protocol types

- Add failing tests for tool availability, bounded argv validation, exact approval behavior, masked-input limits, and non-interactive rejection.
- Add the typed request/preparation/digest state and trusted macOS helper/program checks.

### M2 — Private execution and redaction

- Add capability-bound private process APIs, detached process groups, closed target stdin, sandbox-preserving argv execution, and streaming secret redaction.
- Exercise only injectable fake-sudo and fake-target fixtures.

### M3 — Ordered TUI handoff

- Add exact one-use approval rendering and keys, cached-auth transition, masked secret resolution, cancellation, stale-generation rejection, and continuation.
- Add reviewed wide/narrow snapshots for each stable state.

### M4 — Surface closure and completion

- Remove the tool from headless, service, scheduled, Plan-mode, subagent, and non-macOS advertisements and reject direct calls.
- Run all focused and completion checks and reconcile the repository harness.

## Acceptance Criteria

- A valid exact invocation runs once only after exact approval; cached sudo authorization avoids the password dialog.
- A cache miss accepts one masked password through an authentication-only helper, then runs the target with stdin closed.
- Wrong password, denial, cancellation, timeout, stale job, wrong owner/nonce, changed binding, and expired authorization fail without target execution.
- Secret echo on either stream, including split chunks, never appears in any live, model, hook, debug, or persisted representation.
- Existing sandbox roots/mode remain effective and network remains denied.
- Every unsupported surface omits the tool and rejects forged calls without prompting or spawning.

## Validation

Focused: privilege, jobs, interaction, approval, tool registry/selection, service/headless, main event-loop, and TUI render tests.

Completion: commands required by `TESTING.md`, including default/all-feature tests, formatting, strict Clippy, release build, smoke checks, diff/snapshot hygiene, and oracle/reference immutability.

Manual/visual: deterministic wide/narrow TestBackend snapshots only.

Blocked checks: any validation requiring real sudo, credentials, provider traffic, runtime mutation, or installed-binary replacement.

## Progress

- [x] Active ExecPlan and P0 task record created.
- [x] Characterization test failed for the missing tool before implementation.
- [x] Typed preparation and private execution are complete.
- [x] Interactive approval and masked handoff are complete.
- [x] Unsupported surfaces fail closed.
- [x] Focused and completion checks are recorded.

## Decisions

- Decision: macOS-only, root-secured native argv with fixed `/usr/bin/sudo`.
  Evidence: user-approved SEC-154 plan.
  Consequence: other platforms and interpreter/shell launchers fail closed.
- Decision: validate authentication separately from target execution.
  Evidence: a cached sudo credential can leave `sudo -S` input unread for the target.
  Consequence: password bytes can never become target stdin.
- Decision: cached authorization is checked first.
  Evidence: user-approved SEC-154 plan.
  Consequence: masked input appears only when required.

## Surprises and Discoveries

- Current macOS protects `/usr/bin/sudo` as execute-only. The fixed helper is
  therefore bound by canonical root ownership, setuid/executable/non-writable
  mode, secure ancestry, device/inode/size/timestamps, and fixed path; native
  Mach-O byte inspection remains required for the requested target.
- macOS temporary paths may traverse the canonical `/private/var` spelling.
  Test preparation canonicalizes cwd and roots so revalidation exercises the
  production binding rather than a textual symlink alias.
- The first complete parallel test run found two PID-fixture observation races
  and one platform-dependent tool-schema count. Bounded PID polling and a
  capability-conditional expectation removed those test-only failures.

## Recovery and Rollback

Keep changes isolated to SEC-154 files and hunks. Revert those hunks without destructive Git operations; process-local handoff state has no persisted migration.

## Completion Record

Changed files:

- Core protocol and secret handling: `src/privilege.rs`,
  `src/interaction.rs`, `src/app.rs`, and `src/main.rs`.
- Tool and unsupported-surface closure: `src/agent/registry.rs`,
  `src/agent/tools.rs`, `src/headless.rs`, `src/service.rs`, `src/schedule.rs`,
  and `src/schedule_v2.rs`.
- TUI coverage: `src/tui/render.rs`, `src/tui/render_tests.rs`, and four
  `privilege_*` wide/narrow snapshots.
- Harness: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `PARITY.md`,
  `TESTING.md`, `docs/.INDEX.md`, and this ExecPlan.

Commands and results:

- Initial missing-tool characterization: expected failure before source work.
- `cargo test privilege::tests --no-fail-fast`: PASS, 8 tests.
- `cargo test privilege_ --no-fail-fast`: PASS, 7 tests.
- Focused masked-input, registry-schema, and prepared-request tests: PASS.
- `cargo test --workspace --no-fail-fast`: PASS.
- `cargo test --workspace --all-targets --all-features --no-fail-fast`: PASS;
  main `ca` 1,843 passed/2 ignored and experiment 118 passed/1 ignored, plus
  all CLI, role-run, and `codex-team` suites.
- `cargo fmt --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  PASS after removing one reported needless `return`.
- `cargo build --workspace --release --all-features`: PASS.
- Released `ca` and `codex-team` help/version smokes: PASS (`0.1.0`).
- `git diff --check`, snapshot hygiene, and protected oracle/reference status:
  PASS.

Remaining risks:

- Sudo policies that require a controlling TTY, retries, askpass, Touch ID, or
  MFA intentionally fail closed. A real credential flow was not exercised.
- V1 remains macOS-only, foreground-only, keyboard-only, and non-networked.
- Rejected launchers/interpreters cover the required named/versioned families
  and a conservative launcher set; any newly introduced general-purpose system
  dispatcher should be added to the closed rejection tests before use.

Harness updates: architecture, continuity, testing evidence, parity status,
knowledge index, and the task record now describe the completed `ca` extension
without claiming Cursor parity. No oracle or reference content was modified.
