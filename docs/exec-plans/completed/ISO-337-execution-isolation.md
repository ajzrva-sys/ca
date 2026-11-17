# ISO-337 — Child execution isolation, mediated effects, and offline evaluation

Status: INSTALLED as ca 0.1.95 from gate `scratch/verify/20261109T085617Z-78373`, SHA-256 `b9162136f9fb3c19ac28d768edf5a22738e048c08dd816a4d5ed30ad487707de`. A confirmed policy generation rejects a nested spawn of the old grant. The restricted-read admission lock, live-proxy revocation, vetted-proxy address and socket-family failures, and active-route cache headroom are in that artifact. Linux and FreeBSD isolated-by-default stay externally unverified. Linux `nono` native enforcement was not run on this macOS host. Do not claim a real-provider or real-model benefit.

This checkout has no root Git metadata and no `.agent/PLANS.md`. Do not initialize a repository. Temporary Git repositories inside fixtures are allowed. Record exact source inventories with the existing disposable-copy tooling. At implementation start, reread `Cargo.toml`, the head of `CONTINUITY.md`, and `ca --version`. The plan below was checked against package **0.1.88** on 2026-11-08. If that baseline has moved, use the later qualified baseline and do not rewind it.

## Outcome

Children receive an immutable host grant and can use only the tools, paths, commands, destinations, and credential operations that grant allows. On a qualified macOS host, an absent `agent.child_execution` setting runs that work in a confined worker. Networking and credentials are host-mediated. New harness adoptions require a separate confirmation phase. Security effects and corrected-skill outcomes are durable and attributable to the invocation that actually ran.

Scripted loopback providers and local fixtures are the acceptance evidence. They do not establish real-model quality, real-provider compatibility, cache savings, or external connectivity. Do not run paid evaluations. Do not promote experimental guidance into the production profile. Exclude the sibling-agent cache-stagger experiment and `ca-hrs-252` / HRS-252.

## Verified baseline and the gaps that justify the work

Installed continuity records signed CA 0.1.88, FLT-333 task maps, and FLT-335/336 repairs. Preserve the fixed 100,000-token shared worker ceiling, separate planner accounting, settlement of known over-limit and post-cancellation usage, selected reasoning effort, attachment handling, native history, exact-revision task approval, independent-branch continuation, durable worker reservations, privacy projection, permission ceilings, canonical history, and running sessions. Leave historical failed and incomplete comparisons labelled as they are. Local fixture success must not relabel FLT-334 or any earlier unqualified comparison.

These gaps are present in the current tree:

1. `prepare_child_tool` in `src/agent/subagent.rs` skips `grant.path_scopes` for typed read-only tools that are not process or Git capabilities. The remaining check is `Path::starts_with` on a string.
2. `prepare_isolation` replaces `path_scopes` with every root of the attached worktree.
3. `JobExecutionPolicy::extend_sandbox_roots` adds `/` as read-only for inherited jobs, and ordinary environment preparation still accepts overlays. `ExperimentClean` exists only under `cfg(test)`.
4. `sandbox_profile` turns `network_allowed` into `(allow network*)` plus `append_credential_rules` (securityd, Keychains, SSH agent socket, `.ssh`, `.config/gh`, `.gitconfig`, `.git-credentials`). `network_allowed_profile_grants_keychain_agent_and_vcs_credentials` currently asserts that behavior.
5. `src/main.rs` is `#[tokio::main]`. `LaunchArgs::HarnessWorker` is parsed only after Tokio starts.
6. Checkpoint suppression is `through_message_count`. `context_limits` reads `state.current_model`. `active_turn_route` is a separate fallback route.
7. `LearningLedger::record_skill_invocation` and `record_skill_outcome` call `registry.inspect_instance` and `skill_event` stores `instance.content_digest`. `SkillLoad.content_digest` is ignored.
8. `harness_hints::single_addition` accepts only a superset of size `baseline + 1`. `harness_trials` and `harness_generation` call it.
9. `HarnessPlan::validate` requires `repetitions == 3`, at least two `held_out` cases, and one `predicted_fix && !held_out` target. A two-case confirmation phase cannot pass that function honestly.

