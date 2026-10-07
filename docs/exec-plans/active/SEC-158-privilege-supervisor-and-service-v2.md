# SEC-158 — Privilege Supervisor and Authenticated Service V2

Status: ACTIVE
Owner/session: current Codex session
Last updated: 2026-08-23

## Goal

Complete the approved exact-command sudo handoff: a user can type one masked
administrator password for a host-owned elevated command in the main TUI or an
explicit authenticated WebSocket v2 session, while the password never reaches
the model, generic protocol, environment, filesystem, persistence, or logs.

## Non-Goals

- No sudo through `run_command`, `/jobs`, `write_shell_stdin`, model-requested
  input, stdio service, `ca attach`, headless, schedule, role-run, workflow,
  subagent, or autonomous-learning surfaces.
- No shell string, target interpreter/dispatcher, arbitrary environment/stdin,
  background process, target user, password retry, PTY, `su`, `doas`,
  `pkexec`, or real credential in automated tests. The fixed `/bin/cat`
  askpass transport described below is the only askpass exception.
- No change to or downgrade negotiation for the frozen `ca.serve.v1` wire.

## Evidence and Current Behavior

- M1 now uses one owned fake-tested direct askpass supervisor. Every sudo
  process receives a cleared environment and fixed PATH/locale; target stdin
  is closed; `sudo -k` runs before and after; cleanup uncertainty latches the
  capability unavailable.
- Production preparation is macOS-only and requires Seatbelt. The target has
  disclosed system-wide filesystem authority with network denied, while the
  private FIFO directory and its `/private/tmp` alias are explicitly denied
  after broader root rules. Linux and FreeBSD fail closed.
- The target program is restricted to a closed set of root-owned native
  foreground utilities. Program/helper/askpass identity, ordered argv, cwd
  inode and open directory descriptor, timeout, owner, nonce, sandbox, and
  authority are revalidated and digest-bound.
- Only the normal interactive `Agent` mode advertises or executes the current
  `run_privileged` tool. Ask, Debug, Plan, Custom, workflow, headless, service,
  schedule, role, subagent, and learning surfaces remain closed.
- Service v1 remains frozen and secret-free. The v2 protocol, public tool
  rename, and separate network-approval flow have not been implemented.

## Scope

Allowed files: `src/privilege.rs`, `src/interaction.rs`, narrow registry/tool
identity and main-TUI orchestration/render changes, a new isolated
`src/service/v2.rs`, narrow `src/service.rs`/CLI integration, pinned crypto
dependencies, focused tests/snapshots, service docs, root harness, and this
plan.

Forbidden files/areas: `oracles/`, `reference/`, real credentials, user
config/sessions, installed binaries, real sudo invocation in automation, and
unrelated dirty changes.

Runtime state affected: process-local privilege latch/channel state and
disposable fake-helper/loopback test state only.

## Design and Invariants

- The public model tool is the closed `run_elevated_command` request: absolute
  root-secured native `program`, exact `args`, optional workspace-contained
  `cwd`, and bounded `timeout_secs`. It accepts no shell, env, stdin, network
  flag, background, target user, password, or sudo options.
- Eligible normal Main/Execute TUI sessions advertise the tool when the fixed
  helper is trustworthy. Plan/Review, headless, schedule, role, workflow,
  subagent, learning, stdio, attach, and v1 never do.
- Every invocation needs a non-persistable exact elevation approval. Persistent
  Allow/session grants/AutoReview/Yolo cannot approve; Persistent Deny remains
  a ceiling. System-wide filesystem authority/sandbox bypass is explicit on
  the card and digest. Network authority is a distinct approval and is never
  inferred from elevation.
- The host binds program/helper identities, ordered argv, cwd, timeout,
  system-wide authority, network authority, owner/session/turn/call, nonce, and
  expiry into one one-use digest.
- Supervisor lifecycle is exact: fixed `sudo -k`; one direct
  `sudo -A -p <private-fifo> -k -H -- <prepared-program> <args>`; final
  `sudo -k` on every exit. There is no `sudo -v` or split cached-credential
  handoff. `/bin/cat` is the fixed root-owned askpass helper; a private FIFO is
  only an IPC rendezvous and never stores password bytes as regular-file data.
  The password is written only after the helper opens the FIFO, the FIFO is
  unlinked after one delivery, and target stdin is always EOF. With `-k`
  attached to the command, sudo ignores and does not update cached credentials.
