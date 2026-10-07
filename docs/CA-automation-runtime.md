# ca automation runtime

ca owns occurrence admission, grants, budgets, worker processes, history and
delivery. `ca-automation` executes one bounded run; `ca-automation-protocol`
contains its provider-independent contracts. The old crate names reexport the
native implementations and retain the existing V1 wire schemas.

## Management

Create or update definitions through `ca automation preview FILE` followed by
the exact confirmation digest. `/automation preview FILE` offers the same
review in the terminal. Existing pauses, grants, schedules, timezone handling
and missed-occurrence policies remain in their original stores. The schedule
service admits their due records through one ordered queue.

`/automation history ID` shows durable run UUIDs. `/automation status RUN_UUID`,
`cancel RUN_UUID` and `reconcile RUN_UUID` inspect or manage the same run as the
worker journal. Their CLI counterparts are `ca automation status`, `cancel`
and `reconcile`. Cancellation stops only owned worker processes. A process
monitor stops its observer without signalling the observed process.

Native typed targets work with external repository MCP delegation disabled:

- `automation_workflow` binds an approved workflow and resource digest.
- `mechanistic` executes typed repository queries or declared verification.
- `monitor` binds a `ca-watch` V1 request, without a provider route.
- `repository_task` binds a repository, local branch, permitted paths, worker
  and independent reviewer routes, pinned host checks and integration authority.
- `harness_discovery`, `harness_evaluation` and `harness_cycle` use the same
  admission and durable worker lifecycle. See [automation-harness.md](automation-harness.md).

Ordinary prompts and workflows also run through ca's contained worker, using
ca's existing agent runner. Model usage and missing outcome reservations stay
charged according to the approved budget. Provider-backed harness work checks
that common budget before each physical request.

Untrusted commands receive the configured physical sandbox at their execution
boundary. The ca-owned coordinator separately owns its bounded process,
credentials and journals. On macOS this avoids applying a second Seatbelt
sandbox inside a sandboxed process. Command scopes deny ca's private config,
receipt and lease paths, and commands receive a cleared environment. Query-only
and remote typed workers retain their coordinator sandbox. Required isolation
fails closed when the backend cannot enforce it.

Native V2 admission freezes the isolation mode in its policy digest and passes
that mode to trusted nested workers. Reloading a profile during a run cannot
weaken the admitted command policy. This does not change legacy V1 definition
digests or enable paused definitions. Resource and executor descriptors stay
inside trusted workers; ordinary command children do not inherit them.

## One typed run

`ca automation-engine preview OPERATION.json --run-id UUID` previews an exact
occurrence, resource and policy binding. Add `--allow-network` only for an
operation that requires it. `start` takes the same arguments and
`--confirm DIGEST` from preview. `status`, `cancel` and `reconcile` take that
UUID. Reusing the UUID returns recorded outcomes; a different occurrence needs
a different UUID. The input is a closed `EngineOperation` containing a V1
workflow or mechanistic request. No provider request is needed.

Per-profile run envelopes and complete receipts are under
`automation-engine/v2/runs`. Shared machine resource locks are under
`automation-resources/v2`; a parent keeps them until worker descendants and
output have been cleaned up. Action checkpoints distinguish prepared, running,
confirmed and unknown effects. Unknown effects require read-back reconciliation
or review. Inspecting a run never retries an unknown mutation.

The shared workspace registry removes idle registration metadata only after
exclusive read-back proves both stable claims are free. It preserves the lock
inodes, live inherited claims and uncertain bindings. Historical registrations
therefore do not accumulate in each admission scan. Compatibility workflow
admission waits within the occurrence budget and respects durable cancellation;
a duplicate identity observes the recorded outcome. It never schedules another
occurrence.

## Repository integration

A repository target freezes the local target branch, creates a ca-owned
worktree, implements only the allowed paths, and runs the declared executable
checks after validating their hashes. A fresh reviewer examines the exact
candidate tree. The publication journal binds that tree to host evidence and
review. Under repository/ref ownership, ca rechecks the target and performs a
local fast-forward. The previous and candidate refs remain recoverable.

