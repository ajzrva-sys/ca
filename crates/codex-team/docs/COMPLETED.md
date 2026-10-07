# Completed Tasks

Moved from `NEXT_STEPS.md` on 2026-07-20. All tasks below have Status: Done.

## PRT-230: Exclude Finder Metadata from Protected Trees
- Exact `.DS_Store` path components are excluded before protected-tree counts
  and hashing under `sha256-merkle-v2-ds-store-excluded`; similarly named
  paths remain protected.
- Failure-first unit and real CLI regressions cover root/nested Finder metadata
  and the similar-name control. Protected unit tests pass 6/6, CLI tests 3/3,
  and the full package passes 652/1 ignored with MCP stdio 8/8.
- The separately authorized manifest-only commit establishes HEAD trust. The
  protected verifier passes with no status, diff, or mutation under `oracles/`
  or `reference/`.

## TST-217: Eliminate Broker Cache-Publication Collisions
- Broker atomic-publication names now combine the process ID, one captured
  nanosecond timestamp, and a dedicated monotonic sequence. Reservation uses
  private mode-0600 `create_new` files and retries at most 16 candidates only
  for `AlreadyExists`.
- A writer cleans up only the temporary file it successfully reserved. Foreign
  collisions are preserved, while the existing write/fsync/rename/parent-fsync
  sequence and atomic last-writer-wins cache semantics remain unchanged.
- Four fixed-clock tests cover concurrent distinct reservations and complete
  same-target payloads, preserved foreign collisions, fail-closed exhaustion,
  and owned-file cleanup. All 11 broker tests, three repeated shared-query
  regressions, 36 MCP unit tests, and the eight-test stdio suite passed.
- Both serial workspace matrices, formatting, strict all-feature Clippy, and
  the all-feature release build passed. No public API, schema, runtime state,
  provider traffic, or installed binary changed.

## TST-216: Deterministic Shared-Evidence Cancellation Barrier
- Replaced the five-client evidence fixture's fixed eight-second build sleep
  with disposable, Git-ignored marker barriers.
- Both evidence phases now require active job status to report five consumers
  before release; the cancellation phase stays deterministically active until
  the last consumer cancels it.
- Production broker and executor behavior is unchanged. The repaired case
  passed four consecutive focused runs and every subsequent broad invocation.
- The full package attempt separately reproduced the pre-existing concurrent
  cache-publication `EEXIST` race in the shared-query case; its isolated rerun
  and one complete eight-test SDK rerun passed. TST-217 subsequently repaired
  that race and passed both complete workspace matrices.

## ORCH-TRIAD-DOC-001: Align docs to Hermes×Rust×MLX triad
- HERMES.md + bridge describe dual MLX lanes, cost ladder, Hermes skill ownership
- In-repo skills labeled contract exports; no longer forbid Hermes→MLX
- Follow-on protocol — ask Hermes what skill gaps remain; Rust answers with facts only

## Follow-on (human / Hermes session)
- Hermes session rewrote living `~/.hermes/skills/codex-team` + `codex-team-boss` with triad, cost ladder, dual MLX, and accomplishments catalog pointer
- Rust/Cursor supplied facts only (no unsolicited living-skill rewrite)

## ORCH-RUST-DET-002: Expand Rust deterministic lanes (token sink)
- Docs-only deterministic lane accepts `git status --short` and `git diff --stat`
- `README.md` counts as a docs-verify envelope path
- Command-only evidence may use docs-verify paths when wording stays verify/evidence (not edit)

## ORCH-MLX-DOGFOOD-001: Operator Hermes×MLX disposable dogfood
- Disposable repo `~/tmp/codex-team-mlx-dogfood-001`; doctor/providers healthy
- Rust stamps Worker `task_id`; Guard blocks `done` when claimed files are missing
- `--json` stdout purity for verification runner progress
- Documented chat-only write gap (Rust→MLX Worker cannot edit files)

## ORCH-MLX-WRITE-001: Toolful local-edit lane (follow-on)
- Preferred edit lane = **Hermes → MLX (tools/skills)** on this Codex-free machine
- Rust→MLX roles documented as chat/JSON / learning only (no worktree writes)
- Hard rule recorded — do not trust Worker `files_changed` without disk evidence
- Deferred: Rust patch-apply ExecPlan

## ORCH-TRIAD-BRIDGE-001: Sync boss bridge with edit-routing decision
- `docs/HERMES_BOSS_BRIDGE.md` includes the edit-routing table / hard rule

## ORCH-LOCAL-MLX-001: No-force Codex / MLX-only roles
- All roles `provider=mlx_lm` in local config
- Doctor skips Codex check when no role uses Codex
- `step`/`run`/`watch-loop` only call `validate_codex_binary` when a role uses Codex

