# SCH-083 — Scheduled Tasks

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Add an explicit, disabled-by-default, profile-scoped schedule manager and runner for exact one-shot or cron prompt/workflow jobs, with pinned provider/model/workspace/runtime authority, deny-by-default tools/network, one process-wide run at a time, missed-invocation skipping, and delivery only to an exact local file or authenticated signed webhook.

## Non-Goals

- Automatic daemon installation/startup, background launch agents, catch-up replay, concurrent schedule runs, interactive approvals, session persistence, ambient skills/MCP/LSP/hooks/memory, messaging-platform delivery, arbitrary delivery commands, unsigned webhooks, embedded scripts, remote definition loading, GUI/TUI management, or editing user cron/launchd state.
- Cursor parity or a claim that Ante's documented scheduler was runtime-observed.

## Evidence and Current Behavior

- No schedule/cron/delivery store, command, runtime path, or runner exists. Leading `serve`/`attach` commands are explicitly routed before the ordinary positional parser in `src/main.rs`.
- `src/headless.rs::run_with_backend` already provides bounded prepared requests, central tool selection, typed tool continuations, output formats, cancellation, sandbox/network gates, and stable exits, but it has no process-grant field or scheduled request kind.
- `src/workflow.rs` already validates bounded local TOML/JSON DAGs, stable IDs/digests, exact dependency outputs, retries, evidence, tool lists, grants, and deterministic next-ready state. Production orchestration is TUI-owned, so scheduled workflow execution needs a narrow noninteractive adapter rather than a synthetic slash command.
- `RuntimePaths::profile_data_dir` provides the correct default/named profile isolation; bare mode must remain nonpersistent.
- Ante documents scheduled automation as a product idea. SCH-083 defines an independent local `ca` CLI/store/runner and makes no runtime-parity claim.

## Scope

Allowed files: new `src/schedule.rs`; narrow changes in `Cargo.{toml,lock}`, `src/{main,runtime,request,headless}.rs`; focused CLI/tests; schedule documentation; this plan and required harness documents.

Forbidden files/areas: oracle/reference trees; real cron/launchd; real user config/schedules/sessions/Keychain; installed binary; non-loopback or unsigned test delivery; TUI/accessibility; unrelated session/provider/config schemas.

Runtime state affected: profile-scoped schedule definitions/state below the injected data root, one exact profile lock, ephemeral child processes and job owners, configured local result files, and explicitly authorized signed webhook requests. Validation uses temporary homes/workspaces, fake clocks/executors, loopback providers/webhooks, and fake secrets only.

## Design and Invariants

### Commands and definitions

- Reserve leading `ca schedule` with `list`, `add <local.toml|json>`, `enable ID`, `disable ID`, `remove ID --confirm ID`, `run ID`, `tick`, and `daemon`. Every command accepts immutable `--profile`; bare refuses before creating paths. `run` is an explicit manual run; `tick` evaluates at most one due job; `daemon` repeatedly ticks until Ctrl+C and is never installed or started automatically.
- Add reads one strict bounded non-symlink local definition containing version, safe ID, one `once`/`cron` trigger, exact configured provider/model, canonical workspace, 1–86400-second runtime cap, 1–3600-second lateness grace, explicit tool/grant/network fields, one prompt or exact local workflow target, and one local-file or signed-webhook delivery. The stored record is always disabled initially.
- Workflow targets store the validated workflow ID, workspace-relative path, and SHA-256 digest captured at Add. Execution refuses drift. Every step route must equal the schedule pair; step tool/grant authority can only intersect the schedule's exact declared authority.

### Persistence, due state, and locking

- One versioned JSON store lives under `RuntimePaths::profile_data_dir(profile)/schedules/`, uses owner-only directories/files, strict bounded decode, comment-independent atomic replace, revisioning, duplicate rejection, and safe metadata-only listing. Definitions may contain prompts and therefore have redacted Debug and never enter logs/list output.
- One exact profile lock serializes add/edit/run/tick/daemon mutations and schedule execution. A dead-owner lock may be recovered only after verifying its recorded PID is absent; an active/unknown lock fails closed.
- Each enabled record has one `next_run_at`. A due claim is persisted before provider work: one-shot disables; cron advances directly to the first occurrence after current time. Jobs later than their configured grace are recorded `skipped` and advanced without execution. There is no replay loop and tick claims at most one job.

### Execution and authority

- Production freezes one validated run spec and sends it to an isolated current-binary worker over bounded stdin; prompts/grants never appear in argv or environment. The worker has no SessionStore, skills, MCP, LSP, hooks, memory, fallback, or interactive approvals. It builds `PreparedModelRequest::Scheduled` with the exact provider/model/workspace.
- Empty tools/grants and false network are the defaults. Model-visible tools are the exact declared built-in set; execution receives only validated process grants, while persistent Deny/Ask remains stronger. Network additionally needs both a schedule declaration and the invoking `--allow-network` upper bound. Unknown/dynamic/interaction/subagent tools fail before provider traffic.
- A runtime deadline terminates/reaps the exact worker process group. Stored run state keeps only safe typed timing/outcome/exit metadata; provider/tool/request content and generated text are delivered but not retained in the schedule store.
- Scheduled workflows execute deterministic ready steps through the same isolated worker, bounded dependency prompt rendering, exact evidence/retry logic, and one overall runtime deadline. No step can widen the schedule route, tools, grants, workspace, or network authority.

### Delivery

