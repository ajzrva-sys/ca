# AGT-270 — Explicit cross-provider subagent control

Status: COMPLETE — validated and recoverably installed
Date: 2026-10-10

## Goal and baseline

Let any configured parent provider, including DeepSeek, choose and control a
child on a different configured provider, including OpenAI and native Codex.
The baseline runners support exact routes and refresh Codex authentication at dispatch,
but the model-visible task/spawn schemas only offer automatic cost routing.
This excludes native subscription routes from per-task selection. Child status
responses also omit the actual provider/model.

The checkout contains extensive existing work, including AGT-269. Preserve all
of it and its retained failures. The referenced `.agent/PLANS.md` is absent;
this plan follows the existing repository plan format.

## Scope and contracts

- Add optional `route: {provider, model}` to task, spawn and batch items. Exact
  configured routes bypass automatic pricing selection; omission preserves the
  existing inherited/fixed/learned behavior.
- Reject unknown routes, malformed route values and route/complexity conflicts
  before child admission or provider traffic. Batch admission remains atomic.
- Offer secret-free sorted `list_agent_routes` metadata. Configured inventory
  does not promise authenticated or currently available inference.
- Preserve provider-neutral await, list, message and cancel controls, frozen
  grants and workspace scope. Show the actual route in child snapshots.
- Keep normal runtime credential resolution and protocol adapters. Tests use
  injected auth fixtures or credentialless loopback providers in disposable state.
- A managerless legacy task path must not silently ignore an explicit route.
- In the subsequent economic routing assessment, fix Learned allowed-route
  filtering before cost ranking so a cheaper disallowed route cannot reject a
  qualifying permitted route. Preserve pricing eligibility, capability floors
  and explicit route pins.

## Acceptance and progress

- [x] Failure-first schema/catalog, route/admission and lifecycle tests.
- [x] Actual CLI DeepSeek parent to another provider/protocol, returned child
      findings, route attribution and cross-provider steering/cancellation.
- [x] Independent review and full `scripts/verify.sh` report/evaluation.
- [x] Standing background release, recoverable signed installation and readback.
- [x] Update user guide, continuity, queue and validation evidence.

No live paid inference, provider setup, user configuration/session change or
learning-service refresh is part of this task. Existing running sessions stay
running. Fixture evidence establishes CA routing/control; live provider access
still depends on the configured account/endpoint.

## Focused evidence and independent review

The initial 46 subagent tests pass, including exact DeepSeek-to-Codex selection,
runtime credential resolution/failure, nesting, batch atomicity, limits, grants
and control. The managerless route case passes separately. Registry (14), tool
selection (21) and lazy discovery (8) pass. Independent core/surface/CLI review
found no blockers; artifacts and task-specific patches are retained under
`scratch/cross-provider-subagents/schema/` and
`scratch/AGT-269-cross-provider/`.

Three CLI tests exercise four real credentialless runs: asynchronous control
(10 parent Chat Completions / 3 child Responses requests), foreground task
(4 / 1), unknown provider/model rejection (5 / 0), and conflicting route/cost
selection (2 / 0, expected protocol exit 6). They check actual child file reads,
steering, active cancellation, route attribution, exact grants, no rejected
admission and no child traffic. The unchanged final fixture fails against the
prior signed installed CA `a244af5a…` because discovery is absent, with zero
child requests. Raw evidence: `scratch/cross-provider-cli/candidate-2/` and
`baseline-final/`; failed initial fixtures remain retained.

Initial new-test corrections are preserved: a temporary borrow failed compile;
the builtin catalog intentionally grows from 91 to 92; the existing early
managerless-task guard already rejects the call, so the redundant lower guard
was removed and its regression asserts the existing exact error. New CLI
expectations were corrected to the existing Explore grants and to distinguish
tool failures from terminal schema rejection. No established behavior assertion,
deadline, snapshot or verifier criterion was weakened. The exact catalog-count
adjustment is independently reviewed as the new tool's contract.

The initial full workflow is retained under `scratch/verify/AGT-270-final/`, bound to
source `229b073bcc1908a8223b7a59bfff8c726220c51c6e78ea49c7804b7da3f80d23`
(1,126 inputs) and unchanged criteria
`01030516d7e82208d2c70c1f6aa37f6f1c68be6f97ad30c526a82aed36e4de3c`.
Preflight, workflow contracts, formatting and strict all-target/all-feature lint
passed. The default matrix caught the remaining prepared-request schema count
at 48 versus expected 47. Its intentional catalog expectation is corrected from
46 to 47 (plus privilege capability), and `list_agent_routes` is added to the
existing required-schema assertions. All previous identity, credential and bare
omission checks remain. A before-image is retained in
`scratch/cross-provider-subagents/before/main.rs`; independent review confirms
the two-line change matches exactly one new advertised tool.

The owned verification run was interrupted after the deterministic count failure
to avoid duplicating the known failure in later stages. Its report and evaluator
remain `infrastructure_failed`, incomplete, and not install evidence. The same
source maps before/after contain 1,126 identical entries. Two unchanged cleanup
probes were also marked failed near interruption without their target's complete
failure diagnostics; these are retained and require focused and fresh complete
matrix evidence. Complete matrices, release/runtime evaluation and install are
pending under a new output rather than rewriting the initial receipt.

