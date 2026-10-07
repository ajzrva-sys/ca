# REL-135 — Sandboxed SSH and Read-Only Approval Friction

Status: COMPLETE
Owner/session: Codex / current user-requested repair
Last updated: 2026-08-17

## Goal

Make an ordinary macOS `run_command` able to resolve the invoking user before
SSH starts, infer network authority for an obvious SSH client invocation, and
make the default AutoReview policy execute a narrowly classified set of
read-only local commands without repeated approval.

## Non-Goals

- Do not weaken the one-use approval required for network access.
- Do not auto-approve SSH, remote commands, shell redirection, substitution,
  sequencing, backgrounding, arbitrary pipelines, Git/Cargo, or any command
  that may execute another program or write a caller-selected path.
- Do not expose `~/.ssh` outside the existing workspace sandbox roots, mutate
  user configuration, install a binary, or change service approval semantics.
- Do not claim Cursor parity or modify oracle/reference evidence.

## Evidence and Current Behavior

- User screenshots show `ssh bsd ...` and `ssh -F ... bsd ...` failing with
  exit 255 and `No user exists for uid 501`, including after the SSH alias was
  read successfully.
- A direct macOS `sandbox-exec` characterization using the current generated
  policy reproduces `id -un` as numeric `501` and SSH's exact UID error.
- Adding only the OpenDirectory `libinfo` Mach lookup makes `id -un` return the
  account name and makes `ssh -G` complete. This is source-command evidence,
  not oracle evidence.
- `src/sandbox.rs::sandbox_profile` denies all unspecified Mach lookups.
- `src/workspace.rs::prepare_tool_invocation` requests network only from a
  static capability or model-supplied `network = true`; `run_command` with an
  obvious `ssh` executable does not infer it.
- `src/agent/registry.rs` conservatively marks every `run_command` as mutating.
  `src/main.rs::tool_needs_approval` and the corresponding headless branch use
  only that static marker, while the default approval mode is `allowlist`.
- Inferred: the static classification makes harmless model-selected shell
  reads prompt even when the user expects AutoReview behavior.

## Scope

Allowed files:

- `src/sandbox.rs`, `src/jobs.rs`
- `src/workspace.rs`, `src/agent/registry.rs`
- `src/permission.rs`, `src/main.rs`, `src/headless.rs`, `src/config.rs`
- focused tests in those modules
- `docs/.INDEX.md`, `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`,
  `NEXT_STEPS.md`, and this plan

Forbidden files/areas:

- `oracles/`, `reference/`, user runtime config/session/credential files
- service approval protocol, outbound communication approval, scheduled grants
- SSH key/config contents, automatic profile migration, binary installation

Runtime state affected:

- Tests and manual checks may create disposable temporary directories and
  ordinary bounded child processes. One explicit SSH `true` smoke check may
  use the user-named `bsd` alias without printing config, keys, or remote data.
- No saved profile/session, remote file, installed binary, or credential is
  changed.

## Design and Invariants

- Add only `com.apple.system.opendirectoryd.libinfo` lookup authority to
  ordinary macOS sandbox profiles. Keep the closed experiment profile from
  gaining this authority.
- Infer network use only for a bounded set of obvious network-client executable
  names, beginning with SSH-family clients. Inference may add an approval; it
  must never remove one.
- Keep `run_command` statically mutating. AutoReview uses a separate closed,
  bounded read-only invocation classifier so workflow/evidence semantics do
  not flatten process execution into a general read tool.
- The classifier accepts only closed shell words and pipelines composed solely
  of an exact allowlist of non-writing utilities. Reject shell expansion,
  redirects, sequencing, backgrounding, unquoted glob syntax, unterminated
  quotes, executable wrappers, dangerous utility flags, and unknown programs.
- Persistent Deny and Ask rules, network approval, and unsafe-shell approval
  continue to take precedence over AutoReview. RunEverything still cannot
  bypass unsafe shell or network approval.
- New and omitted agent configuration defaults to `auto-review`; explicit
  `allowlist` remains supported and unchanged, and no existing file is
  rewritten or migrated.

## Milestones

### M1 — Characterization

- Add a macOS job regression proving an ordinary required sandbox resolves the
  current UID while the experiment profile stays closed.
- Add workspace preparation regressions proving obvious SSH commands request
  network without relying on model arguments.
- Add classifier and interactive/headless approval regressions that fail under
  the current static mutation policy.
- Validation: focused test filters for sandbox/jobs/workspace/permission and
  approval orchestration.

### M2 — Implementation

- Add the least OpenDirectory authority to ordinary generated profiles.
- Add bounded obvious-client network inference and schema guidance.
- Add the closed read-only shell pipeline classifier and route TUI/headless
  AutoReview through it without weakening rule precedence.
- Change semantic agent defaults and their characterization to AutoReview.

### M3 — Validation and Harness Reconciliation

