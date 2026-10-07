# REL-183 — Request-Scoped Read-Only SSH Authority

Status: COMPLETE
Owner/session: current interactive implementation session
Last updated: 2026-08-26 (completed)

## Goal

An ordinary AutoReview or RunEverything turn that explicitly asks CA to inspect
a named SSH destination runs a closed read-only SSH command for that destination
without showing a redundant network approval.

## Non-Goals

- Do not auto-authorize remote writes, shell mutation, interactive SSH, port
  forwarding, file transfer, or a destination absent from the direct request.
- Do not change Allowlist behavior, explicit Ask/Deny rules, outbound email,
  privilege handoff, browser/MCP network behavior, headless/service/role-run
  behavior, workflows, goals, restored interrupted turns, or stored approval
  grants.
- Do not claim Cursor parity; the user's CA screenshot is direct behavior
  evidence, not an oracle fixture.
- Do not make a live SSH connection, mutate runtime configuration/session
  state, install a binary, or modify `oracles/` or `reference/`.

## Evidence and Current Behavior

- The 2026-08-26 user screenshot shows two queued `run_command` calls asking
  for network approval while inspecting files on `root@bsd`; the initiating
  request already names the BSD host and describes copying `.torrent` files
  read-only.
- `src/workspace.rs::prepare_tool_invocation` infers
  `network_requested = true` for trusted spellings of `ssh`, `scp`, and `sftp`.
- `src/approval.rs::ApprovalBroker::prepare` currently turns every prepared
  network invocation into `BrokerDecision::NetworkPrompt` after Deny and before
  all other authority.
- `src/main.rs::execute_pending_tools_async_with_email` queues that prompt
  before AutoReview or RunEverything can classify the invocation.
- `src/main.rs::active_turn_start_message_count` identifies the initiating user
  message only during a live main turn. Expanded attachment text is appended
  after an `--- Attached file:` marker, so it must not be treated as direct
  user authority.
- `src/permission.rs` contains a conservative closed read-pipeline classifier
  used by tests. It already rejects redirects, substitution, mutation-capable
  `find`/`sort` flags, unknown programs, and unsafe executable resolution.
- Inferred: the screenshot's `[1/2]` denotes two queued SSH tool calls rather
  than two approval layers for one call.

## Scope

Allowed files:

- `src/permission.rs`
- `src/approval.rs`
- `src/interaction.rs`
- `src/main.rs`
- `ARCHITECTURE.md`
- `CONTINUITY.md`
- `NEXT_STEPS.md`
- `TESTING.md`
- this plan, then its completed-plan destination

Forbidden files/areas:

- `src/headless.rs`, service and role-run surfaces
- `oracles/`, `reference/`, runtime config/session data, installed binaries,
  credentials, and Git history

Runtime state affected: none during implementation and validation.

## Design and Invariants

The permission layer will expose a bounded pure classifier for request-scoped
SSH authority. It succeeds only when all of these are true:

1. The tool is built-in `run_command` with a closed, non-interactive `ssh`
   invocation and a remote command.
2. The direct initiating user text, excluding appended attachment content,
   contains the exact destination host/alias as a token.
3. Every remote pipeline stage is on a closed read-only program/flag allowlist.
   Read-only remote glob expansion is permitted; redirects, sequencing,
   substitution, backgrounding, forwarding, alternate SSH config, file
   transfer, and unknown stages fail closed.
4. The interactive approval mode is AutoReview or RunEverything. Allowlist
   remains strict.
5. Persistent Deny and exact Ask remain ceilings. The request-derived authority
   is process-only, applies only while the current main turn is active, and is
   never persisted as a session or config grant.

`ApprovalBroker` will accept the already-computed one-call network authority
and suppress only the generic network prompt. Normal policy classification
still runs. `start_completed_tool_batch` will derive the initiating live user
message from existing process state; workflows/goals/restored turns have no
such authority.

## Milestones

### M1 — Failing characterization

- Add pure permission tests for the reported `root@bsd` read-only inspection,
  exact request binding, attachment exclusion, and unsafe remote forms.
- Add broker/orchestration tests showing current network prompting and policy
  ceilings.
- Expected observable result: the positive request-bound cases fail before the
  implementation while all fail-closed cases pass.
- Validation: focused `permission` and approval/network tests.

### M2 — Bounded authority and wiring

- Implement the closed SSH/read-only/request classifier.
- Carry live initiating user text into interactive tool-batch classification.
- Let the broker suppress only the generic network prompt for an authorized
  call while preserving Deny/Ask precedence.
- Expected observable result: both reported-style safe SSH reads can proceed
  without approval; mismatched/unsafe cases remain queued or denied.
