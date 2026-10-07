# AUT-231 Resume Bundle

Frozen 2026-08-29 after the driving agent (Codex) was shut down mid-flight.
This bundle preserves the in-progress work so it can be resumed without loss.

## State at freeze

- Plan: `docs/exec-plans/active/AUT-231-codex-style-local-automations.md`
  (status moved to PAUSED).
- The working tree compiles cleanly:
  `cargo check --bin ca` → exit 0, 27.4s, with only two dead-code warnings
  (`AutomationStore::claim_due` never used, `AutomationExecution::successful_tools`
  never read) — both from the not-yet-wired M2/M3 surface.
- No tests or completion gates have been run for AUT-231. M1 was never marked
  complete; M2–M5 untouched.

## What changed (post-baseline, vs the recorded dirty-tree baseline 25c936f)

The AUT-231 baseline (`AUT-231-dirty-baseline.status.gz.b64`, SHA-256
`6e90af2cbece9cbf25df5faca2ed7a8418acd888c4027a4936bd473b3c68b2b9`) recorded
only git status, not file content. The delta is therefore reconstructed from
mtime and symbol grep, not from a content diff. Files touched after the
22:24:44 baseline:

1. **`src/automation.rs`** — NEW, 2668 lines (copy saved here as
   `src-automation.rs`). M1 contracts/stores: `AutomationDefinitionV1`,
   `AutomationRecordV1`, `AutomationStore`, `AutomationClaimV1`,
   `AutomationRunStateV1`, `AutomationUsageCountersV1`, `AutomationRunEventV1`,
   `AutomationBudgetV1`, pricing binding, notification policy, exact digests,
   plus inline `#[test]` unit tests.
2. **`src/schedule.rs`** — added `pub(crate) struct AutomationExecution`
   (line ~1516) and a workspace/recovery preparation path that reads
   `crate::automation::AutomationDefinitionV1` (line ~1527).
3. **`src/session.rs`** — added `AutomationSessionProvenanceV1` (line ~436),
   `SessionAutomationBinding` (line ~661), `SessionAutomationLease` /
   `SessionAutomationLeaseOwner` (line ~1311), the
   `automation_provenance` / `automation_deliveries` fields on SessionV2, and
   the delivery/lease acquisition + journal-append path (line ~918–978).
4. **`src/main.rs`** — CLI wiring:
   - `mod automation;` (line 8)
   - `LaunchArgs::Automation(cli) => automation::run_cli(cli).await` (line 149)
   - `automation <COMMAND>` entry in the `after_help` string (line 860)
   - `Automation(automation::AutomationCliArgs)` enum variant (line 1202)
   - `if command == Some("automation") { ... automation::parse_args(...) }`
     dispatch (lines 1397–1402)
5. **`src/app.rs`** — touched at 22:46:14; no new `automation` symbol found
   (likely a partial/format-only edit). Review before trusting it as real work.

## How to resume

1. Reopen `docs/exec-plans/active/AUT-231-codex-style-local-automations.md`,
   set `Status:` back to ACTIVE, and continue from the M1 checkbox
   (contracts/stores exist but are not yet failure-first characterized).
2. If `src/automation.rs` is ever missing, restore it from
   `docs/exec-plans/active/AUT-231-resume/src-automation.rs`.
3. Run the plan's focused validation (automation stores/coordinator, scheduler
   compatibility, session lease/delivery crash matrix, fake provider/pricing/
   budget, CLI, tools, TUI) and the completion gates listed in the plan.
4. Remember the plan's hard constraint: never reset or clean the dirty
   repository; reverse only AUT-231-owned hunks if a rollback is needed.
