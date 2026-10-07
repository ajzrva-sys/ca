# Automation guide

## Choose the entry point

`ca schedule` manages explicit scheduled runs. `ca automation` manages confirmed
local automations and results. `ca automation-engine` operates durable typed
runs without needing a model provider. Use each command's `--help` for accepted
subcommands and arguments; do not substitute one interface's records into another.

Automation definitions, occurrences, budgets, results, and delivery are
profile-scoped. Preview and explicit confirmation bind an operation to its
reviewed resources and authority. Scheduling a record does not prove a run has
completed. Inspect recorded state and receipts before reporting success.

## Typed engine lifecycle

Prepare a JSON `EngineOperation` using the native schemas/contracts. Choose a
UUID for one occurrence, then preview it:

```sh
ca automation-engine preview operation.json --run-id UUID
ca automation-engine start operation.json --run-id UUID --confirm DIGEST
ca automation-engine status UUID
```

Replace `UUID` with the occurrence's actual UUID and `DIGEST` with the exact
preview digest. Reusing the UUID observes/reconciles that occurrence; a new
occurrence needs a new UUID. `--runtime-seconds` and `--output-bytes` bound
execution. Add `--allow-network` only for an operation requiring it. A changed
input or authority requires a fresh preview.

```sh
ca automation-engine cancel UUID
ca automation-engine reconcile UUID
ca automation-engine doctor --repository /path/to/project
```

Cancellation requests cleanup and is not proof that a remote side effect was
reversed. Reconciliation reads durable evidence. Unknown side effects require
read-back or review; status inspection does not retry them.

## Durable state and workers

Run envelopes and receipts use `automation-engine/v2/runs` in profile storage.
Shared resource locks use `automation-resources/v2`. Parent ownership remains
held through worker-descendant cleanup. Provider credentials are resolved in
trusted workers; ordinary command children do not inherit them.

The workspace's `ca-automation` package owns native execution and adapters;
`ca-automation-protocol` owns contracts. `codex-team` packages provide compatibility
forwarding. Ensure required companion binaries are deployed with matching source.

## Repository publication and migration

Repository targets freeze the target branch and candidate scope, work in an
owned worktree, execute declared checks, and obtain review of the exact candidate.
Publication rechecks the target before a local fast-forward. Drift, dirty state,
held ownership, or changed acceptance evidence retain the candidate for attention.
Push, deployment, and installation require separately configured actions.

Legacy migration is previewed using
`ca automation-engine config-migration preview --source PATH`; applying requires
`apply --source PATH --confirm DIGEST`. Migration preserves source records and
refuses conflicting native records. It does not enable paused records or repair
stale resource bindings by guessing.

Read [CA-automation-runtime.md](CA-automation-runtime.md) and
[automation-harness.md](automation-harness.md) for detailed admission, budgets,
continuation, publication journals, compatibility, and recovery contracts.
