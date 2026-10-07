# AHE Application Plan for ca

> Applying Agentic Harness Engineering patterns to make ca's harness
> observability-driven and self-improving, adapted for a local Rust terminal
> agent with an existing rich self-improvement ecosystem.

---

## Executive Summary

AHE's core insight is: **the harness is the thing that evolves, not the model,
and every change must be evidence-backed, predicted, and verified.** ca already
has extensive self-improvement infrastructure (Dream/Distill, learning journeys,
ExecPlan system, quality guardrails, convergent review, codex-team
self-improvement). The primary gap is not capability but **structured
observability** — ca doesn't systematically know *whether its harness changes
actually improved outcomes*.

This plan proposes five concrete improvements, ordered by impact and dependency.
Each is designed to integrate with ca's existing architecture (Rust modules,
session JSON, Dream/Distill cycles, ExecPlan system, CONTINUITY.md).

---

## Gap Analysis: AHE vs. ca

### What ca already does well (no changes needed)

| AHE Concept | ca Equivalent | Status |
|---|---|---|
| Evidence-backed changes | ExecPlan system + CONTINUITY.md + QUALITY.md guardrails | Mature |
| Git-tracked audit trail | Git + ExecPlan active/completed + CONTINUITY.md entries | Mature |
| Component-level harness | AGENTS.md, skills, hooks, sub-agents, approval policy | Exists but not decomposed |
| Evaluation/frozen tests | QUALITY.md GRD-001–009a + `cargo test` + snapshots | Mature |
| Promotion decisions | codex-team `evaluation.rs` with `decide_promotion()` | Exists (learning artifacts) |
| Replay/re-evaluation | codex-team `learning_loop.rs` with frozen replay | Exists (learning artifacts) |
| Trace capture | `provider_trace.rs` + `raw_transcript.rs` | Exists |
| Session tool metrics | `session_metrics.rs` (per-tool success/failure counts) | Exists |

### What AHE adds that ca lacks

| AHE Concept | Gap in ca | Impact |
|---|---|---|
| **Session outcome telemetry** | No structured "did the user succeed?" signal | **Critical** — foundation for everything else |
| **Harness component decomposition** | Harness is monolithic markdown/docs, not versioned components | High — can't correlate changes with outcomes |
| **Trace distillation into failure reports** | Dream/Distill focus on memory/skills, not harness failure analysis | High — existing pipeline misses harness-level patterns |
| **Outcome-prediction verification loop** | ExecPlans have predictions but no automated verification | Medium — currently relies on human judgment |
| **Cross-session task stability tracking** | session_metrics is within-session only | Medium — can't detect recurring failure patterns |
| **Best-of-N parallel variant exploration** | No mechanism for A/B testing harness variants | Low — complex, high cost, limited local value |

---

## Priority 1: Session Outcome Telemetry (Foundation)

### Problem

ca tracks tool success/failure within sessions (`session_metrics.rs`) and has
provider traces, but it has no structured signal for **whether the user's goal
was achieved**. Without this, every other improvement is flying blind.

### Design

Add a `SessionOutcome` record to the session JSON format (SessionV4+) that is
populated automatically at session end and explicitly by user feedback.

```rust
/// src/session_outcome.rs — new file
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionOutcomeRecord {
    pub schema: String, // "ca.session_outcome.v1"
    pub session_id: Uuid,
    pub workspace_fingerprint: String, // SHA-256 of workspace root
    pub outcome: OutcomeKind,
    pub harness_snapshot: HarnessSnapshot,
    pub tool_summary: ToolSummary,
    pub user_signals: UserSignals,
    pub created_at: DateTime<Utc>,
    pub digest: String, // SHA-256 of the record
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutcomeKind {
    /// User explicitly accepted (e.g., /accept, clean exit)
    Accepted,
    /// User explicitly rejected or rolled back
    Rejected,
    /// User abandoned (switched tasks, long idle, new session without saving)
    Abandoned,
    /// Session ended normally without explicit signal
    Completed,
    /// Session hit an unrecoverable error
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSummary {
    pub total_calls: u64,
    pub completed: u64,
    pub failed: u64,
    pub denied: u64,     // approval denied
    pub retried: u64,    // same tool retried after failure
    pub tool_failure_rates: BTreeMap<String, f64>, // per-tool fail rate
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSignals {
    pub explicit_corrections: u64,  // times user said "no, that's wrong"
    pub retries_requested: u64,     // /retry count
    pub undos_used: u64,            // /undo count
    pub edits_after_tool: u64,      // user manually edited files ca touched
    pub session_duration_secs: u64,
    pub total_tokens: u64,
}
```