- Validation: focused tests plus related approval/workspace suites.

### M3 — Reconcile and complete

- Update architecture, continuity, testing evidence, and the linked next-step
  record.
- Run repository completion gates and move this plan to `completed/`.
- Expected observable result: complete validation evidence and no protected or
  unrelated file changes attributable to REL-183.

## Acceptance Criteria

- Under AutoReview and RunEverything, a live request mentioning `bsd`
  authorizes `ssh ... root@bsd 'ls ...'` and a second closed
  read-only SSH inspection without a network approval.
- The same command still prompts in Allowlist mode, when the request names a
  different host, when authority appears only in appended attached-file text,
  or when there is no live initiating user message.
- `ssh` without a remote command, `scp`/`sftp`, forwarding/config options,
  redirects, command substitution, sequencing, `find -delete/-exec`, output
  files, `tee`, unknown commands, and obvious mutators remain gated.
- Exact persistent Ask still prompts and Deny still denies.
- Existing local AutoReview, email, privilege, MCP, session-grant, and network
  inference tests remain green.

## Validation

Focused:

- The initial `cargo test -p ca request_scoped_ssh -- --test-threads=1`
  failed as expected because the classifier and broker method were absent.
- Permission tests: PASS, 16.
- Approval tests: PASS, 49.
- Workspace tests: PASS, 7.
- SSH-filtered tests: PASS, 7.
- AutoReview-filtered tests: PASS, 9 CA plus 1 CLI.
- Exact positive classifier, broker, reported two-call batch, deferred ordered
  execution, and no-live-authority network regressions: PASS.

Completion:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS; 2,019 CA,
  38 CLI, 17 role-run, 556 embedded scheduler, and all integration/doc targets;
  three documented manual/external tests ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- `git diff --check`, `.snap.new` hygiene, and protected-tree status: PASS.

Manual/visual: deterministic tests only; no live SSH connection is authorized
for validation. The supplied screenshot is the visual behavior report.

Blocked checks: none currently.

## Progress

- [x] Verified the current network inference, broker precedence, interactive
  batch entry point, live-turn index, attachment expansion boundary, and
  conservative command parser.
- [x] Added characterization coverage; the first focused run failed because
  the request classifier and authority-aware broker entry point did not exist.
- [x] Implemented request-scoped read-only SSH authority, live-turn wiring, and
  authorized-network preservation through ordered execution.
- [x] Ran focused and completion validation.
- [x] Reconciled harness docs and completed the plan.

## Decisions

- Decision: bind implicit network authority to the exact initiating request and
  destination instead of making network generally automatic.
  Evidence: the user explicitly distinguishes network work they already asked
  for from unsolicited access.
  Consequence: benign SSH reads to an unmentioned host still prompt.
- Decision: exclude appended attachment text from authority.
  Evidence: attachments are untrusted context and are stored after a stable
  expansion marker in the canonical message.
  Consequence: an attached document cannot silently authorize network access.
- Decision: scope this change to interactive main turns.
  Evidence: the report is from the TUI and the live-turn index cleanly excludes
  restored, workflow, goal, headless, service, and role-run execution.
  Consequence: other surfaces preserve their existing explicit network rules.

## Surprises and Discoveries

- `[1/2]` is approval-queue progress for two tool calls, not a second approval
  category layered onto one command.
- AutoReview now treats prepared local built-ins broadly; the observed friction
  is specifically the network prompt's earlier broker precedence.
- Automatically authorized network state must travel with a deferred ordered
  execution; otherwise an earlier interaction in the same tool batch would
  silently remove sandbox network access from a later safe SSH read.

## Recovery and Rollback

Revert only the REL-183 hunks with `apply_patch`: remove the pure classifier and
its tests, restore the broker's original `prepare` call, remove live request
threading from the interactive batch, and restore the harness entries. Do not
use destructive Git operations because the worktree contains unrelated user
changes.

## Completion Record

Changed files: `src/permission.rs`, `src/approval.rs`, `src/interaction.rs`,
`src/main.rs`, `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`,
`TESTING.md`, and this plan.

Commands and results: all focused and completion commands listed under
Validation passed after the expected pre-implementation characterization
failure.

Remaining risks: the SSH/read-only grammar is intentionally conservative, so
some harmless utilities or options outside its closed allowlist will still
prompt. No live SSH walkthrough ran because validation was required to avoid
remote side effects; deterministic tests exercise the reported command shape
and stop before process launch.

Harness updates: architecture authority/ownership, continuity, testing
evidence, and the linked next-step record are reconciled. No parity claim was
added.
