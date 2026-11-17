//! Explicit task sections keep unrelated work out of a graph lane's brief.
//! Ambiguous and legacy plans retain their complete existing context.

use crate::work::{PlanArtifact, TaskId};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use std::collections::HashSet;

pub const AUTHOR_GUIDANCE: &str = "Break the request into focused tasks with one coherent objective each. Preserve the overall purpose and all shared constraints, including security-relevant context. Use exactly one level-two heading `## Shared purpose and constraints`, then one `## Task: <exact task title>` section for every ordered task title. Put task-specific inputs, instructions, dependencies and done criteria inside its section using level-three headings. Keep all globally applicable context in the shared section; do not repeat unrelated tasks' details there. Do not add other level-two headings. Propose only; do not execute work.";

/// Return a focused brief only for the explicit, complete section contract.
pub fn plan_context(plan: &PlanArtifact, task: TaskId) -> Option<String> {
    let selected = &plan.tasks.iter().find(|item| item.id == task)?.title;
    let body = plan.body.as_deref()?;
    let titles: HashSet<_> = plan.tasks.iter().map(|item| item.title.as_str()).collect();
    if titles.len() != plan.tasks.len() {
        return None;
    }
    let mut headings = Vec::new();
    let mut active: Option<(usize, String)> = None;
    for (event, range) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H2,
                ..
            }) => {
                // An unrecognized heading can carry shared constraints. Keep
                // full context whenever its ownership is ambiguous.
                if (range.start == 0 || body.as_bytes()[range.start - 1] == b'\n')
                    && body[range.start..].starts_with("## ")
                {
                    active = Some((range.start, String::new()));
                } else {
                    return None;
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, title)) = &mut active {
                    title.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(HeadingLevel::H2)) => {
                if let Some((start, title)) = active.take() {
                    headings.push((start, title));
                }
            }
            _ => {}
        }
    }
    let mut seen = HashSet::new();
    let mut shared = false;
    for (_, heading) in &headings {
        if heading == "Shared purpose and constraints" {
            if shared {
                return None;
            }
            shared = true;
        } else {
            let title = heading.strip_prefix("Task: ")?;
            if !titles.contains(title) || !seen.insert(title) {
                return None;
            }
        }
    }
    if !shared || seen != titles {
        return None;
    }
    let mut text = format!(
        "# {}\n\nPlan {} · revision {}\n\n",
        plan.title, plan.id, plan.revision
    );
    text.push_str(&body[..headings.first()?.0]);
    for (index, (start, heading)) in headings.iter().enumerate() {
        if heading == "Shared purpose and constraints"
            || heading.strip_prefix("Task: ") == Some(selected.as_str())
        {
            let end = headings.get(index + 1).map_or(body.len(), |item| item.0);
            text.push_str(&body[*start..end]);
            text.push('\n');
        }
    }
    if !plan.decisions.is_empty() {
        text.push_str("\n## Shared decisions\n");
        for decision in &plan.decisions {
            text.push_str(&format!("- {decision}\n"));
        }
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::{WorkMutation, WorkState};

    fn plan(body: &str) -> PlanArtifact {
        let mut work = WorkState::default();
        work.apply_model_mutation(WorkMutation::Propose {
            expected_revision: 0,
            title: "Improve the local project".into(),
            body: Some(body.into()),
            tasks: vec!["Inspect parser".into(), "Update docs".into()],
        })
        .unwrap();
        work.plan.unwrap()
    }

    const BODY: &str = "## Shared purpose and constraints\nKeep all work local; preserve the overall project goal.\n\n## Task: Inspect parser\n### Inputs\nparser.rs\n### Done\nRecord parser evidence.\n\n## Task: Update docs\nUNRELATED_DOCS_DETAIL\n";

    #[test]
    fn task_scope_keeps_shared_constraints_and_only_assigned_task() {
        let mut plan = plan(BODY);
        plan.decisions.push("Do not contact the live host".into());
        let focused = plan_context(&plan, plan.tasks[0].id).unwrap();
        assert!(focused.contains("preserve the overall project goal"));
        assert!(focused.contains("parser.rs"));
        assert!(focused.contains("Do not contact the live host"));
        assert!(!focused.contains("UNRELATED_DOCS_DETAIL"));
        assert!(!focused.contains("Task: Update docs"));
        assert!(plan
            .body
            .as_ref()
            .unwrap()
            .contains("UNRELATED_DOCS_DETAIL"));
    }

    #[test]
    fn task_scope_preserves_shared_section_after_task_sections() {
        let body = "## Task: Inspect parser\nparser.rs\n## Task: Update docs\ndocs.md\n## Shared purpose and constraints\nGlobal boundary at the end.\n";
        let plan = plan(body);
        let focused = plan_context(&plan, plan.tasks[0].id).unwrap();
        assert!(focused.contains("Global boundary at the end"));
        assert!(!focused.contains("docs.md"));
    }

    #[test]
    fn task_scope_ignores_code_fence_headings() {
        let body = BODY.replace(
            "parser.rs",
            "```markdown\n## Task: Update docs\n```\nparser.rs",
        );
        let plan = plan(&body);
        assert!(plan_context(&plan, plan.tasks[0].id)
            .unwrap()
            .contains("parser.rs"));
    }

    #[test]
    fn task_scope_falls_back_for_legacy_ambiguous_or_incomplete_plans() {
        for body in [
            "Legacy unstructured plan",
            "## Task: Inspect parser\nMissing shared purpose and other task.",
            &BODY.replace("## Task: Update docs", "## Task: Unknown task"),
            &BODY.replace("## Task: Update docs", "## Task: Inspect parser"),
            &format!("{BODY}\n## Other global heading\nRetain this context."),
            &format!("{BODY}\nGlobal constraints\n---\nKeep this boundary."),
            &format!("{BODY}\n  ## Global constraints\nKeep this boundary."),
        ] {
            let plan = plan(body);
            assert!(plan_context(&plan, plan.tasks[0].id).is_none());
        }
    }
}
