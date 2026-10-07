# OPT-152 — Hosted API Context Efficiency

Status: COMPLETE
Owner/session: Codex / current workspace
Last updated: 2026-08-22

## Goal

Reduce avoidable hosted-model input and round trips while making cache and
latency outcomes measurable, without weakening tool, approval, session, or
provider-protocol boundaries.

## Non-Goals

- No concurrent mutation, process, MCP, interaction, or approval execution.
- No hidden provider KV-cache claim, tokenizer/network request, vector index,
  model-generated repository summary cache, or automatic model-tier rerouting.
- No provider/session schema migration, runtime configuration mutation,
  installed-binary replacement, oracle/reference access, or parity claim.
- No change to the default bounded command-result contract. Model-facing
  semantic condensation is enabled only by the existing explicit `semantic`
  command-output policy.

## Evidence and Current Behavior

- `src/request.rs::PromptAssembly` and
  `src/agent/client.rs::build_segmented_messages` already separate session,
  append-only, logical-turn, and request-only material. PRV-151 freezes the
  prompt epoch and tool catalog through continuations.
- `src/context.rs` already projects validated checkpoints and
  `src/memory.rs` performs deterministic query-aware bounded recall. The
  checkpoint prompt does not currently name facts and hypotheses separately.
- `src/command_output.rs` already produces conservative Git/Cargo summaries,
  but OUT-125 deliberately keeps them human-only; the model always receives
  the bounded raw result even when the user selected `semantic`.
- `src/usage.rs` records input/output/cache token subsets, cost, total request
  duration, and generation duration. It does not surface cache-hit ratio, new
  input, average first-output latency, or tool-continuation round trips.
- `src/request.rs::RequestSnapshot` exposes category estimates but not the
  declared stable-prefix boundary or its estimated size.
- Inferred: total request duration minus measured generation duration is a
  conservative end-to-end latency to the first model output observed by the
  interactive stream. It must not be labelled network-only TTFT.

## Scope

Allowed files: `src/{main,context,command_output,usage,request,headless,service}.rs`,
focused tests/snapshots, this plan, and reconciled harness documents.

Forbidden files/areas: `oracles/`, `reference/`, credentials, runtime config
or sessions, branches/worktrees/stashes, installed binaries, and unrelated
active-plan implementation.

Runtime state affected: process-only request/usage observations and the
existing `agent.command_output = "semantic"` behavior. The default remains
`bounded`.

## Design and Invariants

- The stable system prefix gains deterministic guidance only: target symbols
  and narrow ranges first, batch independent reads/searches in one model turn,
  and request condensed error context before raw logs. No time, request ID, or
  turn-specific value enters the prefix.
- Checkpoints use the canonical sections Task, Constraints, Facts,
  Hypotheses, Files Touched, Decisions, Test State, and Open Issues. The prompt
  explicitly forbids promoting uncertainty to fact.
- `semantic` may replace an eligible command's model-visible result with the
  existing conservative summary. The bounded raw capture stays process-local
  for human inspection/rerun; unsupported or ambiguous commands and the
  default `bounded` policy remain byte-identical.
- `/request` reports only the declared stable-prefix message count and a local
  estimate. It never implies a provider cache hit.
- `/usage` derives new input and cache-hit ratio only when cache-read detail is
  complete. It labels total and first-output latency honestly and reports tool
  continuations as provider round trips.

## Milestones

### M1 — Characterize the missing contracts

- Add failing tests for canonical checkpoint headings/uncertainty, stable
  prefix request inspection, truthful efficiency totals, and semantic
  model-result selection.
- Validation: focused `context`, `request`, `command_output`, and `usage`
  tests.

### M2 — Implement prompt and output efficiency

- Add deterministic retrieval/batching guidance to the session prefix.
- Update checkpoint construction without changing checkpoint persistence.
- Route eligible command results through the explicit semantic policy across
  interactive, headless, and service continuations while keeping events and
  raw process captures truthful.

### M3 — Implement efficiency observability

- Add stable-prefix estimates to `/request`.
- Add exact-known cache ratio/new input, average total/first-output latency,
  and tool-round-trip counts to `/usage` and sitting summaries.
- Review deterministic wide/narrow snapshots affected by the new report.

### M4 — Validate and reconcile

