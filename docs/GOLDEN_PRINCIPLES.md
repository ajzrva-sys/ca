# Golden Principles

1. Observable behavior beats implementation resemblance.
   - Prove behavior with deterministic `ca` fixtures. Oracle source stays read-only, and only when a task asks for that comparison.

2. One tool call, one decision, one result.
   - Every provider tool-call ID must resolve exactly once, including denial and execution failure.

3. The TUI must stay alive while work runs.
   - Rendering/input remain responsive; long work belongs in asynchronous orchestration.

4. State machines are explicit.
   - Composer modes, overlays, approvals, streaming, cancellation, and tool batches have named states and precedence.

5. UTF-8 boundaries are invariants.
   - Byte storage is allowed; invalid cursor boundaries and terminal-cell assumptions are not.

6. Side effects sit behind policy.
   - Classify, approve, snapshot where applicable, execute, report. Unknown actions take the safer path.

7. Headless and interactive modes share protocol truth.
   - Separate adapters may exist, but message ordering and tool semantics cannot drift.

8. Deterministic tests precede broad claims.
   - Network credentials and user runtime files are not regression infrastructure.

9. Oracles are read-only evidence.
   - Cursor defines parity evidence; Codex offers transferable testing/architecture lessons. Neither is production input.

10. Documentation is an executable map.
    - Source changes that invalidate ownership, commands, parity, or handoff state include the corresponding harness update.

11. Fail visibly; never invent success.
    - Provider refusal, malformed protocol data, missing tools, blocked checks, and uncertainty must remain observable.

12. Small verified slices beat “full parity” checklists.
    - Close one characterized behavior end-to-end, then update the matrix.

13. Complex work has one living ExecPlan.
    - Evidence: root `.agent/PLANS.md` was missing from the published snapshot. ISO-337, FLT-321, and `docs/exec-plans/active/BUG-provider-timeout-recovery.md` each recorded that absence. LRN-150 and SEC-158 already use one section list.
    - Apply: new multi-file or parity work starts from `.agent/PLANS.md` and is linked from `NEXT_STEPS.md`.
    - Future enforcement: GRD-010 in `docs/QUALITY.md`.
    - Exception: one owner and a known focused test stay a `NEXT_STEPS.md` entry. `crates/codex-team/` uses its own plan standard.
