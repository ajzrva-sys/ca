# Harness automations and reviewed promotion

An explicitly enabled `harness_improvement` configuration registers the
`harness-self-improvement` automation in ca's normal admission queue. Its route,
request options, price card and harness policy are frozen in the definition.
Disabled configurations do not create an automation, and existing paused
automations stay paused. Changed policy requires an updated confirmed definition.

The target forms are `harness_discovery` with `vault_root`, `harness_cycle` with
`vault_root`, and `harness_evaluation` with `vault_root`, `cycle_id`,
`expected_plan_digest`, `repository` and `baseline_binary`. Evaluation targets
must cover the frozen route, verified pricing and plan budgets. Provider requests
also reserve the occurrence's common request, token, tool and cost caps before
their evidence is delivered to a trial. Unknown completions retain reservations.

Successful evaluations finish in **Needs review**. Old V1 `ReadyToPromote` stages
and `automatic_promotion` fields remain readable; that flag grants no permission
to publish. The immutable authorization binds the exact candidate, evaluation,
repository baseline, executable and applicable guidance.

```sh
ca harness review CYCLE --vault VAULT --repository REPOSITORY --baseline-binary EXECUTABLE --json
ca harness promote CYCLE --vault VAULT --repository REPOSITORY --baseline-binary EXECUTABLE --confirm REVIEW_DIGEST
```

The review command is read only. Promotion requires the displayed digest and
rechecks its evidence before starting publication. Existing activation or rollback
journals continue through their recovery paths. Legacy artifact-learning commands
likewise expose `self-improve review` and `self-improve promote --confirm`;
evaluation and the stable supervisor cannot initiate promotion.

Source activation uses the shared guarded repository publisher. Its deterministic
vault journal retains the previous and candidate refs, exact review evidence,
and index/ref checkpoints. Publication pins the checked-out HEAD, index and
expected old target ref before changing files; a changed branch or ambiguous
interruption preserves the candidate and requires integration review.
Source rollback constructs an additive revert commit with the exact baseline
tree, then uses the same guarded publication. Its original activation journal
and completed V1 revert receipts remain recoverable.

Profile-owned vaults live below the profile's data directory at
`automation/harness`. A pending machine vault requires an explicit migration
before a profile admits independent work:

```sh
ca harness migrate --source LEGACY_VAULT --destination PROFILE_VAULT --json
ca harness migrate --source LEGACY_VAULT --destination PROFILE_VAULT --confirm MIGRATION_DIGEST
```

Migration previews a bounded content inventory, copies into private staging,
validates the records and publishes by rename while retaining the original.
The destination parent must be private and owned by the current user. Pending
cycles continue with their original absolute bindings and authority in the
source vault. Once those cycles become terminal, ca validates and synchronizes
their receipts before admitting new work in the destination. A changed source,
an independently changed destination or an ambiguous effect requires attention;
neither copy is discarded. The stable supervisor keeps health and watchdog duties
while ca owns experiment scheduling and admission.

Native evaluation requires its frozen baseline executable to match the current
ca worker executable. This binds the worker's process containment behavior
without trusting an older binary's version output. An older baseline stops
before provider work and requires the experiment to be frozen and reviewed
again; its candidate and evaluation evidence remain available. Pending review,
activation and rollback recovery continue to use their existing records.
