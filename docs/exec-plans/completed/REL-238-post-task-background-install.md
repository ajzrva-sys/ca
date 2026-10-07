# REL-238 — Post-Task Background Release Installation

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-09-03

## Goal

Persist the user's request to build and replace the current executable in a
background job after completed repository changes, then do so for BUG-237.

## Evidence and Current Behavior

- The current user explicitly requested the standing rule and immediate install.
- `AGENTS.md` previously required a fresh installation request; its control and
  stop clauses are updated to recognize this standing authorization.
- `command -v ca` resolves `/Users/aji/.local/bin/ca`, a regular single-link,
  owner-501 mode-0755 file. Its observed SHA-256 is
  `1e91f11413c0d48cdfe6bbc9af2e1f9bee7c98e4d739e87d4865ea2fb2418f72`.
- The already-tested candidate is a strict-signature-valid arm64 Mach-O,
  SHA-256 `0336bd2050b525d71959f885af40ccc603873f534a0821b85e8bdd3c53fd0192`.
- BUG-237 passed focused/full workspace tests, formatting, strict Clippy,
  release, and smokes. Its separate pre-existing oracle baseline discrepancy
  was reported before the user requested this installation.
- Official OpenAI documentation confirms repository-root `AGENTS.md` supplies
  project instructions: https://learn.chatgpt.com/docs/agent-configuration/agents-md.

## Scope

Repository edits: `AGENTS.md`, `NEXT_STEPS.md`, `CONTINUITY.md`, `TESTING.md`,
`docs/.INDEX.md`, and this plan. One temporary background shell runner/log and
an exact same-filesystem recovery/staging directory support the installation.
The runtime side effect is replacement of `/Users/aji/.local/bin/ca` only;
application source, oracle/reference files, credentials, config, and sessions
are not changed by this follow-up.

## Design and Invariants

Run the all-feature workspace release build in the background. Require its
candidate hash to match the validated BUG-237 artifact. Before installation,
recheck the current executable's observed hash so a concurrent installation is
not silently overwritten. Copy the prior executable to a unique timestamped
full-SHA recovery name, verify it, stage the candidate on the destination
filesystem, verify signature/hash/mode/owner and help/version, then rename
atomically. Check the installed artifact and path lookup. On a post-replacement
failure, restore the preserved prior artifact atomically. Record job/log and
recovery location; do not claim success from dispatch alone.

## Acceptance Criteria and Validation

- Standing rule explicitly states background build/install without per-task
  reauthorization and retains recoverability/verification.
- Shell syntax check passes before job dispatch.
- Background release build succeeds and candidate matches the validated hash.
- Old executable is recoverable with its original bytes and metadata.
- Installed artifact hash/signature/mode/owner, `--version`, `--help`, and PATH
  resolution match the candidate; diff hygiene passes.
- The full Rust suite is reused from the preceding turn because this follow-up
  changes no Rust source. The known protected-tree check is recorded separately;
  this transaction does not repair or reclassify its evidence.

## Progress

- [x] Resolve instruction change, executable identities, and existing validation.
- [x] Persist the standing rule with no duplicate authorization requirement.
- [x] Launch and observe the background build/install job.
- [x] Verify replacement/recovery and record final evidence.

## Execution Discoveries

- A raw detached `nohup` attempt exited after its startup line without reaching
  the build. The tool-managed asynchronous session kept the job alive instead;
  future jobs should use that execution mechanism.
- The first managed run built and verified the exact candidate and recovery
  copy, then stopped before replacement: `--help` derives its usage name from
  the executable basename, so `ca.candidate-stage` did not match `ca`. Staging
  now uses a unique `candidate/ca` path, preserving the strict help comparison.
  The current executable's hash was rechecked unchanged after this failure.

## Completion Record

Background tool session 57224 (PID 45741) returned exit 0. The all-feature
workspace release build completed in 0.20s, and the candidate matched the
validated SHA-256. The installed executable is now
`/Users/aji/.local/bin/ca`, SHA-256
`0336bd2050b525d71959f885af40ccc603873f534a0821b85e8bdd3c53fd0192`.
Candidate/staged/installed hashes and help output match; installed strict
signature, regular single-link owner-501 mode-0755 metadata, PATH lookup, and
`ca 0.1.0` pass. Independent post-job checks reconfirmed the same facts.

Recovery copy:
`/Users/aji/.local/bin/.ca-recovery-20260904T004835Z.WDKErD/ca.previous-1e91f11413c0d48cdfe6bbc9af2e1f9bee7c98e4d739e87d4865ea2fb2418f72`.
Its bytes match the observed prior installed SHA-256 and retain its metadata.
The final job log is `/tmp/ca-rel238.kXCjlb/job.log`.

Changed repository files: `AGENTS.md`, `NEXT_STEPS.md`, `CONTINUITY.md`,
`TESTING.md`, `docs/.INDEX.md`, and this completed plan. Shell syntax and diff
hygiene pass. The unchanged-source BUG-237 validation is reused; the separate
known oracle-baseline mismatch remains honestly recorded. No config, session,
credential, protected-tree, or receipt mutation and no CA restart occurred.

## Recovery and Rollback

Retain the unique full-SHA prior executable. Stage that recovery file beside
the installation and atomically rename it back if verification fails. Reverse
only this task's documentation hunks to remove the standing rule if requested.