- Every sudo process receives `env_clear()` plus a fixed PATH/locale and the
  fixed askpass path only. The sudo policy may construct its standard target
  environment, but no CA/provider/service credential or password is supplied.
- Caller cancellation signals an owned supervisor. The supervisor kills only
  its exact process group, performs final invalidation, and then completes.
  Failed/indeterminate cleanup disables privilege for the process and reports
  manual `sudo -k` guidance.
- Password storage is byte-backed, redacted, zeroizing, non-serializable, and
  absent from Debug. One password attempt is permitted per fresh exact approval.
- On macOS the fixed Seatbelt wrapper uses `/` as the read/write root and denies
  network, giving the disclosed system-wide filesystem authority while keeping
  network denial enforceable. Linux/FreeBSD remain fail closed until a distinct
  network approval or an enforceable network-only backend lands; policy-only or
  off mode must never be rendered as `Network: denied`.
- `ca.serve.v2` is explicit WebSocket-only and requires the exact v2
  subprotocol; v1 is byte-stable and no downgrade occurs. Stdio and attach
  reject v2/secret handoff.
- V2 uses a standard PSK-authenticated Noise channel derived with domain
  separation from the protected bearer token. Encrypted payload bindings cover
  service/channel/session/turn/request/call IDs, digest/nonce, monotonic
  sequence, and expiry. Tamper/replay/drift closes the channel and cancels the
  pending privilege flow through supervisor cleanup.

## Milestones

### M1 — Shared cancellation-safe privilege supervisor

- Add failing fake-helper tests for exact invalidate/auth/execute/invalidate
  ordering, `-H`, cleared environment, final cleanup on every result/cancel,
  cleanup latch, per-call timeout binding, and password/target stdin separation.
- Replace the split free-function lifecycle with one owned supervisor usable by
  the TUI and later service v2. Keep all automated tests fake-only.
- Do not create a reusable sudo timestamp. A direct one-process askpass command
  owns authentication and execution, closes target stdin, and destroys its
  remaining process group even when the direct leader exits first.

### M2 — Approved tool identity and TUI authority flow

- Migrate to `run_elevated_command`, exact schema/digest, system-wide authority
  copy, separate network consent, and masked supervisor handoff.
- Preserve explicit deny and unsupported-surface closure; update reviewed
  wide/narrow states and terminal cancellation tests.

### M3 — Frozen v1 and authenticated WebSocket v2

- Add explicit protocol selection and parallel v2 types/module without adding
  variants to v1 enums.
- Add Noise handshake/state and bound privilege approval/secret/completion
  operations/events. Require daemon/session network upper bounds and a separate
  per-invocation network approval.
- Reject v2 over stdio and attach; keep all v1 golden bytes unchanged.

### M4 — Completion and manual handoff

- Run focused protocol/supervisor/TUI, complete workspace, formatting, strict
  Clippy, release, smoke, snapshot, and protected-tree checks.
- Only after review, offer a user-owned manual real-sudo acceptance run. Never
  request or capture the password in tests or this conversation.

## Acceptance Criteria

- Fake coverage proves invalidate → authenticate → execute → invalidate, exact
  argv, minimal environment, target EOF stdin, redacted split-stream echo, one
  secret attempt, and zero surviving descendants.
- Deny, cancel/disconnect, wrong password, timeout, drift, tamper, replay,
  shutdown, and cleanup failure never run or leave an unowned elevated target;
  cleanup failure latches privilege unavailable.
- The password sentinel is absent from model requests/results, JSON, WebSocket
  plaintext, Debug, events, hooks, traces, jobs, sessions/exports, learning,
  environment, filesystem, and captured output.
- V1 serialization and behavior remain unchanged. V2 cannot start on stdio or
  attach and cannot negotiate without bearer auth, exact subprotocol, and a
  valid Noise handshake.
- No real sudo/password/provider request or runtime mutation occurs in
  automated validation.

## Validation

Focused: privilege supervisor, interaction secret, registry/surface closure,
TUI approval/secret/cancel, service v1 goldens, v2 Noise/binding/replay, CLI
transport refusal, and loopback end-to-end fake helper.

Completion: default and all-target/all-feature workspace tests, formatting,
strict Clippy, all-feature release, both binary smokes, diff/snapshot hygiene,
and protected-tree status.

Manual/visual: reviewed deterministic wide/narrow snapshots. A real sudo probe
is separately authorized and never an automated completion gate.