## ORCH-RUST-LEARN-002: Supervisor-visible learning + negative memory
- Additive `learning` on cycle/status JSON receipts
- Negative route learnings (`outcome: failed`) weight scorecards down
- `[routing].honor_scorecard_soft_gate` default false; pauses on Worker cooldown when enabled
- `hermes_supervisor_loop.sh` stops on `outcome=error` with `error_kind`

## ORCH-RUST-LEARN-001: Supervisor episodes + docs-only Rust lane
- `supervisor_episodes.jsonl` error outcomes synthesize into tool_health cooldowns
- Docs-only mechanical verify tasks are deterministic-lane candidates
- Scorecards boost RustEvidence for docs-only verified profiles

## ORCH-HERMES-003: Emit JSON receipts on hard step failures (+ roadmap pack)
- `step --json` emits `outcome: "error"` receipt on hard failures
- Local JSON hydrate + doctor model presence for mlx/ollama
- Empty-queue idle stop without Planner churn
- Supervisor episodes + `providers --json` + `[routing]` knobs
- `docs/HERMES_MLX_DOGFOOD.md`

## ORCH-HERMES-002: Hermes Supervisor Ready Pack
- Installable `skills/codex-team` + `scripts/install_hermes_skill.sh`
- Documented exit codes; `doctor --json` exits 1 when unhealthy
- `docs/HERMES.md` setup block + `docs/HERMES_INTEGRATION_CHECKLIST.md`
- Operator Hermes `custom:ollama` provider recognised

## ORCH-MLX-001: Rust-Owned Apple MLX LM Role Runner
- `provider = "mlx_lm"` + `[mlx_lm] enabled` / `base_url` (default `http://127.0.0.1:8080`)
- OpenAI-compatible chat adapter + `/v1/models` health probe; fail closed when disabled
- Doctor/models/Hermes docs cover MLX

## ORCH-OLLAMA-001: Rust-Owned Ollama Role Runner
- Per-role `provider` (`codex`|`ollama`) with default `codex`; `[ollama] enabled` / `base_url` config
- Ollama chat completions adapter with run artifacts + telemetry; fail closed when disabled
- Provider-neutral `run_role` dispatches on provider; doctor reports Ollama health when enabled

## ORCH-HERMES-001: Harden Hermes CLI Bridge
- Under `--json`, stdout is exactly one JSON object; human progress goes to stderr
- Typed cycle/status/disposition receipts include additive `pause` and `cycle_id`
- `disposition --json` and `doctor --json` emit machine receipts
- `status --json --observe` loads state without queue recover/save mutation

## ORCH-LESSONS-001: Adopt Others Reliability And Routing Lessons
- Tool-health cooldowns are advisory and visible in status/watch/cost
- Route scorecards and positive route learning use existing `ExecutionLane` values
- `codex-team route-bench` writes local benchmark reports without calling Codex
- Derived status includes `no_signal`
- Risky Worker-bound tasks without characterization evidence stop before Worker

## ORCH-WATCH-008: Auto-Gate PM Pauses During Orchestrator Repair
- PM-created explicit safety-gate pauses can be auto-approved for orchestrator repair cycles
- Dirty-adoption pauses remain non-auto-gateable and still stop

## ORCH-ROLE-COMMIT-001: Add Role-Authorized Rust Commit Requests
- PM `commit_requests` commit only explicitly selected allowed dirty files
- Senior `rust_commit_job` progress actions use the same Rust commit executor
- Failed verification or disallowed direct git commands create no commit
- Prompts and docs say AI roles do not run `git` directly

## ORCH-LEARN-001: Add Repo Lock And Atomic IO Hardening
- `step`, `run`, and `watch-loop` acquire `.orchestrator/codex-team.lock` before loading config or state
- Lock acquisition refuses live PID locks, recovers dead PID locks, refuses corrupt locks
- App write helpers route through atomic text writes and bounded JSONL appends in `src/fs_util.rs`
- `watch-loop` deduplicates target/orchestrator repo locks

## ORCH-LEARN-002: Evaluate Remaining Mutating Command Locks
- Lock policy documented in CONTINUITY/ORCH-INTEGRITY-001
- Focused live-lock refusal coverage retained for `step`
- `status` remains state-refreshing under lock; `cost`/`doctor` stay advisory/read-oriented without lock

## ORCH-ROUTER-001: Add Route Cost Controls
- Route regret records redundant Worker read/search exploration
- Route regret reports include redundant exploration count, lane savings, fingerprint evidence
- `codex-team cost` prints redundant exploration and lane-savings totals
- Scanner deduplicates by task plus redundant command fingerprint

