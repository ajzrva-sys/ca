# HED-056 — Stronger Headless Contracts

Status: COMPLETE
Owner/session: `/root`
Last updated: 2026-08-11

## Goal

Add an explicit fresh-context headless verification pass and bounded locally validated structured output while preserving the byte/shape/exit behavior of ordinary `ca.headless.v1` runs.

## Non-Goals

- Changing ordinary headless approval defaults, existing exit codes, positional prompt or `@file` parsing, provider protocol serialization, TUI `/review`, or session behavior.
- Automatically repairing work under `--check`, silently granting mutation/network authority, accepting remote schema references, generating Rust types from user schemas, or promising semantic correctness beyond one model verification report.
- Replacing `ca.headless.v1`, changing its event variants, or claiming Ante/Cursor runtime parity.

## Evidence and Current Behavior

- `src/headless.rs` owns one injectable tool loop and immutable `ca.headless.v1` envelope/output types. Existing exits are 2 input, 3 setup, 4 approval, 5 transport, 6 protocol, 7 maximum turns, 8 internal, and 130 interruption.
- A successful terminal assistant response is written atomically when requested, emitted as `Completion`, and immediately ends the runner. There is no post-task phase.
- `src/review.rs::run_review` is the closest proven fresh-context pattern: a separate system/user request, an allowlisted read-only tool set, and a sole-final report tool validated locally.
- `jsonschema` Draft 2020-12 validation is already used by the tool registry. No additional validator dependency is required.
- Ante documents `--check` as a second pass in `oracles/ante-main/docs-site/docs/usage/headless.mdx`, but its documented pass may complete or optimize work. HED-056 deliberately makes verification read-only unless a separate repair flag is supplied. This is product evidence only, not runtime observation or Cursor parity.
- Inferred: new lifecycle phases cannot be added to `HeadlessEventV1` without breaking the public enum/schema contract, so any run using new contracts must opt into an explicit `ca.headless.v2` envelope while ordinary runs remain v1.

## Scope

Allowed files:

- new `src/headless_contract.rs`
- `src/{main,headless}.rs`, focused CLI/unit tests, and only documentation/snapshots required by this plan
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, `docs/.INDEX.md`, and this ExecPlan

Forbidden files/areas:

- `oracles/`, `reference/`, TUI/session/provider protocols, real config/Keychain/session state, installed binary, network services other than loopback fixtures, and later protocol/policy plans
- broad tool-policy changes, implicit yolo, remote `$ref` retrieval, arbitrary schema code execution, or mutation during verify-only checks

Runtime state affected:

- Both features are process/CLI-only. Schema documents are read locally before provider traffic and never persisted. Verification reports and structured values are emitted only to requested headless output surfaces.

## Design and Invariants

- Add `--check` and `--check-repair` (`--check-repair` requires `--check`). Verify-only uses a fresh two-message context containing bounded original request and terminal result plus project instructions, exposes only existing read-only main tools and one `report_verification` tool, and does not inherit main conversation/tool results.
- `report_verification` is the sole final tool call. It returns a bounded typed report with `passed`, summary, and bounded issues/evidence. Missing, duplicate, mixed, malformed, or oversized reports are provider-protocol failures. A valid `passed: false` report exits 9 after emitting the report; a passing report exits 0.
- `--check-repair` may expose the already selected main tools during the check and execute through the exact existing approval, permission, workspace, sandbox, and network boundaries. It never implies yolo/network access. After any repair tool calls, a sole-final report is still required. The report records whether repair was requested and whether tools actually ran.
- Add `--output-schema PATH`. Load one regular UTF-8 file before provider traffic with strict byte/depth/node/property bounds, a root object schema, and no remote or fragment-external references. Compile Draft 2020-12 locally.
- Structured-output runs advertise normal selected tools plus one reserved `report_structured_output` function whose argument schema wraps the user's root schema under `value`. The report call must be the sole final call; its value is validated again locally and bounded after serialization. It is never executed as a side effect.
- `--output-schema` changes terminal success from free text to the validated JSON value. Text prints compact JSON; JSON/JSONL emit a typed v2 structured-output event/result. `--output-last-message` writes the same compact JSON. Natural-language terminal output without the report tool fails protocol validation.
- Runs using `--check`, `--check-repair`, or `--output-schema` emit `ca.headless.v2`. V2 carries existing lifecycle semantics plus explicit check-start/report and structured-output events. Ordinary runs, including failures before provider traffic when no new flag is present, retain exact `ca.headless.v1` types and schema.
- Check and structured-output tools use reserved names and are appended only after central tool selection. A selected/configured/MCP tool with either reserved name fails before provider traffic. Tool exposure remains separate from execution authority.
- All new input/report/value sizes are bounded, terminal text is sanitized, structured JSON stays semantic JSON, output writes retain broken-pipe behavior, and no prompt/schema/provider body enters failure output beyond safe local validation messages.