For a checked-out branch, a prepared Git transaction holds the expected target
and HEAD while an owned index guard protects checkout. Publication checks for
untracked and ignored file collisions before changing the working tree.
Interrupted checkout/ref phases require read-back; an ambiguous partial update
needs attention and is not replayed automatically.

Acceptance and recovery check actual tracked content through a private copy of
the index. Hidden-change flags and stale stat caches cannot establish that the
working files still match the reviewed tree. The real index remains owned and
unchanged during that proof. Reviewed harness source activation uses the same
guarded publisher, while evaluation alone still requires review.

If the branch advances, is dirty, is held by another owner, or its acceptance
evidence changes, ca retains the candidate and marks the run as needing
attention. It does not automatically rebase or force integration. Push,
deployment and installation require separate configured actions. A provider's
success message does not establish acceptance.

## Compatibility migration

`ca automation-engine config-migration preview --source PATH` inventories the
legacy config or state root. `apply --source PATH --confirm DIGEST` copies and
validates those exact bytes and installs compatible V1 workflow records into
the native profile store. Source records remain available for recovery;
conflicting native records are refused. Original absolute resource bindings
retain their original authority. Changed or unavailable resources require a
fresh binding review. Imported interrupted effects remain held for read-back.
Migration does not enable paused records or rewrite live profiles.

Each imported record is checked again against its reviewed bytes when it is
published. Legacy harness commands and native profiles share repository and
vault claims, including a migrated vault's retained source identity. A harness
target cannot execute a different repository from its granted workspace.

`ca automation-engine doctor --repository PATH --delegate-profile FILE`
reports absent Git metadata and stale delegate bindings without changing them.
This source checkout has no Git metadata; repository targets therefore need a
real configured Git checkout. Native workflows and process monitors do not.

## Interactive continuation recovery

After completed tools, a transient connection failure before any new output
can retry the frozen continuation once, preserving its exact request and parent
turn. Completed effects are not replayed. Partial output, changed ownership or
unresolved effects require manual recovery. `/retry` preserves a resolved-tool
continuation's parent identity; new turns and workflows obtain an owned parent
through the child manager before lifecycle hooks are bound.

## Validation

The completed [AUT-259 plan](exec-plans/completed/AUT-259-ca-automation-absorption.md)
records the source binding, gates, terminal receipts and recoverable installation.
Unfiltered default/all-feature workspace suites pass 3,783/3,932 cases with zero
failures or exclusions (43/44 existing ignored). Both strict lint profiles,
formatting and the all-feature workspace release build pass. Actual signed
terminal workflow and required verification complete with zero provider requests
and about 20 ms typing p95; continuation recovery and observer-crash reconciliation
also pass. Evidence is in `scratch/automation-absorption/final-validation/`.

`tools/automation_runtime_pty.py` exercises actual terminal workflow/verification;
`tools/automation_continuation_pty.py` checks frozen retry/parent ownership;
`tools/automation_compat_crash.py` checks compatibility observer-crash recovery.
They use isolated fixtures. `tools/automation_install.py --check` verifies the
built signed pair without replacement. Its default installation preserves both
before-images, journals atomic replacement/signing and verifies the pair;
`--recover RECEIPT` rolls back only a proven owned installation. The current
signed ca/compatibility pair is installed with private timestamped SHA-labelled
rollback copies. Runtime state migration remains explicit and separate.

Legacy schedule V1 and V2 definitions retain their original schemas and confirmation digests. Execution adds profile-private sidecar receipts with the same run identity in the worker and result. Resource and executor descriptors are inherited by owned workers, so a parent crash cannot release an active resource claim. The terminal stops only its owned automation tasks when quitting, then drains their descendants; other profile processes keep their ownership.

Compatibility workflow commands accept `--run-id` to correlate duplicate dispatch
with the native journal under the configured legacy state root. Without it, a
compatibility run receives a fresh occurrence identity. Mutating legacy workflow
commands forward a closed, bounded request to a sibling ca executable or the
explicit `CA_AUTOMATION_HOST`. ca rechecks the original resource and policy
bindings, owns the worker and retains its claims through completion. Closing
the compatibility observer does not cancel an admitted effect. A duplicate run
identity reads the completed receipt without repeating that effect.