### HarnessSnapshot

This is the key AHE adaptation — a compact, content-hashed record of which
harness component versions were active during the session.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessSnapshot {
    pub agents_md_hash: String,      // SHA-256 of AGENTS.md
    pub system_prompt_hash: String,   // SHA-256 of effective system prompt
    pub tool_defs_hash: String,       // SHA-256 of tool definition JSON
    pub skills_hash: String,          // SHA-256 of active skill set
    pub approval_policy_hash: String, // SHA-256 of approval mode/rules
    pub memory_hash: String,          // SHA-256 of memory store state
    pub config_hash: String,          // SHA-256 of effective config
    pub quality_guardrails_hash: String, // SHA-256 of QUALITY.md
}
```

### Integration points

- **Session end**: `src/session.rs` → `save()` already runs at session end.
  Compute `SessionOutcomeRecord` and append it to the session JSON alongside
  the existing `ContextCheckpoint`.
- **User signals**: Wire into existing `/undo`, `/retry`, user correction
  detection (already exists in `src/interaction.rs`).
- **OutcomeKind detection**:
  - `Accepted`: user runs `/accept` or explicitly says "looks good"
  - `Rejected`: user runs `/undo` on ca's changes, or explicitly says "no"
  - `Abandoned`: new session via `/new` or idle timeout without saving
  - `Completed`: normal session end
  - `Failed`: provider error, tool loop exhaustion
- **Storage**: Append to session JSON (existing `~/.local/share/ca/sessions/`).
  Add `ca outcomes --recent N` CLI command and `/outcomes` slash command.
- **Privacy**: No raw transcript text, no tool arguments, no file contents.
  Only hashed fingerprints, counts, and durations.

### Implementation estimate

- New file: `src/session_outcome.rs` (~400 lines)
- Modify: `src/session.rs` (outcome computation at save time, ~80 lines)
- Modify: `src/interaction.rs` (correction signal counting, ~40 lines)
- Modify: `src/app.rs` (add outcome state to AppState, ~20 lines)
- Modify: `src/main.rs` (CLI subcommand, ~30 lines)
- Tests: ~200 lines focused coverage

---

## Priority 2: Harness Component Registry

### Problem

ca's harness is distributed across AGENTS.md, the system prompt builder,
tool definitions, skills, hooks, approval policy, memory, and config. These
are all independently mutable but there's no registry that tracks their
current content hashes or associates them with session outcomes.

### Design

Add a lightweight `HarnessRegistry` that computes and caches content hashes
of each harness component at session start.

```rust
/// src/harness_registry.rs — new file
pub struct HarnessRegistry {
    root: PathBuf, // ~/.local/share/ca/harness/
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessComponentRecord {
    pub component: HarnessComponent,
    pub content_hash: String,
    pub byte_length: u64,
    pub last_modified: Option<DateTime<Utc>>,
    pub source_path: String, // relative, no secrets
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum HarnessComponent {
    AgentsMd,
    SystemPrompt,
    ToolDefinitions,
    ActiveSkills,
    ApprovalPolicy,
    MemoryStore,
    Config,
    QualityGuardrails,
    Hooks,
    SubAgentDefinitions,
}
```

### Key behaviors

1. **Snapshot at session start**: Hash each component and produce the
   `HarnessSnapshot` from Priority 1.
2. **Change detection**: On `/new` or session save, compare current hashes
   to last-saved. Log component changes to the learning journey ledger.
3. **CLI**: `ca harness status` shows current component hashes and last change
   time. `ca harness diff <session_id>` shows which components changed between
   two sessions.
4. **Integration with Dream/Distill**: When Dream runs, it now has access to
   "which harness components changed since last Dream" — enabling targeted
   analysis.

### Integration points

- Uses existing `src/learning_journey.rs` to log `HarnessComponentChanged` events
- Feeds `HarnessSnapshot` to Priority 1's `SessionOutcomeRecord`
- `ca harness` subcommand via existing `src/main.rs` CLI routing
- No changes to tools, provider wire, or session format beyond Priority 1

### Implementation estimate

- New file: `src/harness_registry.rs` (~350 lines)
- Modify: `src/session.rs` (snapshot computation, ~60 lines)
- Modify: `src/main.rs` (CLI subcommand, ~40 lines)
- Modify: `src/learning_journey.rs` (new event kind, ~30 lines)
- Tests: ~150 lines

---

## Priority 3: Failure Trace Distillation

### Problem

Dream and Distill are great at consolidating memory and distilling skills, but
they focus on **what the agent learned** rather than **what went wrong with the
harness**. When a session fails, ca doesn't automatically ask: "Was this a tool
description problem? A system prompt gap? An approval policy issue?"

### Design

Extend Dream (7-day cycle) with a new **harness failure analysis** pass that
runs on sessions with poor outcomes (from Priority 1).

```rust
/// Extend src/learning.rs — add HarnessAnalysis to MaintenanceKind

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceKind {
    Dream,
    Distill,
    Analyze, // NEW: harness failure analysis
}
```

### Analysis flow

1. **Select candidates**: Sessions from the last 7 days where
   `OutcomeKind` is `Rejected` or `Abandoned`, or where
   `tool_failure_rates` exceed a threshold (e.g., >30% per-tool failure).
2. **Classify failure domain**: Using the session's `HarnessSnapshot` and
   tool summary, classify each failure into one of seven AHE-inspired domains:
   - `SystemPrompt`: Agent didn't understand the task context
   - `ToolDescription`: Agent called the wrong tool or wrong arguments
   - `ToolImplementation`: Tool returned unexpected error
   - `ApprovalPolicy`: User had to manually approve things that should be auto
   - `Skill`: Skill didn't activate or was unhelpful
   - `Memory`: Agent repeated a mistake it should have remembered
   - `SubAgent`: Sub-agent delegation failed or was unhelpful
3. **Generate structured report**: For each failure domain, produce a
   `HarnessFailureReport` with evidence links (session IDs, message indices),
   root cause hypothesis, and suggested fix.
4. **Store as learning artifact**: Reports go into the existing
   `LearningLedger` with a new `HarnessAnalysis` event kind. They're also
   written to `~/.local/share/ca/analysis/` for inspection.

### Structured report format

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessFailureReport {
    pub schema: String, // "ca.harness_failure_report.v1"
    pub report_id: Uuid,
    pub analyzed_sessions: Vec<Uuid>,
    pub domain: FailureDomain,
    pub failure_count: u64,
    pub success_rate_before: f64, // in sessions with same harness hash
    pub evidence: Vec<FailureEvidence>,
    pub root_cause: String,
    pub suggested_fix: String,
    pub predicted_impact: String,
    pub affected_component: HarnessComponent,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureEvidence {
    pub session_id: Uuid,
    pub message_index: u32,
    pub tool_name: String,
    pub error_category: String,
    pub user_correction_text_hash: String, // hash of correction, not raw text
}
```

### Integration points

- Extends existing `src/learning.rs` `MaintenanceKind` enum
- Uses `SessionOutcomeRecord` from Priority 1 as input
- Uses `HarnessComponent` from Priority 2 for failure domain mapping
- Reports feed into Priority 4's evidence-gated change system
- `/analysis` slash command shows recent reports
- `ca analysis --domain <domain>` CLI for filtered inspection

### Implementation estimate

- Extend: `src/learning.rs` (~300 lines for Analyze variant + analysis logic)
- New section in `src/learning.rs`: harness failure classifier (~200 lines)
- Modify: `src/main.rs` (CLI + slash commands, ~60 lines)
- Tests: ~250 lines

---

## Priority 4: Evidence-Gated Harness Changes

### Problem

ca's ExecPlan system is excellent documentation but relies on human discipline
to ensure changes have evidence. AHE's key innovation is **enforcing** that
every harness change cites failure evidence, states a root cause, and makes a
testable prediction.

### Design

Add a `HarnessChangeManifest` that is required when any harness component
is modified. This integrates with the existing ExecPlan workflow.

```rust
/// src/harness_manifest.rs — new file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessChangeManifest {
    pub schema: String, // "ca.harness_change_manifest.v1"
    pub manifest_id: Uuid,
    pub component: HarnessComponent,
    pub content_hash_before: String,
    pub content_hash_after: String,
    pub evidence: Vec<Uuid>,      // links to HarnessFailureReport IDs
    pub root_cause: String,
    pub targeted_fix: String,
    pub prediction: ChangePrediction,
    pub exec_plan_id: Option<String>, // link to ExecPlan if applicable
    pub created_at: DateTime<Utc>,
    pub verification: Option<VerificationResult>, // filled in later
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePrediction {
    pub metric: String,         // e.g., "tool_error_rate", "session_abandon_rate"
    pub direction: String,      // "decrease", "increase"
    pub magnitude: String,      // "significant", "moderate", "slight"
    pub confidence: f64,        // 0.0–1.0
    pub verification_window: u64, // number of sessions to wait
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub sessions_observed: u64,
    pub metric_before: f64,
    pub metric_after: f64,
    pub prediction_confirmed: bool,
    pub verified_at: DateTime<Utc>,
}
```

### Enforcement modes

1. **Strict mode** (default for harness changes): Refuses to save AGENTS.md,
   tool descriptions, or system prompt changes without a manifest. The manifest
   doesn't need to be perfect — even "manual improvement, no failure evidence,
   predicted slight improvement" is valid — but the prediction must be explicit.
2. **Advisory mode** (for skills, memory, config): Warns but allows changes
   without manifests. Learning artifacts from Dream/Distill/Analyze are exempt
   since they have their own evidence chains.
3. **Bypass**: `--no-manifest` flag for emergency fixes.

### Integration points

- Pre-edit hook in `src/hooks.rs`: `BeforeSubmitPrompt` checks for harness
  component edits and requires a manifest
- `/manifest` slash command to create and view manifests
- `ca manifest --pending` shows manifests awaiting verification
- Verification runs automatically after N sessions (configurable, default 20)
  by comparing the predicted metric with actual outcome data from Priority 1

### Implementation estimate

- New file: `src/harness_manifest.rs` (~400 lines)
- Modify: `src/hooks.rs` (harness edit detection, ~80 lines)
- Modify: `src/main.rs` (CLI + slash commands, ~60 lines)
- Tests: ~200 lines

---

## Priority 5: Cross-Session Task Stability Tracking

### Problem

ca tracks tool success within sessions but can't answer: "Does `read_file`
fail more often in Rust projects than Python projects?" or "Did the tool error
rate change after we updated the system prompt?" This requires cross-session
aggregation.

### Design

Add a `TaskStabilityIndex` that aggregates outcome data across sessions,
grouped by harness component versions and workspace characteristics.

```rust
/// src/task_stability.rs — new file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStabilityIndex {
    pub schema: String, // "ca.task_stability.v1"
    pub workspace_fingerprint: String,
    pub harness_hash: String,       // composite hash of all components
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub session_count: u64,
    pub outcome_distribution: BTreeMap<String, u64>, // OutcomeKind → count
    pub tool_stability: BTreeMap<String, ToolStability>,
    pub stability_score: f64, // 0.0–1.0 composite metric
    pub change_points: Vec<ChangePoint>, // harness changes detected
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStability {
    pub call_count: u64,
    pub success_rate: f64,
    pub avg_retries: f64,
    pub p95_latency_ms: u64,
    pub trend: Trend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Trend {
    Improving,
    Stable,
    Degrading,
    Insufficient, // < 5 data points
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePoint {
    pub at: DateTime<Utc>,
    pub component: HarnessComponent,
    pub hash_before: String,
    pub hash_after: String,
    pub metric_delta: f64, // change in stability_score
}
```

### Integration points

- Computed by a periodic background task (piggybacks on Dream cycle)
- `/stability` slash command shows current stability score and trends
- `ca stability --tool read_file` shows per-tool trend
- Feeds into Priority 3's Analyze pass for failure domain classification
- ChangePoints directly link to Priority 4's manifests

### Implementation estimate

- New file: `src/task_stability.rs` (~300 lines)
- Modify: `src/learning.rs` (stability computation during Dream, ~80 lines)
- Modify: `src/main.rs` (CLI + slash commands, ~40 lines)
- Tests: ~150 lines

---

## What NOT to Do (Adaptation Notes)

### Don't replicate AHE's benchmark-driven loop

AHE targets Terminal-Bench 2 pass@1 — a fixed benchmark with known-correct
answers. ca serves real users with diverse, unpredictable tasks. The adaptation
is: **user outcome signals (accept/reject/abandon) replace benchmark pass/fail**.

### Don't add AHE's E2B sandbox dependency

ca runs locally. The adaptation is: **use ca's existing sandbox/seatbelt system
and the user's local environment** as the execution context. Outcome telemetry
captures real-world conditions.

### Don't make harness changes automatic

AHE auto-edits the harness and evaluates. ca should keep human-in-the-loop for
harness changes (consistent with its ExecPlan philosophy). The adaptation is:
**evidence-gated manifests enforce discipline without removing human judgment**.

### Don't replicate AHE's Best-of-N exploration

AHE runs N parallel harness variants and picks the best. This is expensive and
complex for a local agent. The adaptation is: **cross-session stability tracking
detects regressions after changes, achieving the same goal at lower cost**.

### Don't expand Dream/Distill scope arbitrarily

ca's Dream and Distill already have clear boundaries (memory consolidation and
skill distillation). The new Analyze pass is a sibling, not a scope expansion.
Dream continues handling memory; Distill continues handling skills; Analyze
handles harness failure patterns.

---

## Implementation Roadmap

| Phase | Priority | Effort | Dependencies | Impact |
|---|---|---|---|---|
| Phase 1 | Session Outcome Telemetry | 2–3 days | None | Foundation for all phases |
| Phase 2 | Harness Component Registry | 1–2 days | Phase 1 | Enables change-outcome correlation |
| Phase 3 | Failure Trace Distillation | 2–3 days | Phase 1, 2 | Automated failure pattern detection |
| Phase 4 | Evidence-Gated Changes | 2–3 days | Phase 1, 2, 3 | Enforced improvement discipline |
| Phase 5 | Cross-Session Stability | 1–2 days | Phase 1 | Regression detection |

**Total estimated effort**: 8–13 days of focused implementation.

**Recommended starting point**: Phase 1 alone provides enormous value. Even
without the rest, knowing "60% of my sessions end with the user undoing ca's
work" or "read_file has a 15% failure rate in this project" is transformative
for understanding where to improve.

---

## How This Differs from What ca Already Has

| ca's Current System | AHE-Inspired Addition | Key Difference |
|---|---|---|
| Dream (memory consolidation) | Analyze (harness failure analysis) | Dream asks "what did I learn?" — Analyze asks "what broke and why?" |
| Distill (skill extraction) | Evidence-gated manifests | Distill extracts skills — manifests enforce evidence for *any* harness change |
| ExecPlan (change documentation) | ChangePrediction + VerificationResult | ExecPlans document *what* — manifests predict *impact* and verify it |
| session_metrics (tool counts) | SessionOutcomeRecord (user success signal) | Metrics count tools — outcomes measure whether the user succeeded |
| provider_trace (debug data) | Failure trace distillation | Traces are raw — distillation produces structured failure reports |
| learning_journey (event ledger) | Harness component change tracking | Journeys track skills — component tracking tracks harness versions |
| CONTINUITY.md (state tracking) | TaskStabilityIndex (cross-session aggregation) | Continuity is narrative — stability is quantitative |

---

## Concrete File Changes Summary

### New files (5)
- `src/session_outcome.rs` — session outcome telemetry
- `src/harness_registry.rs` — harness component hashing and registry
- `src/harness_manifest.rs` — evidence-gated change manifests
- `src/task_stability.rs` — cross-session stability tracking
- `docs/AHE-APPLICATION-PLAN.md` — this plan document

### Modified files (7)
- `src/learning.rs` — add `MaintenanceKind::Analyze` variant + failure analysis
- `src/session.rs` — outcome computation at save time
- `src/interaction.rs` — correction signal counting
- `src/app.rs` — outcome state in AppState
- `src/main.rs` — CLI subcommands (`ca outcomes`, `ca harness`, `ca analysis`,
  `ca manifest`, `ca stability`) and slash commands
- `src/hooks.rs` — harness edit detection for manifest enforcement
- `src/learning_journey.rs` — new event kinds for component changes and analysis

### Test additions
- `src/session_outcome.rs` — outcome classification, privacy, hashing
- `src/harness_registry.rs` — component detection, change detection
- `src/harness_manifest.rs` — manifest creation, enforcement modes
- `src/task_stability.rs` — stability computation, trend detection, change points
- `src/learning.rs` — Analyze pass integration, failure classification