## Milestones

### M1 — Characterize v1 and add failing contract tests

- Freeze ordinary text/JSON/JSONL envelopes and existing exits.
- Add CLI validation tests for flag relationships, positional ordering, schema preflight, and zero-provider-traffic failures.
- Add fake-backend failures for missing/mixed/duplicate/malformed report calls and verify-only mutation attempts.
- Validation: focused tests fail only for missing HED-056 behavior.

### M2 — Implement bounded schema and v2 output contracts

- Add schema loader/compiler/bounds, reserved report tool, local result validation, V2 events/output, and output-last-message integration.
- Route structured-output runs through the existing headless tool loop without changing ordinary V1 emission.
- Validation: unit and loopback CLI fixtures prove valid/invalid structured values, bounds, no remote refs, sole-final enforcement, and unchanged v1 snapshots.

### M3 — Implement fresh-context verification and explicit repair

- Run a second fresh-context pass after main success, with read-only constraints by default and ordinary selected tools only under explicit repair.
- Emit bounded typed reports, exit 9 for a valid failed verification, preserve existing transport/protocol/approval exits, and prove repair uses normal authority.
- Validation: fake/loopback suites prove no mutation under `--check`, explicit repair gating, fresh context, report determinism, and no duplicate main output write.

### M4 — Complete gates and isolated walkthrough

- Run focused suites, full tests, formatting, strict Clippy, release build, help/version/profile/flag-order smoke, and zero-pending-snapshot checks.
- With a disposable home/workspace and loopback provider, exercise ordinary v1, passing/failing verify-only checks, explicit repair approval, valid/invalid schema output, and zero-I/O schema rejection.
- Reconcile harness documents, archive this plan, and activate PRV-057 only after every gate passes.

## Acceptance Criteria

- No-new-flag text, JSON, JSONL, exit-code, last-message, approval, and broken-pipe behavior remains `ca.headless.v1` and passes its existing tests unchanged.
- `--check` runs only after main success, receives a fresh bounded context, exposes no mutating tools, requires one locally valid sole-final report, exits 0 on pass and 9 on verified failure, and never edits the workspace.
- `--check-repair` is rejected without `--check`; with it, repair calls traverse unchanged selection, permission, workspace, sandbox, and network checks and are visibly reported.
- `--output-schema` rejects unsafe/oversized/malformed/non-object/remote-reference schemas before provider traffic, requires one sole-final report call, locally validates the returned value, and writes/emits exactly that bounded JSON value.
- New phases use only `ca.headless.v2`; ordinary v1 serialized fixtures are byte/shape stable.
- Focused/full gates and isolated walkthrough pass without touching real runtime state, oracle files, external providers, or the installed binary.

## Validation

Focused:

- PASS: seven CLI/schema/tool-support contract tests plus 23 headless unit tests cover parsing, bounds, local references, fresh context, verify-only restrictions, explicit repair authority, sole-final calls, local value validation, v2 emission, and unchanged v1 behavior.
- PASS: two loopback CLI integration tests cover pass/fail verification, exact exit 9, structured output/last-message, local schema preflight, and zero provider traffic on invalid references.

Completion:

- `cargo test --no-fail-fast`: PASS, 667 unit tests plus 28 CLI integration tests.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- Rebuilt help/version/flag-conflict smoke and zero-pending-snapshot check: PASS.

Manual/visual:

- Headless-only plan: no TUI snapshots were required because no TUI surface changed.
- PASS: disposable release-binary loopback walkthrough covered ordinary v1, passing and failing v2 checks, repair refusal and explicit authorization, valid structured output/atomic last-message, and invalid-schema zero-I/O rejection.

Blocked checks:

- None currently.

## Progress

- [x] Inspected current CLI parsing, v1 emitter/exits, central tool loop, local schema validator dependency, and fresh-review sole-final pattern.
- [x] Reviewed Ante's documented `--check` behavior as non-parity product evidence.
- [x] Created the active ExecPlan before source implementation.
- [x] Add failing v1/schema/check characterizations.
- [x] Implement v2 structured output.
- [x] Implement verify-only check and explicit repair.
- [x] Pass focused and repository-wide gates.
- [x] Complete isolated walkthrough and harness reconciliation.

## Decisions

- Decision: reserve exit 9 for a valid verification report whose `passed` value is false.
  Evidence: existing public exits occupy 2–8 and 130; a semantic check failure is neither transport/protocol nor task-run failure.
  Consequence: automation can distinguish “task ran, verification failed” without parsing prose.
- Decision: preserve v1 by switching opt-in runs wholesale to v2 rather than mixing envelope schemas in one stream.
  Evidence: adding variants to the serialized v1 event enum would silently expand a claimed stable contract.
  Consequence: ordinary consumers are unchanged; consumers requesting new flags explicitly accept v2.
- Decision: model structured output as a sole-final tool call and validate its argument locally.
  Evidence: the existing review runner already proves this control-flow shape, while free-text JSON cannot enforce completion or exclude trailing prose.
  Consequence: providers must support tools for `--output-schema`; incompatible models fail clearly before or during the request.
- Decision: verify-only check uses read-only tools, while repair is a separate flag that does not grant authority.
  Evidence: the program explicitly forbids implicit repairs and existing headless permission controls are already deterministic.
  Consequence: `--check-repair` can still exit 4 when the user's approval policy does not authorize a proposed mutation.

## Surprises and Discoveries

- `jsonschema` is already a direct dependency and the registry already compiles Draft 2020-12 schemas, avoiding a new dependency/security surface.
- The current runner owns emission and tool execution in one function, so preserving v1 is easiest with a small contract-aware emitter/phase extraction rather than modifying `HeadlessEventV1`.
- Wrapping a user schema under the report tool's `value` property changes the JSON Pointer root. Local fragment references are rewritten only in the advertised wrapper while local validation continues against the untouched original schema.
- Opt-in flags without a prompt would otherwise force headless mode and block reading an attached terminal indefinitely; they now fail locally with guidance unless a prompt or piped stdin is present.

## Recovery and Rollback

- Keep ordinary v1 tests green throughout and isolate v2/schema/check types in `headless_contract.rs` so rollback is patch-local.
- Never remove or rename a v1 field/event. If v2 cannot be cleanly separated, stop rather than weakening the compatibility requirement.
- Track loopback/TUI-free walkthrough processes by exact PID and move only exact temporary roots to Trash.

## Completion Record

Changed files: `src/headless_contract.rs`, `src/{headless,main,request}.rs`, `tests/cli.rs`, and the repository harness documents listed in Scope.

Commands and results: focused contract/headless/loopback suites passed; `cargo test --no-fail-fast` passed 667 unit plus 28 CLI tests; formatting, strict Clippy, release build, help/version/conflict smoke, zero-pending-snapshot check, and the isolated loopback walkthrough all passed.

Remaining risks: verification remains one bounded model judgment, not a proof of semantic correctness. Schema mode requires tool-capable providers. Explicit repair can still make authorized changes, so it deliberately retains the caller's approval, permission, sandbox, workspace, and network policy.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, and `docs/.INDEX.md` now record the v1/v2 boundary, exit 9, local schema rules, validation evidence, and PRV-057 handoff.
