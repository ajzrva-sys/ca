# QUALITY

## Quality Bar

- Preserve Rust ownership of state transitions, evidence, guard checks, command execution, and commits.
- Keep AI roles advisory unless Rust converts output into typed state,
  evidence, or role-authorized commit requests.
- Keep parser, verification, guard, Codex adapter, and role boundaries separated as modules are extracted.
- Prefer focused tests beside the extracted module plus one full `cargo test` before claiming source behavior is safe.
- Do not add shell execution paths that bypass typed command validation.

## Existing Mechanical Checks

- Rust unit tests under `src/app_tests.rs` and module tests under `src/queue/next_steps.rs`.
- Allowlist-style command parsing rejects shell control operators; evidence in tests around legacy recipe parsing and deterministic command policy.
- Guard snapshots compare Worker file changes against allowed files.

## Known Gaps

- `src/app.rs` remains large and owns too many subsystem details.
- Verification plan construction, evidence persistence orchestration, and live Codex terminal rendering still have most behavior in `src/app.rs`.
- The Codex runner seam and production `codex exec` spawn path now live in `src/codex/run.rs`; keep new role-call process work behind that adapter boundary.
- Clippy still warns under `-D warnings`; CI currently runs clippy without treating warnings as errors.

## Proposed Guardrails

### GRD-005: Test Children Must Be Kill-on-Drop

- Invariant: a test that spawns a long-lived process must install an exact
  kill-and-wait owner immediately after spawn; failed setup and assertions may
  not orphan the child.
- Evidence: the MCP restart fixture's `TestProcessReaper` and its explicit
  early-exit regression test.
- Why it matters: dropped `std::process::Child` handles do not terminate the
  process and can silently consume CPU after a failed test.
- Enforcement: focused regression plus post-matrix process scan during local
  process-supervision changes.
- Suggested failure message: `Long-lived test child lacks unconditional
  kill-and-wait ownership.`
- Priority: High.
- Related task: completed by the MCP restart fixture cleanup.

### GRD-001: Root Workspace Validation

- Invariant: source changes should pass `cargo test` and formatting check.
- Evidence: the parent repository's `.github/workflows/ci.yml` pins Rust and
  action revisions and validates both workspace packages with locked/offline
  format, strict Clippy, tests, release builds, smokes, and hygiene checks.
- Why it matters: weaker agents need a single objective completion gate.
- Enforcement: GitHub Actions on push/PR to `master` or `main` after the root
  integration is tracked; local commands remain authoritative until then.
- Suggested failure message: `Run cargo test and cargo fmt -- --check before merging source changes.`
- Priority: Medium.
- Related task: `GRD-001` in `NEXT_STEPS.md` (Done).

### GRD-002: Keep Queue Parser Pure

- Invariant: `src/queue/next_steps.rs` must not call Codex, run commands, read files, write files, or mutate orchestration state.
- Evidence: module docs and extracted parser API.
- Why it matters: parser behavior can be tested without runtime side effects.
- Enforcement: review checklist now; future structural test or lint.
- Suggested failure message: `Queue parser code must stay pure; move IO/state behavior to app or queue IO helpers.`
- Priority: High.
- Related task: future task after more extraction.

### GRD-003: No AI Intern Runtime

- Invariant: legacy Intern config may deserialize, but no Intern role is active at runtime.
- Evidence: README and `legacy_intern_support_contract` tests.
- Why it matters: support packets must remain deterministic and cheap.
- Enforcement: tests asserting no `run_codex(..., "intern", ...)` path and no generated Intern prompt/schema.
- Suggested failure message: `AI Intern runtime path reintroduced; use Rust support packets instead.`
- Priority: High.
- Related task: none currently.

### GRD-004: One Spawn Point for Codex

- Invariant: model process launches should go through the Codex runner seam.
- Evidence: fake-runner test and `src/codex/README.md`.
- Why it matters: routing tests need to avoid real `codex` subprocesses.
- Enforcement: module extraction plus test coverage.
- Suggested failure message: `Do not spawn codex outside the adapter seam.`
- Priority: Medium.
- Related task: `MOD-002` in `NEXT_STEPS.md`.