The user's economic-selection assessment also exposed a narrow existing bug:
Learned selection ranked every candidate, then rejected the globally cheapest
route outside its allowlist even when an allowed route qualified. A new focused
manager test reproduces that exact error. The candidate now applies the exact
allowed-route predicate before the existing capability/forecast/comparator.
Unrestricted selection, configured pricing/billing/freshness/credential
eligibility, no-candidate failure and explicit pin precedence remain intact.
Three new tests cover permitted selection, capability/no-candidate handling,
unrestricted/empty selection and exact pins. Independent source review found no
issues; before-images, red log and task-specific patch are retained under
`scratch/cross-provider-subagents/allowlist/`. Corrected regressions pass 3/3,
pricing passes 23 with one existing downloaded-source-fixture ignore, and the
manager module passes 49/49. The corrected prepared-request regression passes
1/1, and all three unchanged cleanup/cancel/timeout probes pass separately.
These focused results do not substitute for complete matrices.

Fresh complete verification passes under `scratch/verify/AGT-270-corrected/`
(background session 92999, exit 0), bound to source
`d4d021af9912ea83ed24d15b8e96b701f040b17b15986c2b79e4f3f0f4039b95`
(1,126 inputs). Source and tests remained frozen through the complete report and
independent evaluator. The immutable CLI fixture harness is retained at
`scratch/cross-provider-cli/frozen-harness`, SHA-256
`fa796e2e82ce9e77a9ea900fe40f9c57b0f85e60ad430fdec33ae26d2781ad28`.
The full gate rebuilt its unchanged source; the same retained harness was
replayed against the prior installation and fails all three tests as expected,
with zero child requests. Original baseline receipts remain unchanged.

The fresh preflight, 117 workflow contracts, formatting and strict
workspace/all-target/all-feature lint pass. Both full matrices pass.
`scratch/cross-provider-subagents/source-scope-preservation.json` independently
compares all compiled inputs against AGT-269's retained manifest: exactly the
seven intended source files differ, one CLI fixture is added, none is removed,
and all other existing compiled work is preserved.

The broader task-first economic decomposition, effort/context and reusable local
guidance design is separately recorded in AGT-271. Codex-team retains ownership
of mechanical non-AI execution. No automatic economic pipeline or live savings
is claimed by this narrow fix.

## Final acceptance and installation

The complete corrected gate and independently reconciled raw evidence pass:
3,977 default cases across all 29 targets and 4,133 all-feature cases across all
30 targets, zero failures or outer filtered cases, and 43 / 44 existing ignores.
All 117 workflow contracts, formatting, strict lint, all-feature workspace
release and nine signed runtime cases pass. The evaluator has zero issues.
Before/after/current source manifests contain the same 1,126 inputs and digest
above; criteria remain unchanged. Independent review verifies every stage log,
all declared targets, 18 runtime artifact hashes, actual tool/reviewer readback,
current-turn report delivery and parent completion/terminal restoration.
Evidence: `scratch/cross-provider-subagents/independent-gate-review.json`.
The three unchanged cleanup/cancellation/timeout probes pass in both complete
matrices. Earlier failed/interrupted receipts remain unchanged.

The identical immutable CLI harness passes all three tests/four scenarios on
both the signed candidate and the installed executable. Each replay observes
21 parent Chat Completions and four child Responses requests; invalid route and
conflicting selection cases produce zero child requests. Actual file findings,
steering, await/list route attribution, active cancellation, exact grants and
no rejected admission are independently reconciled. Raw receipts, logs and
before/after hashes are under `scratch/cross-provider-cli/signed-candidate/`
and `installed/`, with the respective `*-post.json` assessments. The original
and immutable-harness baseline failures against the prior release are retained.
The final independent installed replay/documentation binding passes 55 checks
with zero issues: `scratch/cross-provider-subagents/installed-replay-independent-review.json`.

Background release/install session 12010 exits 0. The signed rebuilt candidate,
validated candidate and actual `/Users/aji/.local/bin/ca` all have SHA-256
`c95b495804d2986cfce965d61e0fe8c2a079d1d8027d81c8c03437b527695400`.
Installed mode is 0755, owner UID 501/GID 20; signature, version `ca 0.1.0`, help
and current source binding pass. The prior signed release is recoverable at
`/Users/aji/.local/bin/.ca-release-stage.LIqhXq/ca.recovery-ORC258-20261010T140912Z-a244af5a738c8a00b985dda34b1d7c7cc1f15a7ffdf3298dcbfd6a691b1a8c1b`.
Its independently checked bytes match the prior `a244af5a…` baseline.
Installation evidence: `scratch/cross-provider-subagents/release-install.log`,
`install-transaction.log`, `installed-readback.json` and
`installed-independent-review.json` (33 independent checks, zero issues). No settings, sessions,
running processes or learning-service enrollment were changed. Start a new CA
process to load this release. No paid/live account inference or measured savings
is established by the credentialless acceptance.

Changed compiled inputs are `src/agent/{registry,subagent,tools}.rs`,
`src/{main,pricing,tool_discovery,tool_selection}.rs` and the added
`tests/cross_provider_subagents_cli.rs`. The user guide, configuration guidance,
index, continuity, queue and testing record document the final behavior.
AGT-271 is the next implementation task for the broader assessed economic policy;
codex-team retains mechanical execution ownership.
