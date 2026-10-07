# DIA-035 — Provider-Aware Doctor

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-10

## Goal

Add secret-safe offline diagnostics through `ca --doctor` and `/doctor`, with an explicit `--connect` option that reuses the existing bounded provider probe and reports text or JSON with meaningful exit status.

## Non-Goals

- Do not change provider configuration, defaults, sessions, or Keychain data.
- Do not make network requests unless `--connect` is supplied.
- Do not add the positional `ca doctor` form because it would break the existing positional-prompt contract.
- Do not claim Cursor or Ante parity.

## Evidence and Current Behavior

- `RawCliArgs` treats positional values as one-shot prompts, so a new positional subcommand would be incompatible (`src/main.rs`).
- `/config` currently opens only the config path, while `/provider setup` already owns explicit typed provider checks (`src/main.rs`, `src/provider_probe.rs`).
- Runtime paths are centralized in `src/runtime.rs`; safe provider summaries come from `ConfigStore`.
- Ante documents a no-TTY readiness command, but this is secondary descriptive evidence only (`oracles/ante-main/docs-site/docs/reference/cli-reference.mdx`).

## Scope

Allowed files: `src/doctor.rs`, `src/main.rs`, `src/tui/commands.rs`, focused tests/snapshots, and current harness/plan documents.
Forbidden files/areas: `oracles/`, `reference/`, user config, Keychain, saved sessions, installed binaries.
Runtime state affected: none; optional loopback provider traffic only when explicitly requested.

## Design and Invariants

- `ca --doctor` is offline and emits a deterministic text report; existing `--output-format json` emits one aggregate JSON object. JSONL is rejected for this one-shot report.
- `ca --doctor --connect` resolves credentials only inside the existing provider-probe boundary and checks providers sequentially. Reports contain provider/model IDs and typed outcomes only.
- `/doctor` opens the same offline report in the pager; it never connects automatically.
- Checks cover runtime directory/config accessibility, config parsing, current/default provider-model consistency, safe credential-source presence, configured catalog counts, sandbox availability, and a deterministic test-backend render.
- No resolved key, environment value, Keychain value, request URL, response body, or credential length enters report state or output.

## Milestones

### M1 — Characterization and report core
- Add failing CLI/command tests and a typed diagnostic report module.
- Validate offline execution with an isolated temporary home.

### M2 — CLI and TUI integration
- Add `--doctor`, guarded `--connect`, text/JSON formatting, exit mapping, and `/doctor` pager routing.
- Reuse provider probes for explicit connection checks without duplicating network logic.

### M3 — Completion
- Add render/command coverage, run completion gates, reconcile harness docs, and archive this plan.

## Acceptance Criteria

- Offline doctor performs zero provider requests and never creates or mutates runtime config.
- Missing/invalid config and inconsistent default selection are reported with nonzero status.
- Connect mode reports typed provider outcomes without secrets or response bodies.
- `/doctor` is discoverable and idle-only; its pager is readable at wide and narrow sizes.
- Existing positional prompts, `--list-models`, headless output, and provider setup remain unchanged.

## Validation

Focused: doctor module, command registry, CLI subprocess, and render snapshots.
Completion: `cargo test --no-fail-fast`, `cargo fmt --check`, strict Clippy, release build, help/version.
Manual: rebuilt binary with a temporary home; loopback only for connect mode.

## Progress

- [x] Repository/docs and implementation seams inspected.
- [x] Characterization tests added.
- [x] Report core implemented.
- [x] CLI and TUI integrated.
- [x] Focused and completion checks passed.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: use `--doctor`, not positional `doctor`.
  Evidence: positional arguments already mean one-shot prompts.
  Consequence: no CLI compatibility break.
- Decision: offline is the default; provider traffic requires `--connect`.
  Evidence: diagnostics must remain safe without credentials or network.
  Consequence: ordinary readiness checks are deterministic and free.

## Surprises and Discoveries

- None yet.

## Recovery and Rollback

Revert only the files named in Scope using targeted patches. No runtime state is written.

## Completion Record

Changed files: `src/doctor.rs`, CLI/TUI command integration, safe config snapshot, CLI/render/unit tests, and harness documents.
Commands and results: focused tests PASS; full 382 unit + 23 CLI PASS; format, strict Clippy, release, help/version PASS; isolated missing-config check PASS.
Remaining risks: connection checks intentionally remain sequential and can take the configured probe timeout per provider; they are never automatic.
Harness updates: `CONTINUITY.md`, `ARCHITECTURE.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` reconciled.