Existing selectors that match current tests, and must keep passing:

- `admitted_children_keep_frozen_ceiling_and_observe_live_tightening`
- `nested_spawn_inherits_subset_and_obeys_depth_limit`
- `child_execution_enforces_prepared_path_command_network_and_job_owner_scopes`
- `optional_child_worktree_has_owned_branch_and_truthful_receipt`
- `experiment_environment_is_closed_and_redacted`
- `prepared_prompt_cache_key_is_stable_and_route_layout_scoped`
- `prompt_cache_epoch_is_an_extension_or_has_a_recognized_invalidation`
- `checkpoint_prepares_a_typed_request_before_provider_io`
- `checkpoint_ready_during_tool_turn_waits_for_prompt_epoch_finish`
- `generator_failure_does_not_replace_existing_checkpoint`

An unmatched selector is not coverage. Extend these tests before adding replacements.

## Ownership

One integration owner edits shared Rust, freezes the tree, runs verification, bumps the version, and installs. Fixture text, snapshot fixtures, and documentation drafts may be prepared beside that work, but they merge only at the owner’s freeze. No concurrent Cargo stage may use the same target directory.

Evaluator behavior stays in `crates/ca-automation`. `codex-team` only re-exports it. Do not add a second evaluator.

New modules, each added to `src/main.rs` and to `HARNESS_PROTECTED_SOURCE` in `crates/ca-automation/src/self_improvement/harness_protected.rs` in the same change that introduces them:

| Module | Owns |
|---|---|
| `src/execution_grant.rs` | `AdmittedExecutionGrant`, intersection, path class, worktree rebase, generation checks |
| `src/network_broker.rs` | Destination and credential-operation references, per-worker proxy, revocation |
| `src/isolated_worker.rs` | Pre-Tokio launch, handshake, scoped IPC, worker lifecycle |
| `src/harness_confirmation.rs` | Confirmation catalog, reservation, consumption, verdict integration |
| `src/security_journal.rs` | Security-record metadata and restricted-flow provenance |

Candidate harness agents must not edit those files, `src/sandbox.rs`, `src/harness_verifier.rs`, or the confirmation catalog. The protected-source list is host authority.

`AdmittedExecutionGrant` is an `Arc` created only by host admission. Deserializing a worker message, restoring a session, or accepting model arguments does not create one. It carries version, grant id, owner, parent lineage, policy generation, digest, allowed tools, exact command invocations, separate read and write scopes, scope kind (`ExactFile` or `DirectoryTree`), writable output directories, runtime/toolchain read scopes, environment policy, destination and credential-operation references, protected host paths, effective execution mode, and required backend capabilities. Ordinary main-agent contexts keep today’s policy path. Every admitted child carries an explicit grant.

`AgentGrant` remains the narrow request view used by existing spawn records. It is not the enforcement object.

## Public contract

- `isolation = shared | worktree` stays workspace placement. `prepare_isolation` still creates the worktree; it must stop replacing scopes.
- Add `agent.child_execution = "isolated" | "in-process"` on `AgentConfig`, serde default absent. Absent resolves to `isolated` only after that platform’s native qualification is recorded. Until then it resolves to the existing in-process path. Explicit `isolated` fails closed when a required capability is unavailable. No model-facing argument can select or weaken this mode.
- Spawn requests gain optional requested read paths, writable output directories, destination references, and credential-operation references. They may only narrow parent authority. Widening still uses the existing human approval path.
- New registry tools `http_request`, `git_operation`, and `ssh_operation` select an approved reference plus operation parameters. Schemas reject secrets, authentication headers, alternate executables, and raw endpoints.
- Extend harness show/status JSON already printed from `src/harness_control.rs`. Do not add a dashboard.
- Keep Automation V2 and `SessionV4`. New security, confirmation, and learning metadata use their own versioned stores and tolerate old files.

## Release sequence