- Run focused tests, then `cargo test`, format check, strict all-target Clippy,
  and release build per `TESTING.md`.
- On macOS, run a generated-policy identity test and one read-only `ssh bsd
  true` smoke check with network allowed.
- Update architecture, continuity, testing, next-step, and index records. Move
  this plan to completed only after all required local gates pass.

## Acceptance Criteria

- A required ordinary macOS command sandbox returns a nonnumeric current user
  name from `id -un`; the prior exact UID failure is covered by regression.
- `run_command {"command":"ssh bsd true"}` is prepared as network-requesting
  even when `network` is omitted, and it still requires one-use network
  approval.
- Under AutoReview, exact safe local reads such as `rg pattern src`, `ls -la`,
  and `ps auxww | grep '[r]sync'` do not prompt.
- Redirection, substitution, sequencing, `tee`, `find -exec`, `rg --pre`, SSH,
  Git, Cargo, wrappers, unknown commands, and malformed shell remain approval
  gated; explicit Ask/Deny and network policy still win.
- New/omitted configs default to `auto-review`; explicit `allowlist` remains
  loadable and behaviorally distinct.
- Focused and completion checks pass with actual results recorded.

## Validation

Focused: PASS. The initial classifier characterization failed to compile
because `auto_review_safe_command` did not exist. After implementation, 10
permission, 7 sandbox, 7 workspace, 17 job, and 70 config-store tests pass,
plus focused interactive/headless AutoReview and inferred-network approval
tests.

Completion: PASS. `cargo test --no-fail-fast` passed 1,557 main and 37 CLI
tests with one ignore. The complete all-target/all-feature rerun passed 1,586
main, 118 experiment, and 37 CLI tests with two ignores. The first all-feature
run had one unrelated voice descendant-cleanup timing failure; the isolated
test and the complete rerun passed. Format, strict all-target/all-feature
Clippy, and the all-feature release build pass.

Manual/visual: PASS for the behavior boundary; no TUI layout changed. A
required macOS sandbox matching the ordinary profile resolved the user and
completed `ssh bsd true` with BatchMode, strict known-host checking, a
five-second connect timeout, and network enabled. It produced no output and
exit code 0.

Blocked checks: physical FreeBSD is not required for this macOS-specific user
regression unless a platform-neutral change fails portability gates.

## Progress

- [x] Reproduced the UID failure with the current sandbox authority.
- [x] Identified the least macOS Directory Services lookup that repairs it.
- [x] Added failing focused regressions.
- [x] Implemented the bounded fixes.
- [x] Passed focused validation and SSH smoke check.
- [x] Passed completion gates and reconciled harness documents.

## Decisions

- Decision: preserve an explicit one-use network approval for SSH.
  Evidence: network authority is a separate existing invariant and the user
  complaint is failed execution, not a request to silently allow network.
  Consequence: SSH becomes usable after one approval, never silent.
- Decision: do not mark `run_command` statically read-only.
  Evidence: arbitrary shell execution remains mutating even though a closed
  subset can be characterized for AutoReview.
  Consequence: schedules, workflows, services, and unknown call sites remain
  conservative.

## Surprises and Discoveries

- The exact SSH stderr occurs before network connection: blocked macOS
  `getpwuid(501)` lookup, not SSH alias, key, host, or remote authentication.
- A model-supplied `network` boolean is currently the only way a generic shell
  command requests network, so repairing Directory Services alone would expose
  a second avoidable failure for obvious SSH calls.

## Recovery and Rollback

All changes are local source/doc edits. Revert only the REL-135 hunks or move
this plan back to active; do not reset, checkout, or disturb the user's broader
working tree. Disposable processes and temporary directories are owned by the
tests/job manager and are shut down or removed on completion.

## Completion Record

Changed files: `src/{sandbox,jobs,workspace,permission,config,headless,main}.rs`,
`src/agent/registry.rs`, `docs/.INDEX.md`, `ARCHITECTURE.md`, `CONTINUITY.md`,
`TESTING.md`, `NEXT_STEPS.md`, and this completed plan.

Commands and results: focused suites and native SSH smoke PASS; default tests
PASS (1,557 main, 37 CLI, one ignored); full all-feature rerun PASS (1,586
main, 118 experiment, 37 CLI, two ignored); formatting, strict Clippy, and
all-feature release build PASS; release help/version, diff check, snapshot
hygiene, and oracle/reference diff check PASS.

Remaining risks: existing profiles explicitly saved as `allowlist` are not
silently migrated; users choose AutoReview explicitly. Network and remote SSH
commands still require one-use approval. The read-command allowlist is
deliberately conservative and unknown utilities continue to prompt.

Harness updates: architecture ownership, current continuity, validation
evidence, next-step status, and knowledge index reconciled. This is an
intentional `ca` extension/repair, not a Cursor parity claim.