- Local delivery atomically replaces one exact regular path inside the scheduled workspace, rejects symlink components, and writes one bounded `ca.schedule.result.v1` envelope.
- Webhook delivery requires HTTPS or literal-loopback HTTP, no URL credentials/query/fragment, one validated environment-secret reference, no proxy/redirects, a bounded deadline/body, HMAC-SHA256 over exact bytes, and an explicit `--allow-webhooks` upper bound. Missing secret fails locally with zero requests. Response bodies are never read or retained.

## Milestones

### M1 — Store, validation, clock, and CLI characterization

- Add the active plan/claim and failing tests for strict definitions, profile paths, default-disabled state, cron/once next times, missed skipping, atomic claims, lock recovery, safe listings, and leading-command positional compatibility.

### M2 — Isolated prompt/workflow execution and delivery

- Add the process-grant/request-kind headless seam, bounded child worker, deterministic workflow adapter, local atomic delivery, and signed no-proxy webhook path with fake executor/clock/HTTP tests.

### M3 — Gates and disposable walkthrough

- Run focused/full/release gates. With a disposable home, exact one-shot plus cron definitions, credentialless loopback provider, and loopback signed webhook, prove disabled no-run, explicit enable/tick, missed skip, local/webhook delivery, no tools/network by default, no sessions, and clean worker/listener shutdown.

## Acceptance Criteria

- New schedules are inert until explicitly enabled or manually run; no command installs background services or mutates system scheduler state.
- One exact due invocation runs at a time, late occurrences skip without catch-up, claims survive crashes without duplicate replay, and all mutations/runs are profile-isolated and atomic.
- Prompt and drift-free workflow targets run with exact route/workspace/runtime/tool/grant/network authority through prepared headless requests and create no sessions.
- Deliveries are only symlink-safe local files or explicitly allowed signed webhooks; missing secret, unsafe URL, HTTP failure, timeout, and oversized output fail safely without body/secret leakage.

## Validation

Focused: PASS. Eight schedule-core tests cover strict identifiers and delivery authority, five-field cron normalization, direct future advancement without catch-up, profile isolation/bare refusal, owner-only atomic persistence, redacted Debug, and worker grant/workspace bounds. The released prompt integration covers default-disabled Add/List/Run behavior, exact route/request, bounded local output, and no sessions.

Completion: PASS. `cargo test --no-fail-fast` passed 1005 unit plus 31 CLI tests; `cargo fmt --check`, strict Clippy, and `cargo build --release` passed. Release help/version/schedule-help smokes passed.

Manual/visual: PASS. A released-binary disposable-home walkthrough added disabled one-shot/local and cron/webhook definitions, proved `tick` was inert while disabled, manually ran both through exactly two credentialless pinned requests with zero tools, validated the bounded local envelope and HMAC-verified loopback webhook envelope, found no session directory, cleanly stopped the fake listener, and moved the inspected fixture recoverably to `/Users/aji/.Trash/ca-sch083-walk.G6k2xP`.

Blocked checks: none known.

## Progress

- [x] Existing CLI/headless/workflow/runtime seams traced and SRV-082 archived.
- [x] Failing schedule store/trigger/lock/CLI tests added.
- [x] Isolated prompt/workflow execution and delivery implemented.
- [x] Focused/full gates and released walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: persist strict local definition files through `ca schedule add` rather than place prompts/grants/webhook metadata in shell arguments.
  Evidence: scheduled prompts and authority can be sensitive and are too structured for safe ad hoc flags.
  Consequence: argv/list output remain metadata-only and Add can validate the whole authority envelope before persistence.
- Decision: use a frozen stdin run spec and current-binary worker instead of mutating process cwd or reusing TUI state.
  Evidence: headless currently owns global stdio/current-cwd assumptions, while the scheduler must enforce deadlines, capture bounded results, and isolate crashes without process-global path races.
  Consequence: scheduled provider work reuses canonical headless boundaries while the parent retains exact claim/delivery ownership.
- Decision: tick claims at most one job and cron advancement jumps to the first future occurrence.
  Evidence: the roadmap forbids catch-up storms and defaults concurrency to one.
  Consequence: overdue work is visible as skipped, never burst-replayed.

## Surprises and Discoveries

- Headless permission evaluation already supports process grants in `PermissionMatcher`, but `HeadlessRunConfig` currently passes an empty slice to both built-in and MCP brokers. SCH-083 must expose a narrow injected grant field without adding a public grant CLI that could blur scheduled authority.

## Recovery and Rollback

Disable all records, stop the explicit daemon with Ctrl+C, and preserve the schedule JSON for inspection. Source rollback removes the schedule module/CLI/worker seam locally; no system scheduler or session migration exists. Test fixtures are disposable and move recoverably to Trash after inspection.

## Completion Record

Changed files: `Cargo.{toml,lock}`, `src/{main,runtime,request,headless,schedule}.rs`, `tests/cli.rs`, the active/completed plan, and required harness documents.

Commands and results: focused schedule tests PASS (8); released worker CLI integration PASS; full test PASS (1005 unit + 31 CLI); format PASS; strict Clippy PASS; release build/help/version/schedule smoke PASS; disposable loopback walkthrough PASS.

Remaining risks: `daemon` is intentionally a foreground poller and installs no OS service. Cron uses UTC semantics. Store records retain schedule prompts by design under owner-only profile data. Webhook allowlisting is invocation-scoped and secrets remain external environment references. No production HTTP/TLS service was contacted.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, and `docs/.INDEX.md` now document the completed extension and route the next plan to PLG-084.