Expected versions if 0.1.88 is still current at each gate: R1 `0.1.89`, R2 `0.1.90`, R3 `0.1.91`, R4 `0.1.92`. Re-read before each bump. Run `python3 scripts/bump-release-version.py` once, after that release’s focused checks and before `./scripts/verify.sh`, so the gate artifact is the installed binary. Do not bump again if the version already advanced for that release. If the full verifier changes, run `./scripts/verify.sh --self-test` and keep the receipt.

Each release’s private evidence directory is `scratch/ISO-337/rN/`. It holds the plan delta, fixture sizes, source inventory, candidate hash, synthetic provider requests, raw VT and decoded frames, per-case outcomes, independent artifact checks, provider/tool/approval counts, timing phases, token and cost availability with provenance, grant and backend identities, cleanup observations, failures, timeouts, unknown outcomes, unstarted cases, and `assessment.json`. Snapshot headers only from a reviewed allowlist. Use fixed IDs and synthetic credentials.

Terminal qualification for each installed binary must show a second pending update replacing the first, with at least 192 history messages. A stored snapshot or a finished-result screenshot is not that proof.

Install only through the gate artifact:

```sh
bash tools/bug257_install.sh --check
bash tools/bug257_install.sh
```

Then verify candidate, staged, and installed hashes, mode, ownership, `codesign -vvv` (“valid on disk”), `ca --version`, `ca --help`, the recovery copy, and one ordinary launch. Leave running sessions and production config alone. Restore the recovery copy if readback fails.

### Release 1 — Grants, closed child environments, installed skills, request snapshots

Goal: admitted children obey one grant at every effect, installed skills run after their source checkout is gone, and catalogued provider bodies are snapshotted after production preparation. Model loops stay in the host. `child_execution = "isolated"` is stored and fails because worker confinement is not qualified yet. Absent still means in-process.

#### R1.1 Admission

In `execution_grant.rs`, pure functions with table tests:

- Intersect requested tools, exact commands, read scopes, write scopes, output directories, destinations, and credential operations with the parent grant and current policy. Empty requested sets mean the parent’s corresponding set, except network and credentials, which stay closed unless the parent grant already names them.
- Reject unknown tools, commands outside the exact invocation list, stale policy generations, and required capabilities the backend cannot report.
- Rebase a scope into a worktree only when it belongs to that worktree’s source root. Preserve `ExactFile` versus `DirectoryTree` and read versus write. Reject a scope that cannot be rebased. Never replace the set with all attached roots.
- Path class is not `starts_with` on a string. Match path components after lexical normalization, then require the effect boundary to re-check the opened or created node.

Call the intersection from `spawn_with_inherited_policy`, `spawn_batch`, nested spawn, graph workers, explorer, planner, and reviewer admission. Keep batch size, parent-turn, and concurrency checks in `spawn_batch` as they are. Store `Arc<AdmittedExecutionGrant>` on `AgentRunContext` and `ChildToolContext`. `validate_spawn_request` still rejects tools outside the advertised subagent set and depth over the limit.

Failing tests first: a read-only `read_file` of a sibling is denied; a worktree child with one assigned file cannot read another root; a stale generation is rejected; a non-rebasable scope is rejected rather than widened.

#### R1.2 Effect boundaries

Thread the grant into `ToolContext` (`src/agent/registry.rs`) and the child `JobExecutionPolicy`. `execute_tool_with_context`, `execute_prepared_tool_in_workspace_policy_owned_with_skills`, `execute_file_mutation`, formatter launch (`formatting::prepare_with_authority`), foreground commands, background jobs, and nested children all consult it.

Use existing workspace resolution and mutation. Add an anchored check at open and create time so a symlink swap or a new path cannot pass a check that only canonicalized once. Exact-file rules:

- Shell commands may read the assigned source and may not write it.
- Native write, patch, edit, and create run only for an admitted exact target or a file inside an admitted writable output directory, with the existing drift check.
- Atomic replacement of an exact file does not admit the parent directory.