- Run focused checks, then the minimum Rust completion gates in `TESTING.md`.
- Reconcile `CONTINUITY.md`, `NEXT_STEPS.md`, `ARCHITECTURE.md`, `TESTING.md`,
  and `docs/.INDEX.md`; archive this plan on completion.

## Acceptance Criteria

- Stable instructions contain no volatile values and explicitly encourage one
  model turn to request multiple independent targeted reads/searches.
- A checkpoint request separates verified facts from hypotheses and preserves
  unresolved uncertainty under the exact canonical headings.
- Bounded policy output is unchanged. Semantic policy condenses only a proven
  supported command and retains raw process-local evidence for human use.
- `/request` shows the stable-prefix boundary without revealing its private
  scope ID or claiming a hit.
- `/usage` reports new input/cache ratio only from complete provider detail,
  distinguishes total from first-output latency, and counts tool continuation
  requests without treating local tools as provider calls.

## Validation

Focused: PASS. The exact system-prefix contract, canonical checkpoint prompt,
stable-prefix request report, semantic model-result selection, efficiency
totals, command-output presentation, usage reports, provider-account reports,
and session report all pass their focused unit/snapshot checks. The module
suites pass with 27 context, 10 request, 27 usage, and 6 command-output tests.

Completion: PASS. `cargo test --no-fail-fast --quiet` passed 1,731 main tests
and 38 CLI tests with one intentional ignore. The all-target/all-feature run
passed 1,760 main tests, 118 experiment tests, and 38 CLI tests with two
intentional ignores. Formatting, strict all-target/all-feature Clippy, the
all-feature release build, release help/version (`ca 0.1.0`), `git diff
--check`, snapshot hygiene, and oracle/reference worktree status all pass.

Manual/visual: PASS for deterministic reviewed wide/narrow command-output,
usage, provider-account usage, and session-report snapshots.

Blocked checks: no live provider traffic, credentials, runtime-state mutation,
installed-binary replacement, or oracle/reference access is authorized.

## Progress

- [x] Existing prompt, context, output, request, and usage boundaries audited.
- [x] Failing focused characterizations recorded.
- [x] Prompt/checkpoint and semantic-result behavior implemented.
- [x] Request/usage efficiency observations implemented.
- [x] Completion gates and harness reconciliation complete.

## Decisions

- Decision: reuse the explicit semantic policy instead of changing the default.
  Evidence: OUT-125 intentionally made semantic presentation opt-in and
  preserved bounded canonical results.
  Consequence: existing default behavior and old configurations remain stable.
- Decision: report first observed model output, not provider TTFT.
  Evidence: the current usage clock begins before local preparation and the
  stream records its first reasoning/text/tool delta.
  Consequence: the label remains truthful across providers and reasoning modes.
- Decision: defer actual parallel tool execution.
  Evidence: interactive batches combine preflight, ordered approval, mutations,
  MCP, process jobs, and interaction requests.
  Consequence: this slice reduces provider round trips through batching
  guidance but does not reorder execution authority.

## Surprises and Discoveries

- Most of the requested stable-prefix, compaction, retrieval, cost, and
  planner/executor foundations already exist in completed PRV-151, CTX-123,
  REL-147, OUT-125, and RND-095 work.

## Recovery and Rollback

Remove the added derived observations and prompt text, restore the old
checkpoint headings, and route semantic command policy back to human-only
presentation. No stored session or configuration rewrite is required.

## Completion Record

Completed 2026-08-22. The deterministic stable prefix now directs targeted,
symbol-first, independently batched read/search requests without volatile
content. Checkpoint generation separates facts from hypotheses and preserves
open uncertainty. Existing explicit semantic command-output mode now also
reduces eligible model-visible Git/Cargo results in interactive, headless, and
service continuations while keeping bounded mode unchanged and same-process
raw evidence expandable by the user.

`/request` exposes only the declared stable-prefix boundary and local estimate,
with an explicit no-hit disclaimer. `/usage` and `/session` add exact-known new
input/cache-hit observations, tool-continuation round trips, and honest total
versus first-observed-output latency. No stored schema or default changes.

Actual concurrent tool execution remains outside this plan because safe
parallelism requires a separate ordering contract across approvals, mutations,
process jobs, MCP calls, and interaction tools. No provider request,
credential access, runtime-state mutation, installed-binary replacement, or
oracle/reference content access occurred.