## CHURN-001: Add Worker Context Pack Receipts And Attempt Capsules
- `ContextPackBuilder` with named sections, priorities, byte/token caps, fallback summaries
- Every Worker attempt writes a prompt receipt
- Over-budget prompts are rejected before launching Codex
- Retry/escalation context includes Attempt Capsule; omits duplicate logs/diffs

## CHURN-002: Add Context Delta Mode And Prompt Linter
- Worker retries use Attempt Capsule delta mode via `prior_content_hashes` / `verification_improved`
- Prompt lint rejects transcript markers, duplicate section headers, full diffs, unordered file contents
- Same-fingerprint retries without verification improvement stop before another Worker call

## CHURN-003: Add Repo Hint Cache And File Lease Context
- `src/context/lease.rs` owns `FileLeaseSet`, stale-tolerant `RepoHintCache`, leased file rendering
- Worker prompts embed leased file contents only; default embeds none
- Worker schema `expand_lease` asks Rust for bounded additional leases

## LOOP-001: Add Supervised Watch Loop Automation
- `watch-loop` runs a bounded target step + watcher/self-heal loop
- Unresolved target alerts can be recorded as external efficiency feedback in an orchestrator repo
- Orchestrator repo is marked Planner-due and can run a bounded repair cycle
- Loop reports persisted under `.orchestrator/observations/efficiency_loop.*`

## MOD-001: Extract Verification Runner Contract
- `src/verification/runner.rs` owns the command execution boundary
- `src/app.rs` calls the runner module API and no longer owns command execution mechanics

## MOD-001B: Extract Verification Plan and Evidence Contracts
- `VerificationPlan` / `EvidenceReport` / `CommandEvidence` extracted
- `Config` and `State` live in `src/config.rs` / `src/state.rs`; `find_git_root` in `src/git.rs`

## MOD-002: Move Codex Runner Seam Into Adapter Module
- `CodexRunner`, `CodexRunRequest`, `RealCodexRunner` live in `src/codex/run.rs`
- `src/codex/stream.rs` owns pure JSONL parser helpers
- `step_once_with_runner` allows fake-runner tests without launching `codex`

## GRD-001: Propose a CI Guardrail for Core Validation
- GitHub Actions workflow `.github/workflows/ci.yml` runs fmt, clippy (advisory), test, build
- Clippy is advisory (not `-D warnings`) until existing crate lints are cleaned up

## ORCH-HEL-002: Add Mechanical Parity Preflight Guard
- Renderer/campaign tasks without a cited seven-field preflight are blocked before Worker
- Non-renderer tasks continue through existing routing
- Implemented in the Helicon readiness guardrails slice

## ORCH-HEL-003: Add Non-Interactive Disposition And Adoption Report Classification
- `codex-team disposition --repo <path> --decision preserve|recheck|gate|keep-paused`
- Dirty adoption reports classify `user_baseline`, `orchestrator_init`, and `generated_or_ignored` entries

## ORCH-HEL-004: Refresh Legacy Dirty Adoption Pause Reports
- `codex-team disposition --repo <path> --decision keep-paused --refresh-report`
- Added `legacy_unknown` dirty adoption report classification

## ORCH-HEL-005: Preserve Disposition Suppresses Dirty Continuation
- Preserved dirty baselines do not generate, review, persist, append, queue, or run `DIRTY-*` work
- Stale approved `DIRTY-*` sections are ignored under preserve
- Renderer/campaign dirty candidates without preflight fail before Senior review

## RECONCILE-1782766991147 - Reconcile PM-Identified Control Docs
- Reconciled `CONTINUITY.md` and `ROADMAP.md` to describe preserved dirty baseline
- Rust auto-accepted verified

## RECONCILE-1782773249690 - Reconcile PM-Identified Control Docs
- Worker skipped; Rust ran command-only evidence receipt
- Status: passed

## ORCH-DELEGATION-FABRIC-003: Mechanistic Delegation Fabric
- Added one versioned Rust contract for bounded tracked-repository queries,
  typed isolated evidence, and contained local process/copy observation.
- Added exact `observe | delegate | operate` MCP tiers with 11/15/28 tools,
  durable jobs, revision binding, categorical feedback, and zero resources.
- Added additive task delegation policies, bounded query context, eligible
  Rust-only Worker skips, and explicit `prefer`/`require` fallback semantics.
- Added inert digest-bound learned `DELEGATION.json` manifests with shadow,
  exact human approval, drift, quarantine, retirement, and correction controls;
  stored scripts and executable members never gain authority.
- Final workspace, formatting, strict lint, all-feature release, compatibility,
  snapshot, schema, and production isolation checks passed. Fail-closed dirty
  repository, ignored protected-root baseline, and overlapping stale root MCP
  fixture conditions are recorded in the completed ExecPlan and `TESTING.md`.