The current `typed_read` bypass in `prepare_child_tool` goes away. `ResourceKind::Url` no longer becomes true merely because `grant.network` is true; destination checks arrive in R2, and R1 denies child network unless a destination reference is already present and approved. With no R2 broker yet, child network stays denied in R1 even if an old `network: true` flag is set. Update `child_execution_enforces_prepared_path_command_network_and_job_owner_scopes` so it expects that denial, and keep the job-owner assertions.

Mandatory negatives: sibling read and search, second-file write, symlink retarget after the lexical check, escape through a newly created path, and one assigned scope becoming another root after worktree rebase.

#### R1.3 Child environment

Add `JobEnvironmentPolicy::AdmittedChild`, applied after the existing inherit/overlay preparation, only for contexts that carry an `AdmittedExecutionGrant`. Build a fresh environment: private `HOME`, temp, and build directories, plus an explicit toolchain read list on the grant. Do not copy the user Cargo home to share compiler caches. Drop overlays that would restore `HOME`, `SSH_AUTH_SOCK`, proxy bypass (`NO_PROXY` and lowercase variants), loader variables (`DYLD_*`, `LD_*`), compiler wrappers (`CC`, `CXX`, `RUSTC_WRAPPER`, `CARGO_BUILD_RUSTC_WRAPPER`, and similar), or unrelated project environment keys.

Stop adding `/` as a read-only sandbox root for admitted children. Keep main-agent `Inherit` behavior, including the existing experiment-clean test. Describe the release as constrained tool and process access.

#### R1.4 Installed skills

Add one disposable skill fixture with `SKILL.md` (prerequisite and completion guidance), `references/guide.md`, and a script that writes a deterministic artifact. Drive the real `ca skill` CLI:

1. `ca skill install SOURCE` shows a preview and does not publish.
2. Confirm with the displayed full-tree digest.
3. The installed instance is disabled.
4. Discover and enable that instance and digest.
5. Remove the source checkout.
6. Start ordinary ca against a loopback provider and select the installed skill.
7. Read the installed reference and run the script through normal admission.
8. An independent check reads the artifact.
9. Repeat with the prerequisite missing. Guidance names the missing prerequisite and does not invent a run or install anything.

Also fail stale digest, disabled instance, missing resource, resource drift, and install-state publication. Fix only failures those cases demonstrate.

Update `new_skill` in `src/skill_cli.rs` and `generate_distill_bounded` in `src/learning.rs` so newly authored skills include the workflow standard: when to use it, prerequisites, the reference to read, the command to run, and how to tell completion. Import of arbitrary third-party skills does not gain a heading requirement.

#### R1.5 Provider snapshots

Derive the case list from `provider_templates::ALL` and each template’s actionable availability. Add native Codex, one custom endpoint, and distinct first-party model contracts that the catalog does not already separate. Capture the body after `ProviderAdapter::prepare`, privacy and context projection, contract resolution, adapter transformation, body limits, and final destination and header handling (`stream_prepared` / `stream_prepared_inner` / `stream_adapter_request`). Cover Chat Completions, Responses, Anthropic Messages, and Google GenerateContent; initial, completed-tool continuation, and checkpoint requests; default, disabled, and elevated reasoning. Unsupported catalog entries or settings assert an explicit unavailable result and capture no body. Header snapshot is an allowlist. Authorization values are never recorded.

#### R1 exit

Grant paths used by ordinary, batch, nested, graph, explorer, planner, and reviewer children are tested. Allowed work succeeds. The mandatory path, worktree, and environment negatives fail closed. The installed skill works without its source tree. Catalog coverage has no silent skip. Focused tests, `cargo fmt` check, strict Clippy on the touched targets, one `./scripts/verify.sh`, native command-sandbox checks, and the installed terminal proof pass. Report R1 as source-tested, installed, and locally demonstrated. External providers and real-model quality stay unverified.

### Release 2 — Destination proxy, credential operations, revocation, cache and compaction

