//! Bounded host-owned annotations on logical plan content, independent of wrapping.
use crate::work::{PlanArtifact, PlanId, PlanReview, TaskId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const MAX_NOTES: usize = 64;
const MAX_COMMENT_BYTES: usize = 4096;
const MAX_LEDGER_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FeedbackAnchor {
    Task {
        task_id: TaskId,
        quote: String,
    },
    /// Unicode scalar offsets into `logical_document`, never terminal rows.
    Text {
        start: usize,
        end: usize,
        quote: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackStatus {
    Draft,
    Stale,
    Submitted,
    Historical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanFeedbackNote {
    pub id: Uuid,
    pub plan_id: PlanId,
    pub reviewed_revision: u64,
    pub content_sha256: String,
    pub anchor: FeedbackAnchor,
    pub comment: String,
    pub status: FeedbackStatus,
    #[serde(default)]
    pub submission_id: Option<Uuid>,
    #[serde(default)]
    pub submitted_revision: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanFeedbackLedger {
    #[serde(default)]
    pub notes: Vec<PlanFeedbackNote>,
}

pub fn logical_document(plan: &PlanArtifact) -> String {
    let mut text = format!("# {}\n\n", plan.title);
    if let Some(body) = &plan.body {
        text.push_str(body);
        text.push_str("\n\n");
    }
    text.push_str("## Tasks\n\n");
    for task in &plan.tasks {
        text.push_str(&task.title);
        text.push('\n');
    }
    if !plan.decisions.is_empty() {
        text.push_str("\n## Decisions\n\n");
        for decision in &plan.decisions {
            text.push_str(decision);
            text.push('\n');
        }
    }
    text
}

pub fn content_digest(plan: &PlanArtifact) -> String {
    format!("{:x}", Sha256::digest(logical_document(plan).as_bytes()))
}

fn scalar_slice(text: &str, start: usize, end: usize) -> Option<String> {
    if start >= end || end > text.chars().count() {
        return None;
    }
    Some(text.chars().skip(start).take(end - start).collect())
}

impl FeedbackAnchor {
    /// Resolve selected content against logical source, independent of wrapping.
    /// Ambiguous quotations require an explicit task ID or source range.
    pub fn selected_quote(plan: &PlanArtifact, quote: &str) -> Result<Self, String> {
        let normalize = |text: &str| {
            let mut result = Vec::<(char, usize)>::new();
            for (offset, ch) in text.chars().enumerate() {
                let ch = if ch.is_whitespace() { ' ' } else { ch };
                if ch == ' ' && result.last().is_some_and(|(last, _)| *last == ' ') {
                    continue;
                }
                result.push((ch, offset));
            }
            result
        };
        if quote.len() > MAX_COMMENT_BYTES {
            return Err("Plan selection exceeds 4 KiB.".into());
        }
        let target = normalize(quote.trim())
            .into_iter()
            .map(|(ch, _)| ch)
            .collect::<Vec<_>>();
        if target.is_empty() {
            return Err("Select plan text before adding a note.".into());
        }
        let source = normalize(&logical_document(plan));
        let matches = source
            .windows(target.len())
            .filter(|window| window.iter().map(|(ch, _)| *ch).eq(target.iter().copied()))
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err("Selection is ambiguous or differs from logical source; use a task ID or an explicit source range from /plan comments.".into());
        }
        Self::text(plan, matches[0][0].1, matches[0][target.len() - 1].1 + 1)
    }

    pub fn text(plan: &PlanArtifact, start: usize, end: usize) -> Result<Self, String> {
        let quote = scalar_slice(&logical_document(plan), start, end)
            .ok_or("Select a nonempty logical source range inside the plan.")?;
        if quote.len() > MAX_COMMENT_BYTES {
            return Err("Plan selection exceeds 4 KiB.".into());
        }
        Ok(Self::Text { start, end, quote })
    }

    pub fn task(plan: &PlanArtifact, task_id: TaskId) -> Result<Self, String> {
        let task = plan
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .ok_or("Unknown plan task.")?;
        if task.title.len() > MAX_COMMENT_BYTES {
            return Err("Task title exceeds the anchor limit.".into());
        }
        Ok(Self::Task {
            task_id,
            quote: task.title.clone(),
        })
    }

    fn still_matches(&self, plan: &PlanArtifact) -> bool {
        match self {
            Self::Task { task_id, quote } => plan
                .tasks
                .iter()
                .any(|task| task.id == *task_id && task.title == *quote),
            Self::Text { start, end, quote } => {
                scalar_slice(&logical_document(plan), *start, *end).as_ref() == Some(quote)
            }
        }
    }
}

impl PlanFeedbackLedger {
    pub fn validate(&self) -> Result<(), String> {
        if self.notes.len() > MAX_NOTES
            || self.notes.iter().any(|note| {
                note.comment.trim().is_empty()
                    || note.comment.len() > MAX_COMMENT_BYTES
                    || note.content_sha256.len() != 64
                    || match &note.anchor {
                        FeedbackAnchor::Task { quote, .. } | FeedbackAnchor::Text { quote, .. } => {
                            quote.len() > MAX_COMMENT_BYTES
                        }
                    }
            })
        {
            return Err("Plan comments exceed their bounded ledger contract.".into());
        }
        let mut ids = std::collections::HashSet::new();
        if self.notes.iter().any(|note| !ids.insert(note.id)) {
            return Err("Duplicate plan comment identity.".into());
        }
        if serde_json::to_vec(self)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_LEDGER_BYTES
        {
            return Err("Plan comment ledger exceeds 128 KiB.".into());
        }
        Ok(())
    }

    pub fn refresh(&mut self, plan: &PlanArtifact) {
        let digest = content_digest(plan);
        for note in &mut self.notes {
            if note.status == FeedbackStatus::Draft
                && (note.plan_id != plan.id
                    || note.content_sha256 != digest
                    || !note.anchor.still_matches(plan))
            {
                note.status = FeedbackStatus::Stale;
            }
        }
    }

    pub fn add(
        &mut self,
        plan: &PlanArtifact,
        review: PlanReview,
        anchor: FeedbackAnchor,
        comment: String,
    ) -> Result<Uuid, String> {
        if review != plan.review() || !anchor.still_matches(plan) {
            return Err("The reviewed plan or selection changed. Review it again.".into());
        }
        let id = Uuid::new_v4();
        let mut next = self.clone();
        next.notes.push(PlanFeedbackNote {
            id,
            plan_id: plan.id,
            reviewed_revision: review.revision,
            content_sha256: content_digest(plan),
            anchor,
            comment,
            status: FeedbackStatus::Draft,
            submission_id: None,
            submitted_revision: None,
        });
        next.validate()?;
        *self = next;
        Ok(id)
    }

    pub fn edit(&mut self, id: Uuid, comment: String) -> Result<(), String> {
        let mut next = self.clone();
        let note = next
            .notes
            .iter_mut()
            .find(|note| note.id == id)
            .ok_or("Unknown plan comment.")?;
        if note.status != FeedbackStatus::Draft {
            return Err("Only draft comments can be edited; add a new comment after reviewing stale content.".into());
        }
        note.comment = comment;
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn delete(&mut self, id: Uuid) -> Result<(), String> {
        let note = self
            .notes
            .iter()
            .find(|note| note.id == id)
            .ok_or("Unknown plan comment.")?;
        if matches!(
            note.status,
            FeedbackStatus::Submitted | FeedbackStatus::Historical
        ) {
            return Err("Submitted comments retain historical provenance.".into());
        }
        self.notes.retain(|note| note.id != id);
        Ok(())
    }

    /// Returns one queue item identity. Repeated submission cannot enqueue it again.
    pub fn submit(
        &mut self,
        plan: &PlanArtifact,
        review: PlanReview,
    ) -> Result<(Uuid, String), String> {
        if review != plan.review() {
            return Err("The plan changed. Review its current revision before submitting.".into());
        }
        self.refresh(plan);
        if self
            .notes
            .iter()
            .any(|note| note.status == FeedbackStatus::Stale)
        {
            return Err("Remove or replace stale comments before submitting.".into());
        }
        let selected: Vec<_> = self
            .notes
            .iter_mut()
            .filter(|note| note.status == FeedbackStatus::Draft)
            .collect();
        if selected.is_empty() {
            return Err(
                "No draft comments to submit; this batch may already have been submitted.".into(),
            );
        }
        let id = Uuid::new_v4();
        let mut text = format!("Revise plan {} at reviewed revision {} using these comments (batch {}). Comments request revision only.\n", plan.id, review.revision, id);
        for note in selected {
            text.push_str(&format!(
                "\nAnchor {:?}\nComment: {}\n",
                note.anchor, note.comment
            ));
            note.status = FeedbackStatus::Submitted;
            note.submission_id = Some(id);
            note.submitted_revision = Some(review.revision);
        }
        Ok((id, text))
    }

    pub fn make_historical(&mut self) {
        for note in &mut self.notes {
            note.status = FeedbackStatus::Historical;
        }
    }

    pub fn render(&self, plan: &PlanArtifact) -> String {
        let mut output = format!("Plan comments — revision {}\n\nLogical source (Unicode character offsets):\n{}\n\nTasks:\n", plan.revision, logical_document(plan));
        for (index, task) in plan.tasks.iter().enumerate() {
            output.push_str(&format!("{}: {} {}\n", index + 1, task.id, task.title));
        }
        for note in &self.notes {
            output.push_str(&format!(
                "\n{} [{:?}]\n{:?}\n{}\n",
                note.id, note.status, note.anchor, note.comment
            ));
        }
        output.push_str("\n/plan comment task <number or UUID> <comment>\n/plan comment text <start>:<end> <comment>\n/plan comment edit <UUID> <comment>\n/plan comment delete <UUID>\n/plan submit\n");
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::{TaskStatus, WorkMutation, WorkState};
    fn work() -> WorkState {
        let mut work = WorkState::default();
        work.apply_model_mutation(WorkMutation::Propose {
            expected_revision: 0,
            title: "Unicode plan".into(),
            body: Some("你好 é repeated repeated".into()),
            tasks: vec!["First task".into()],
        })
        .unwrap();
        work
    }
    #[test]
    fn feedback_progress_does_not_change_content_digest() {
        let mut work = work();
        let plan = work.plan.as_ref().unwrap();
        let digest = content_digest(plan);
        let task = plan.tasks[0].id;
        work.apply_model_mutation(WorkMutation::UpdateTask {
            expected_revision: 1,
            task_id: task,
            status: TaskStatus::InProgress,
            blocked_reason: None,
        })
        .unwrap();
        assert_eq!(digest, content_digest(work.plan.as_ref().unwrap()));
    }
    #[test]
    fn feedback_unicode_ranges_are_logical_and_duplicate_quotes_unambiguous() {
        let work = work();
        let plan = work.plan.as_ref().unwrap();
        let source = logical_document(plan);
        let starts: Vec<_> = source
            .char_indices()
            .enumerate()
            .filter_map(|(scalar, (byte, _))| {
                source[byte..].starts_with("repeated").then_some(scalar)
            })
            .collect();
        let a = FeedbackAnchor::text(plan, starts[0], starts[0] + 8).unwrap();
        let b = FeedbackAnchor::text(plan, starts[1], starts[1] + 8).unwrap();
        assert_ne!(a, b);
        assert!(b.still_matches(plan));
    }
    #[test]
    fn feedback_rejects_stale_review_and_submits_once() {
        let work = work();
        let plan = work.plan.as_ref().unwrap();
        let mut ledger = PlanFeedbackLedger::default();
        let anchor = FeedbackAnchor::task(plan, plan.tasks[0].id).unwrap();
        assert!(ledger
            .add(
                plan,
                PlanReview {
                    revision: 0,
                    ..plan.review()
                },
                anchor.clone(),
                "change".into()
            )
            .is_err());
        ledger
            .add(plan, plan.review(), anchor, "change".into())
            .unwrap();
        let (id, text) = ledger.submit(plan, plan.review()).unwrap();
        assert!(text.contains(&id.to_string()));
        assert!(ledger.submit(plan, plan.review()).is_err());
    }
    #[test]
    fn feedback_changed_content_stays_stale_without_rebasing() {
        let mut work = work();
        let plan = work.plan.as_ref().unwrap();
        let mut ledger = PlanFeedbackLedger::default();
        let anchor = FeedbackAnchor::text(plan, 0, 5).unwrap();
        ledger
            .add(plan, plan.review(), anchor, "change".into())
            .unwrap();
        work.apply_model_mutation(WorkMutation::Edit {
            expected_revision: 1,
            title: Some("Other".into()),
            body: None,
            decisions: vec![],
        })
        .unwrap();
        let plan = work.plan.as_ref().unwrap();
        assert!(ledger.submit(plan, plan.review()).is_err());
        assert_eq!(ledger.notes[0].status, FeedbackStatus::Stale);
    }
    #[test]
    fn feedback_limits_fail_without_losing_existing_drafts() {
        let work = work();
        let plan = work.plan.as_ref().unwrap();
        let mut ledger = PlanFeedbackLedger::default();
        let anchor = FeedbackAnchor::task(plan, plan.tasks[0].id).unwrap();
        assert!(ledger
            .add(plan, plan.review(), anchor.clone(), "x".repeat(4097))
            .is_err());
        assert!(ledger.notes.is_empty());
        for _ in 0..64 {
            ledger
                .add(plan, plan.review(), anchor.clone(), "comment".into())
                .unwrap();
        }
        assert!(ledger
            .add(plan, plan.review(), anchor, "one more".into())
            .is_err());
        assert_eq!(ledger.notes.len(), 64);
    }
    #[test]
    fn feedback_resume_and_fork_preserve_provenance_without_actionable_notes() {
        let work = work();
        let plan = work.plan.as_ref().unwrap();
        let mut ledger = PlanFeedbackLedger::default();
        ledger
            .add(
                plan,
                plan.review(),
                FeedbackAnchor::task(plan, plan.tasks[0].id).unwrap(),
                "change".into(),
            )
            .unwrap();
        let bytes = serde_json::to_vec(&ledger).unwrap();
        let mut loaded: PlanFeedbackLedger = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(loaded, ledger);
        loaded.make_historical();
        assert_eq!(loaded.notes[0].status, FeedbackStatus::Historical);
        assert!(loaded.submit(plan, plan.review()).is_err());
    }
    #[test]
    fn selected_quotes_resolve_unicode_wrapping_and_refuse_duplicate_content() {
        let work = work();
        let plan = work.plan.as_ref().unwrap();
        let anchor = FeedbackAnchor::selected_quote(plan, "你好\né").unwrap();
        assert!(anchor.still_matches(plan));
        assert!(FeedbackAnchor::selected_quote(plan, "repeated").is_err());
        assert!(FeedbackAnchor::selected_quote(plan, "not in the source").is_err());
    }
    #[test]
    fn feedback_content_changes_stale_notes_even_when_the_task_quote_is_unchanged() {
        let mut work = work();
        let plan = work.plan.as_ref().unwrap();
        let mut ledger = PlanFeedbackLedger::default();
        ledger
            .add(
                plan,
                plan.review(),
                FeedbackAnchor::task(plan, plan.tasks[0].id).unwrap(),
                "review this task".into(),
            )
            .unwrap();
        work.apply_model_mutation(WorkMutation::Edit {
            expected_revision: 1,
            title: None,
            body: Some("Changed constraints".into()),
            decisions: vec![],
        })
        .unwrap();
        let plan = work.plan.as_ref().unwrap();
        ledger.refresh(plan);
        assert_eq!(ledger.notes[0].status, FeedbackStatus::Stale);
        assert!(ledger.submit(plan, plan.review()).is_err());
    }
}
