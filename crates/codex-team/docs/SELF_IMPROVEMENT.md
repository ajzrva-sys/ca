# Autonomous Learning Fabric

The self-improvement subsystem is disabled until a person reviews and supplies
the exact enrollment digest. Configuration alone does not enroll the machine,
install a service, start a provider request, change source, or switch a binary.

## What learns

Hosted model weights do not change. `codex-team` records structurally scrubbed
role traces, immutable evidence capsules, objectives, frozen evaluation plans,
artifact revisions, provider/model overlays, and deployment outcomes. Future
calls can receive an exact shared-core revision and one route-specific overlay.
The existing evidence-only Rust recipe compiler remains separately isolated and
does not gain source-write or network authority. Under exact autonomous
enrollment, an admitted evidence-only task may publish its observed typed recipe
through the compiler's existing fresh isolated validation and immutable approval
path without a second recipe-specific human decision.

Human corrections and rollbacks supersede all machine-positive evidence.
Deterministic acceptance is next. Independent provider judgment is used only
for a true deterministic tie, and all available non-author judges must agree.
A candidate cannot regress a deterministic baseline pass or remove a
higher-priority acceptance property. Cost or latency-only promotion requires at
least ten percent improvement with verifier-equivalent output.

## Stable boundary

`codex-team-supervisor` is a separate binary. Enrollment copies and hashes it
once; autonomous work cannot update it. It owns the global lock, pause state,
known-good pair, private vault, login-service contract, and A/B recovery state.
It does not handle model prompts, credentials, objectives, or candidates. On a
healthy tick it invokes the verified active `codex-team` binary for exactly one
bounded worker stage at low priority, without holding the supervisor state lock.
Timeout, pause, or foreground work terminates the worker process group.
The safety manifest protects supervisor, credential, trace scrubber, repository
guard, verification isolation, cancellation, rollback, and protected-tree
authority. Dependency additions and lockfile dependency changes are excluded.

An evaluation-promoted shared-core package can be injected into later role
calls, with one exact provider/model overlay layered on top. Packages are
content-addressed and may contain multiple text or binary members. Binary
assets and script text remain inert and non-executable; only explicitly marked, secret-free
UTF-8 members can enter a prompt, within a fixed aggregate bound.

The source transition engine accepts one commit rooted at the exact clean
current HEAD. It refuses branch/index/tree/submodule drift, protected or private
trees, safety-kernel paths, dependency manifests, multi-commit candidates, and
failed diff hygiene. The artifact worker never invokes this primitive. Source
changes, builds, commits, dependency changes, candidate binary activation, and
deployment are inactive in this slice; only operator rollback remains public.

## Enrolled `ca` bridge and artifact loop

Enrollment accepts repeatable `--ca-profile NAME` evidence profiles and one
`--ca-execution-profile NAME` (default `codex-team`). The exact confirmation
digest includes every offline bridge preflight and the execution profile's
Codex/DeepSeek/MiMo route matrix. Enrollment-v1 records deserialize with an
empty evidence allowlist and therefore contribute no `ca` sessions.

Only enrolled profiles switch Dream/Distill to candidate-only behavior. `ca`
queues a bounded sanitized intent, exports it without provider traffic, and
removes it only after exact digest acknowledgement. Import, objective creation,
acknowledgement, evaluation, candidate generation, every replay, judgment, and
promotion are durable and idempotently recoverable.

The worker performs one transition per invocation:

```text
observation → objective → historical/adversarial freeze → one candidate
→ baseline replay → candidate replay → independent judgment → promotion
```

With fewer than five historical inputs, three adversarial inputs must be frozen
by a provider different from the candidate author. Shared artifacts replay on
every enrolled route; a provider overlay replays only on its exact target route
and uses other enrolled provider routes as unanimous non-author judges when
deterministic evidence cannot decide. Baseline passes and acceptance properties
cannot regress. Cancellation, route drift, isolation loss, and provider
unavailability stop without counting as candidate-quality failures. Two
identical typed candidate failures create a 24-hour objective cooldown.

Promoted memory, complete skill packages, role prompts, and provider overlays
are immutable and injected into future role calls with exact revisions. Skill
references, templates, tests, assets, and scripts may be packaged, but every
file remains inert; only bounded secret-free UTF-8 members enter prompts.

The owner-only vault is `~/.codex-team/self-improvement/`. Existing path
components and records must be real, non-symlink entries. Directories are mode
`0700`, records are mode `0600` (the immutable safety manifest is `0400`), and
writes are temp-file, fsync, rename, and parent-fsync. Free space below the
greater of 10 GiB or five percent pauses learning without deleting evidence.

## Trace privacy

Known secret-bearing fields are omitted structurally. Authorization, cookies,
passwords, tokens, private keys, secret environment entries, and Keychain lines
are redacted. Unknown tool arguments and results are stored as hashes only.
Repository and task identities in traces are fingerprints, not canonical paths.
The trace capture path never resolves provider credentials.

## Operator interface

Use `codex-team self-improve status --json` and `doctor --json` before any host
action. `pause` is immediate. `resume` requires the exact reviewed supervisor
revision. MCP exposes the same read surfaces and requires
`human_confirmed = true` for pause, resume, or rollback; these calls do not make
a model request. The pinned repository cannot be overridden through MCP.

Enrollment requires clean named branches for both source repositories, regular
non-symlink release binaries, a sibling `codex-team-supervisor`, and the exact
digest printed by a rejected dry confirmation. Enrollment is idempotent for the
same contract and refuses a different contract.

## Rollout state

This remains disabled by default and pilot-only. Both repositories' full
all-target/all-feature matrices pass, as does a disposable process-level pilot
that exports and acknowledges an enrolled intent, exercises fake Codex,
DeepSeek, and MiMo routes, and globally activates one replay-approved skill
without moving either source HEAD or creating a deployment record. The final
release `ca` role contract also passes. Actual `ca::McpManager` observe/operate
compatibility passes with the debug-only isolation double; the release server
fails closed in this managed app because the production macOS Seatbelt backend
cannot self-test here. Rerun that one gate on an isolation-capable host before
real enrollment. No live provider, installation, login service, source
transition, build, commit, candidate slot activation, or deployment has been
authorized or run.