Goal: `network: true` no longer grants unrestricted connectivity or host credentials. Typed operations publish intent before effect. Revocation has termination evidence. Cache and checkpoint transitions are measured on loopback requests.

#### R2.1 References

Host permission and admission state owns destination definitions: protocol, canonical hostname, port, permitted address classes, and whether opaque tunnelling is allowed. Credential operations add the credential reference and the permitted operation. Local and private endpoints remain available only when that definition is explicitly approved. Provider authentication and `AgentClient` stay in the host and are not placed in the child grant.

#### R2.2 Proxy

`network_broker.rs` starts a per-worker HTTP/CONNECT proxy. The child OS profile allows only that endpoint. On macOS, Seatbelt allows outbound TCP to that localhost port and denies other network, including UDP. Remove `append_credential_rules` from the child network path. Replace `network_allowed_profile_grants_keychain_agent_and_vcs_credentials` with an assertion that those credential rules are absent.

The proxy checks the destination reference, resolves names, checks every returned address against the address class, and connects to the vetted address. It does not resolve the hostname a second time for the connect. It rejects other protocols, ports, and classes. Redirects are new admissions. Revocation closes the affected streams and tunnels. Opaque HTTPS is destination-level only; do not claim visibility into URL paths or bodies.

Tests must include direct TCP, UDP, an alternate socket family, ignored `http_proxy` settings, IPv4 and IPv6, and a DNS or address change between check and connect. A rule that only names the proxy port is not enough.

#### R2.3 Typed operations

Handlers for `http_request`, `git_operation`, and `ssh_operation` pin the executable and argument vector. HTTP takes method, path, body, and non-authentication headers. Git takes repository, remote, operation, ref, and path. SSH takes user, host, port, and the exact admitted command. Host-owned configuration disables hooks, external helpers and protocols, credential helpers, filters, signing commands, forwarding, and alternate SSH commands. Repository config is not authority.

Wrap each external effect in the existing `RunStore` sequence: `prepare_action`, `begin_action` and execute only when it returns permission, perform the operation, `confirm_action` with the observed result, and `reconcile_action` when publication is uncertain. Do not replay. This sequencing lands in R2; R4 only enriches the record.

#### R2.4 Revocation

A policy-generation change invalidates queued admissions and approval tickets, refuses new work on the old grant, closes affected tunnels, and cancels owned jobs, workers, and descendants. Wait until cleanup is confirmed. A cancellation request is not confirmation. Unconfirmed cleanup stays visible and blocks affected work. Do not signal unrelated production processes.

Race tests: queued approval, active tunnel, background descendant, and nested spawn.

#### R2.5 Cache and compaction

Using `PromptAssembly`, `PromptSegmentLifetime`, and `PromptCacheIntent`, drive two tool rounds, a second user turn, a substantial skill activation, compaction, and continuation through the loopback provider. Compare the physical serialized prefix and layout with the cache identity. Report the first unexpected divergence.

Route tests: selected-model switch, active fallback, and primary restoration, each with a smaller and a larger known window. Inspect `context_limits`, `active_turn_route`, checkpoint admission, and message-count suppression before changing them. Required behavior:

- Input and output headroom come from the effective route, not only `current_model`.
- Suppression follows that route and does not reset the global retry limit.
- A checkpoint request already known to exceed the route is rejected before dispatch.
- Failure keeps the previous checkpoint and canonical history.
- A result from a stale generation or owner is not activated.
- Short-history controls do not add a checkpoint request.

Extend the five existing cache and checkpoint tests listed above. Add the mandatory context negatives from the handoff.

#### R2 exit

Native macOS tests show proxy bypass and unapproved credential access fail, and an approved typed operation succeeds once. Revocation records confirmed termination. Cache and checkpoint cases complete through loopback. Same focused, Clippy, full-gate, install, and terminal bar as R1. Report R2 separately. Do not claim cache savings or external connectivity.

### Release 3 — Confined workers, truthful backends, confirmation, hint removal, offline map comparison

Goal: macOS isolated workers are real OS processes whose ready handshake is checked before any model-selected command. New adoption paths require confirmation. Removal trials prove exposure. The direct-versus-map runner is local mechanism evidence only.