Blocked checks: real password, real privileged target, and production remote
client interoperability until the fake/security matrix passes.

## Progress

- [x] Existing TUI handoff and service-v1 closure characterized.
- [x] Supervisor/service gaps and approved authority choices recorded.
- [x] Active ExecPlan and task/index pointers created.
- [x] Shared cancellation-safe supervisor and fake-helper matrix complete.
- [ ] Approved tool/TUI authority flow complete.
- [ ] Authenticated WebSocket-only v2 complete with v1 frozen.
- [ ] Completion validation and harness reconciliation complete.

## Decisions

- Decision: do not expose the current SEC-154/155 implementation to service.
  Evidence: inherited environment, missing final invalidation/latch, and direct
  task abort violate approved service security invariants.
  Consequence: service remains fail closed until M1/M2 pass.
- Decision: service secrets use explicit WebSocket v2 only; stdio and attach
  stay non-secret/v1.
  Evidence: user-approved transport choice.
  Consequence: no plaintext secret enters line-oriented JSON bridges.
- Decision: system-wide filesystem authority is an explicit elevation effect;
  network consent remains separate.
  Evidence: user-approved authority choices.
  Consequence: the UI/protocol cannot describe elevation as sandbox-contained.
- Decision: replace the originally planned `sudo -v`/`sudo -n` split with a
  direct `sudo -A ... -k ... <command>` and fixed `/bin/cat` FIFO handoff.
  Evidence: `sudo -v` creates a reusable timestamp, while the local sudo(8)
  manual states that `-k` used with a password-requiring command ignores and
  does not update cached credentials; `-A` keeps password transport separate
  from target stdin.
  Consequence: every approved command asks for one masked secret even when its
  sudoers rule is NOPASSWD, but an unused secret is never delivered to target
  stdin and is zeroized when the command finishes.
- Decision: production privilege is macOS-only until another host has an
  enforceable network-denial backend.
  Evidence: system-wide filesystem authority cannot truthfully be described as
  network-denied under Linux/FreeBSD PolicyOnly/Off.
  Consequence: those hosts fail before approval or password input.
- Decision: allow only fixed root-owned native foreground utilities and deny
  the private askpass directory explicitly after system-wide sandbox allows.
  Evidence: arbitrary launchers can escape process ownership, and a NOPASSWD
  target could otherwise race or enumerate the FIFO.
  Consequence: shell/interpreter/dispatcher/custom executables are rejected and
  the elevated target cannot inspect the password rendezvous.

## Surprises and Discoveries

- Existing SEC-154/155 documentation says the target environment is private,
  but `run_private_process` inherits almost the complete CA environment. The
  supervisor must close this before any remote support.
- The first M1 draft still had three fail-open gaps: successful leaders could
  leave descendants holding pipes, `sudo -n -v` returned an unowned timestamp,
  and the separate `sudo -S -v` authentication exposed a reusable credential
  window. It also rendered network denial where PolicyOnly/Off enforced none.
- Independent review found four more M1 gaps and closed them: a NOPASSWD FIFO
  race under the `/` allow, dispatcher/process-group escape, cwd path
  replacement after approval, and privilege advertisement in non-Agent modes.
  A final review also found that sudo may use a PTY monitor in another session;
  cancellation now sends catchable TERM and waits for bounded relay/reap before
  any forced kill, with a setsid-monitor regression.

## Recovery and Rollback

Keep v2 parallel to frozen v1. Use narrow inverse patches and leave the
privilege capability omitted if any milestone is incomplete. Test fixtures own
and reap exact fake processes; no persisted migration exists.

## Completion Record

Changed files: `src/privilege.rs`, `src/sandbox.rs`, narrow privilege registry,
main orchestration/mode gating, TUI interaction/render coverage, and the root
harness. No service-v2 source was added.

Commands and results: privilege 20/20, sandbox 10/10, privilege surfaces 10/10,
masked input 2/2, formatting, strict all-target/all-feature Clippy, default
workspace tests, all-feature release build, and release help/version smokes
pass. The final all-target/all-feature matrix had one unrelated flaky
`codex-team` clean-filter exact-count assertion; its focused rerun passed and
REL-160 records it. No real sudo/password run occurred.

Remaining risks: production password acceptance is unverified; tool rename,
separate network approval, authenticated WebSocket v2, and remote
interoperability remain M2/M3/M4.

Harness updates: SEC-158 current facts and fake-only limits are recorded in
`CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and this plan.
