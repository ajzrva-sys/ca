//! Prompt rendering and prompt-size budget helpers.
//!
//! Builds the final prompt strings sent to AI roles, including state JSON,
//! control-doc excerpts, and queue briefs.  Also contains the schema
//! placeholder resolver.

use anyhow::Result;
use serde_json::Value;

use crate::paths::Paths;
use crate::prompts::TASK_SCHEMA_JSON;
use crate::queue::next_steps::{next_steps_state_from_text, parse_approved_next_steps_tasks};
use crate::state::State;

use super::formatting::truncate_inline;

// ---------------------------------------------------------------------------
// Prompt size limits
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(crate) struct PromptLimits {
    pub(crate) roadmap: usize,
    pub(crate) next_steps: usize,
    pub(crate) next_steps_non_ai: usize,
}

impl PromptLimits {
    pub(crate) fn full() -> Self {
        Self {
            roadmap: 24_000,
            next_steps: 24_000,
            next_steps_non_ai: 16_000,
        }
    }

    pub(crate) fn compact_planner() -> Self {
        Self {
            roadmap: 1_800,
            next_steps: 2_400,
            next_steps_non_ai: 900,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn worker_like() -> Self {
        Self {
            roadmap: 1_000,
            next_steps: 1_600,
            next_steps_non_ai: 600,
        }
    }

    pub(crate) fn senior_review() -> Self {
        Self {
            roadmap: 800,
            next_steps: 1_200,
            next_steps_non_ai: 400,
        }
    }
}

// ---------------------------------------------------------------------------
// Prompt rendering
// ---------------------------------------------------------------------------

pub(crate) fn render_prompt(
    template: &str,
    paths: &Paths,
    state: &State,
    extra: Value,
) -> Result<String> {
    render_prompt_limited(template, paths, state, extra, PromptLimits::full())
}

pub(crate) fn render_prompt_limited(
    template: &str,
    paths: &Paths,
    state: &State,
    extra: Value,
    limits: PromptLimits,
) -> Result<String> {
    let state_json = serde_json::to_string_pretty(state)?;
    Ok(format!(
        r#"{template}

## Mission
{mission}

## Orchestrator state JSON
```json
{state_json}
```

## ROADMAP.md
```md
{roadmap}
```

## NEXT_STEPS.md
```md
{next_steps}
```

## NEXT_STEPS_non-ai.md
```md
{next_steps_non_ai}
```

## Extra run context
```json
{extra}
```

Return only the JSON object required by the provided output schema. Do not wrap it in Markdown.
"#,
        template = template,
        mission = state.mission,
        state_json = state_json,
        roadmap = crate::app::truncate(
            &crate::app::read_text_or_empty(&paths.roadmap),
            limits.roadmap
        ),
        next_steps = crate::app::truncate(
            &crate::app::read_text_or_empty(&paths.next_steps),
            limits.next_steps
        ),
        next_steps_non_ai = crate::app::truncate(
            &crate::app::read_text_or_empty(&paths.next_steps_non_ai),
            limits.next_steps_non_ai
        ),
        extra = serde_json::to_string_pretty(&extra)?,
    ))
}

pub(crate) fn render_project_manager_compact_prompt(
    template: &str,
    paths: &Paths,
    state: &State,
    extra: Value,
) -> Result<String> {
    let state_json = serde_json::to_string_pretty(state)?;
    let limits = PromptLimits::compact_planner();
    Ok(format!(
        r#"{template}

## Compact Project Manager Context Mode

Rust already pre-read the queue state and built the brief below. Prefer the
brief and the embedded control-doc excerpts over broad repository inspection.

Rules for compact mode:
- Do not read source/test files by default.
- Read source/test files only when a task card cannot be made safe from the
  control docs, readiness packets, and queue brief.
- If a readiness/spec packet names the next safe gate, approve at most one
  tightly constrained task from that packet instead of re-auditing the repo.
- Prefer `NEXT_STEPS_non-ai.md` for command-only verification.
- Escalate or choose planning only when the brief/control docs are genuinely
  contradictory or insufficient.

## Mission
{mission}

## Orchestrator state JSON
```json
{state_json}
```

## Rust queue brief
```md
{brief}
```

## ROADMAP.md excerpt
```md
{roadmap}
```

## NEXT_STEPS.md excerpt
```md
{next_steps}
```

## NEXT_STEPS_non-ai.md excerpt
```md
{next_steps_non_ai}
```

## Extra run context
```json
{extra}
```

Return only the JSON object required by the provided output schema. Do not wrap it in Markdown.
"#,
        template = template,
        mission = state.mission,
        state_json = state_json,
        brief = build_planner_queue_brief(paths, state),
        roadmap = compact_control_doc_excerpt(
            &crate::app::read_text_or_empty(&paths.roadmap),
            limits.roadmap
        ),
        next_steps = compact_control_doc_excerpt(
            &crate::app::read_text_or_empty(&paths.next_steps),
            limits.next_steps
        ),
        next_steps_non_ai = compact_control_doc_excerpt(
            &crate::app::read_text_or_empty(&paths.next_steps_non_ai),
            limits.next_steps_non_ai
        ),
        extra = serde_json::to_string_pretty(&extra)?,
    ))
}

fn compact_control_doc_excerpt(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let head_chars = max_chars / 3;
    let tail_chars = max_chars - head_chars;
    let head = text.chars().take(head_chars).collect::<String>();
    let tail = text
        .chars()
        .rev()
        .take(tail_chars)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{head}\n\n<!-- compact excerpt: middle omitted -->\n\n{tail}")
}

pub(crate) fn build_planner_queue_brief(paths: &Paths, state: &State) -> String {
    let next_steps_text = crate::app::read_text_or_empty(&paths.next_steps);
    let non_ai_text = crate::app::read_text_or_empty(&paths.next_steps_non_ai);
    let next_state = next_steps_state_from_text(&next_steps_text);
    let non_ai_state = next_steps_state_from_text(&non_ai_text);
    let approved_worker = parse_approved_next_steps_tasks(&next_steps_text)
        .into_iter()
        .map(|task| format!("{} [{}] {}", task.task_id, task.kind, task.title))
        .collect::<Vec<_>>();
    let approved_non_ai = parse_approved_next_steps_tasks(&non_ai_text)
        .into_iter()
        .map(|task| format!("{} [{}] {}", task.task_id, task.kind, task.title))
        .collect::<Vec<_>>();
    let recipes = crate::app::deterministic_recipe_summaries(paths, 5);
    let git_status = crate::git::planner_git_status(paths);

    format!(
        "- Mission complete flag: {}\n- In-memory queue length: {}\n- NEXT_STEPS.md: approved={} unfinished={} future_work={}\n- NEXT_STEPS_non-ai.md: approved={} unfinished={} future_work={}\n- Approved Worker tasks: {}\n- Approved deterministic tasks: {}\n- Recent deterministic recipe candidates: {}\n- Last review: {}\n- Git status: {}\n",
        state.project_complete,
        state.queue.len(),
        next_state.approved,
        next_state.unfinished,
        next_state.future_work,
        non_ai_state.approved,
        non_ai_state.unfinished,
        non_ai_state.future_work,
        if approved_worker.is_empty() {
            "none".to_string()
        } else {
            approved_worker.join("; ")
        },
        if approved_non_ai.is_empty() {
            "none".to_string()
        } else {
            approved_non_ai.join("; ")
        },
        if recipes.is_empty() {
            "none".to_string()
        } else {
            recipes.join("; ")
        },
        state
            .last_review_summary
            .as_deref()
            .map(|summary| truncate_inline(summary, 500))
            .unwrap_or_else(|| "none".to_string()),
        git_status,
    )
}

// ---------------------------------------------------------------------------
// Schema placeholder resolver
// ---------------------------------------------------------------------------

pub(crate) fn render_schema(text: &str) -> String {
    text.replace("TASK_SCHEMA_PLACEHOLDER", TASK_SCHEMA_JSON.trim())
}