#### R3.1 Pre-Tokio launcher

Split `main` so a synchronous outer entry inspects argv before `#[tokio::main]`. The hidden worker flag with inherited host channels and a valid `WorkerLaunchSpec` enters `isolated_worker`. The flag alone exits in error and creates no authority. Ordinary CLI parse errors stay as they are when the flag is absent.

`WorkerLaunchSpec` contains version, worker, owner, and lineage ids, launch nonce, policy generation, expected executable and grant digests, backend identity, required capabilities, private directory, scoped channel references, and admitted limits. It contains no secrets, user config, session, or sibling state.

Apply the OS confinement before the worker runtime starts and before any model-selected activity. The ready handshake reports identities and effective capabilities. The host checks them and only then dispatches. Reuse the bounded framing and cancellation style in `harness_worker.rs` (`MAX_FRAME`, length prefix). Provider streaming stays asynchronous and host-accounted. Child commands do not inherit broker descriptors (`FD_CLOEXEC` and a cleared child environment).

Backend failure or a mismatched ready identity runs no model-selected command.

#### R3.2 Backends

`BackendCapabilities` reports filesystem, network, process protection, descendant cleanup, and aggregate resource guarantees as separate booleans. Host token, runtime, and output limits stay host-owned. A per-process limit is not an aggregate tree guarantee.

- macOS keeps `sandbox-exec` / Seatbelt in `src/sandbox.rs`. Qualify the new worker path with native tests. This is the only platform that may flip the absent setting to isolated in this program, and only after those tests pass on the release artifact.
- FreeBSD keeps the authenticated root-owned helper and the current capability checks in `crates/ca-automation/src/freebsd_sandbox.rs`. Do not enable the new default there.
- Linux, only under `cfg(target_os = "linux")`, depends on `nono` 0.81.0 at revision `e9796cca01eb863a9a83e96c18ae6120aa50beb1` with `default-features = false`. Pin that git revision in `Cargo.toml` and `Cargo.lock`. Do not compile nono into the macOS release. `NetworkMode` defaults to `AllowAll`, and `ProxyOnly` / `localhost_ports` are port filters. Landlock does not bind a destination address. nono’s destination checks live in a seccomp-notify supervisor that reads sockaddr from process memory. Do not approve a suspended socket syscall after re-reading that memory. Set network mode explicitly. Deny unmediated UDP, raw, and other families with a static filter. Destination admission remains the R2 proxy, which connects to the vetted address. If a required guarantee cannot be enforced that way, report the capability unsupported and fail explicit `isolated` on Linux. Do not turn on isolated-by-default for Linux or FreeBSD in this delivery.

#### R3.3 Confirmation phase

Do not weaken `HarnessPlan::validate`. Add a separate frozen confirmation specification and phase in `harness_confirmation.rs`, with pure paired-trial predicates extracted for reuse. Development plans still require a target, two held-out cases, and three repetitions.

For newly frozen cycles:

- Reserve two unused cases from that family’s existing four-case pool.
- Keep confirmation payloads out of guidance and source-generation inputs.
- Include phase identity in attempt journal keys.
- Consume the cases durably before the first dispatch.
- Run three counterbalanced paired repetitions.
- Require independent development success and confirmation success.
- At adoption, recheck candidate, source, route, model, options, tool schema, and edit-contract identities.

Keep the current acceptance thresholds in `evaluate_harness`. Behavioral improvement still needs the repeated-pass gain. Equivalent verified behavior may still use the existing efficiency predicate. Confirmation must not open a quieter path.

Gate cycle decision (`HarnessCycle::decide`), activation preparation, source promotion, and guidance transition. A change only in `decide` fails the alternate-path test.

Recovery stays:

- A completed validated receipt is republished locally and is not rerun.
- A started attempt without a reliable outcome enters recovery.
- Dispatched or uncertain cases stay consumed.
- Only an unexposed reservation can be released after development rejection.
- Exhaustion waits and is visible; it does not invent cases.

Old `ca.harness-cycle.v1` records remain readable under their recorded rules. New cycles use an explicit confirmation-capable schema. Add serialization and digest compatibility tests.

Negatives: reused case, exhausted pool, phase-key collision, stale identity, missing receipt, and adoption through activation, promotion, or guidance when decision was not confirmed.

#### R3.4 Hint removal

Replace `single_addition` with a classifier: unchanged, exactly one addition, or exactly one removal. Reject swaps, multiple edits, and a hint change mixed with a prompt rewrite or a source edit. Update `harness_generation.rs` and `harness_trials.rs`.

Exposure is symmetric and comes from the attempt’s exposure artifact. Addition: the candidate receives the hint and the baseline does not. Removal: the baseline receives it and the candidate does not. Missing exposure blocks adoption even when the metrics would pass. Removal applies only to existing host-authored experimental hints and guidance. It cannot remove user instructions, privacy controls, permissions, or budgets.

#### R3.5 Offline direct-versus-map runner

Add a companion runner beside `tools/hrs317_qualify.py` that uses `tools/verify_runtime.py` terminal and provider primitives and the applicable HRS-317 metric helpers. Do not call `run_one()`. That function uses the configured real route. Do not inherit its environment.

The runner uses an allowlisted disposable environment and the same release binary in both lanes. The only difference is automatic task breakdown. Keep the official direct-path gate and HRS-317’s original 60-trial criteria for any future live qualification; this offline runner does not satisfy or replace that live gate.

Exercise a small fix, dependent multi-file work, independent branches with a failure, and a warm follow-up of at least 192 messages. Drive real revision and approval controls. Check resulting files from outside the agent. Label the report mechanism and overhead evidence.

#### R3 exit

macOS worker confinement and handshake pass on the release artifact. Explicit isolation does not fall back. New adoption paths require confirmation. A removal trial proves exposure and a missing artifact blocks adoption. The comparison report is labelled local mechanism evidence. Same install bar. Linux and FreeBSD remain unqualified for the new default.

### Release 4 — Security journal, one restricted-flow rule, skill lineage, documentation

Goal: an effect that the fixture server observed is not repeated when local publication fails. Restricted data cannot race an unrestricted send. Skill outcomes keep the digest that was loaded. The four operational summaries become current and small, and the archived history stays reachable.

#### R4.1 Journal

Reuse `Prepared → Running → Confirmed/Unknown`. Do not add another effect state machine. `security_journal.rs` appends version, sequence, previous record and segment digests, action and lineage identity, operation digest, grant, policy, and backend identities, authorization, intent, observed outcome, and reconciliation references. Storage is private, bounded, and serialized. Records hold references and metadata, not secrets or transcript bodies. Unresolved actions survive rollover.

If intent publication fails, do not dispatch. If dispatch may have happened, leave the action unknown until the same local publication or an authoritative readback resolves it. The durability fixture observes one server-side effect, fails the local outcome write, and asserts the effect count stays one. Reconciliation may confirm that single effect. It may not perform it again.

#### R4.2 Restricted flow

One optional human rule maps restricted path scopes to approved destination references. There is no policy language.

- Label native data before it is returned.
- If a shell read scope intersects a restricted path, label the command result restricted.
- A worker that can read a restricted file directly gets the same conservative label.
- Labels follow requests, outputs, handoffs, forks, and resumes.
- Label changes and outbound admission share one serialization point so a concurrent read cannot send unrestricted.
- Missing historical provenance is restricted.
- An incompatible parent operation is refused. Do not pour restricted context into an unrestricted lineage, and do not silently change main-agent settings.

#### R4.3 Skill lineage

Attribute invocation and outcome with `SkillLoad.content_digest` and the invocation identity. Do not call `inspect_instance` to discover the digest. Keep registry lookup only for display fields that were snapshotted at load time, or store those fields on the load.

Extend the learning event with optional correction, refinement-journal, invocation-group, loaded-digest, and host-verification reference, status, and scope. Old version-1 ledger files still load. New fields are optional and absent on old events.

Compute the first post-refinement reuse inside the ledger’s existing locked read-modify-write in `append`. Do not compute it before `append`. Publish lineage only after the mutation commits. If publication is missing, recover it from the matching committed journal. A prepared journal that existing recovery rolls back is not a successful refinement.

Completion stays distinct from verified success. Missing host verification remains unverified. Show the links in the existing learning view.

Paired fixtures: an original skill and a corrected skill, checked by independent artifacts. Report lineage and workflow evidence, not a general learning benefit. Negatives: mutation during invocation, two refinements, concurrent first reuse, missing lineage publication, and missing host verification.

#### R4.4 Documentation

The four operational summaries are `CONTINUITY.md`, `ARCHITECTURE.md`, `TESTING.md`, and `NEXT_STEPS.md`. They are currently about 600 KiB, 203 KiB, 594 KiB, and 360 KiB. Rewrite each as a current summary. Their combined size must be at most 96 KiB. Move historical sections into `docs/archive/` without deleting evidence. Correct links in `docs/.INDEX.md` and the summaries.

`PARITY.md` stays the parity contract and is not folded into the 96 KiB cap. `docs/.INDEX.md` remains the first lookup.

A two-read check must find each of these: installed version (`CONTINUITY.md`), active task (`NEXT_STEPS.md`), applicable checks (`TESTING.md`), source owner (`ARCHITECTURE.md`), and known gaps (`CONTINUITY.md` or `NEXT_STEPS.md`). The first read is the index.

Update the summaries with what R1–R4 actually shipped, the macOS qualification boundary, and the Linux/FreeBSD default still being the existing path. Do not describe scripted results as real-provider gains.

#### R4 exit

The durability fixture shows one effect. The restricted-flow race is denied. Skill outcomes retain the loaded digest. Archived evidence is reachable from the index. `wc -c` of the four summaries is at most 98304. Same install bar. This is the last release of the program.

## Per-release verification

During implementation, for the selectors touched by that change:

```sh
python3 scripts/check-change.py \
  --selector <existing-or-new-test> \
  --selector <existing-or-new-test>
```

Add `--release` only when that command is producing the candidate the installer will use. Do not also start `./scripts/verify.sh` against the same target. Freeze delegated edits before making a verification copy.

At each stable milestone, after the single version bump:

1. Independent review of the combined release diff.
2. Focused behavioral tests, formatting, and strict Clippy for the affected targets.
3. `./scripts/verify.sh` once. Read the report and the logs. Timeout, missing execution, and infrastructure failure are not passes.
4. Companion native or terminal qualification against that exact artifact.
5. `tools/bug257_install.sh` and the readback above.

Small Rust edits need a failing test first where the behavior is observable. At most three automatic repairs for one failure. After each failed repair, write down what the new evidence changed in the diagnosis. Keep the failed evidence.

Every ten minutes of active implementation, report what works, what is installed, what is unverified, and the remaining release work.

## Stop rules

- Do not lower budgets, disable privacy, weaken verification, broaden grants, or change fixture criteria to obtain a pass.
- If a backend cannot enforce a requested guarantee, record the capability as unsupported and fail that isolated launch.
- If dispatch may have occurred, keep the unknown outcome. Do not replay it to repair a receipt.
- Do not mark a later release complete because an earlier interface compiles.
- Do not edit `oracles/`, `reference/`, or `ca-hrs-252`.
- Do not read or print API keys. Do not mutate production config or sessions. The standing install rule covers only the `ca` binary, with a recoverable copy.
- Finish the current release’s install and readback before editing the next release’s shared source.

## Report shape for each release

State one of: source-tested, installed, natively qualified, locally demonstrated, externally unverified. Name changed files, the tests and gate that actually ran, failures and blocked checks, the installed version and hash, and the next release id if work remains. R1 through R3 leave Linux and FreeBSD isolated-by-default externally unverified by design. None of the four releases claim a real-model or real-provider benefit.
