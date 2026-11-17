#![allow(dead_code)]
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum QueueDelivery {
    #[default]
    Plain,
    Steer,
}

impl QueueDelivery {
    fn is_plain(self) -> bool {
        matches!(self, Self::Plain)
    }
}

fn skip_plain_delivery(delivery: &QueueDelivery) -> bool {
    delivery.is_plain()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingInput {
    pub id: Uuid,
    pub text: String,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "skip_plain_delivery")]
    pub delivery: QueueDelivery,
}

impl PendingInput {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            text: text.into(),
            created_at: Utc::now(),
            delivery: QueueDelivery::Plain,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct InputQueue {
    items: VecDeque<PendingInput>,
    pub selected_index: usize,
    pub editing_id: Option<Uuid>,
}

impl InputQueue {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> impl Iterator<Item = &PendingInput> {
        self.items.iter()
    }

    pub fn persisted_items(&self) -> Vec<PendingInput> {
        self.items.iter().cloned().collect()
    }

    pub fn restore(&mut self, items: Vec<PendingInput>) {
        self.items = items.into();
        self.selected_index = 0;
        self.editing_id = None;
    }

    pub fn enqueue(&mut self, text: impl Into<String>) -> Option<Uuid> {
        self.enqueue_with_delivery(text, QueueDelivery::Plain)
    }

    pub fn enqueue_with_delivery(
        &mut self,
        text: impl Into<String>,
        delivery: QueueDelivery,
    ) -> Option<Uuid> {
        let text = text.into();
        if text.trim().is_empty() {
            return None;
        }
        let mut item = PendingInput::new(text);
        item.delivery = delivery;
        let id = item.id;
        self.items.push_back(item);
        Some(id)
    }

    pub fn take_delivery(&mut self, delivery: QueueDelivery) -> Vec<PendingInput> {
        let mut taken = Vec::new();
        let mut kept = VecDeque::new();
        while let Some(item) = self.items.pop_front() {
            if item.delivery == delivery {
                taken.push(item);
            } else {
                kept.push_back(item);
            }
        }
        self.items = kept;
        self.clamp_selection();
        taken
    }

    pub fn enqueue_with_id(&mut self, id: Uuid, text: impl Into<String>) {
        if self.items.iter().any(|item| item.id == id) {
            return;
        }
        self.items.push_back(PendingInput {
            id,
            text: text.into(),
            created_at: DateTime::from_timestamp(0, 0).unwrap(),
            delivery: QueueDelivery::Plain,
        });
    }

    pub fn enqueue_front_with_id(&mut self, id: Uuid, text: impl Into<String>) {
        if self.items.iter().any(|item| item.id == id) {
            return;
        }
        self.items.push_front(PendingInput {
            id,
            text: text.into(),
            created_at: Utc::now(),
            delivery: QueueDelivery::Plain,
        });
        self.selected_index = 0;
    }

    pub fn pop_front(&mut self) -> Option<PendingInput> {
        let item = self.items.pop_front();
        self.clamp_selection();
        item
    }

    pub fn push_front(&mut self, item: PendingInput) {
        self.items.push_front(item);
        self.clamp_selection();
    }

    pub fn front(&self) -> Option<&PendingInput> {
        self.items.front()
    }

    pub fn selected(&self) -> Option<&PendingInput> {
        self.items.get(self.selected_index)
    }

    pub fn select_id(&mut self, id: Uuid) -> bool {
        let Some(index) = self.items.iter().position(|item| item.id == id) else {
            return false;
        };
        self.selected_index = index;
        true
    }

    pub fn select_previous(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    pub fn select_next(&mut self) {
        if self.selected_index + 1 < self.items.len() {
            self.selected_index += 1;
        }
    }

    pub fn delete_selected(&mut self) -> Option<PendingInput> {
        let removed = self.items.remove(self.selected_index);
        if removed
            .as_ref()
            .is_some_and(|item| Some(item.id) == self.editing_id)
        {
            self.editing_id = None;
        }
        self.clamp_selection();
        removed
    }

    pub fn move_selected_up(&mut self) -> bool {
        if self.selected_index == 0 || self.selected_index >= self.items.len() {
            return false;
        }
        self.items
            .swap(self.selected_index, self.selected_index.saturating_sub(1));
        self.selected_index -= 1;
        true
    }

    pub fn move_selected_down(&mut self) -> bool {
        if self.selected_index + 1 >= self.items.len() {
            return false;
        }
        self.items
            .swap(self.selected_index, self.selected_index + 1);
        self.selected_index += 1;
        true
    }

    pub fn begin_edit_selected(&mut self) -> Option<String> {
        let item = self.selected()?.clone();
        self.editing_id = Some(item.id);
        Some(item.text)
    }

    pub fn begin_edit_last(&mut self) -> Option<String> {
        let index = self.items.len().checked_sub(1)?;
        self.selected_index = index;
        self.begin_edit_selected()
    }

    pub fn save_edit(&mut self, text: impl Into<String>) -> bool {
        let Some(id) = self.editing_id.take() else {
            return false;
        };
        let text = text.into();
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.text = text;
            true
        } else {
            false
        }
    }

    pub fn remove_by_id(&mut self, id: Uuid) -> Option<PendingInput> {
        let index = self.items.iter().position(|item| item.id == id)?;
        let removed = self.items.remove(index);
        if self.editing_id == Some(id) {
            self.editing_id = None;
        }
        self.clamp_selection();
        removed
    }

    fn clamp_selection(&mut self) {
        self.selected_index = self.selected_index.min(self.items.len().saturating_sub(1));
    }
}

pub fn should_drain_after(outcome: crate::activity::TurnOutcome) -> bool {
    outcome == crate::activity::TurnOutcome::Completed
}

/// Why a queued row is waiting. Kept off [`PendingInput`] so session bytes stay the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum QueuedFollowUpKind {
    Plain,
    Bash,
    Skill,
    SyntheticWake,
}

/// One queued follow-up as the merger sees it. The transcript row is not this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedRow {
    pub text: String,
    pub has_images: bool,
    pub kind: QueuedFollowUpKind,
}

impl QueuedRow {
    pub fn new(text: impl Into<String>, has_images: bool, kind: QueuedFollowUpKind) -> Self {
        Self {
            text: text.into(),
            has_images,
            kind,
        }
    }

    pub fn plain(text: impl Into<String>) -> Self {
        Self::new(text, false, QueuedFollowUpKind::Plain)
    }

    pub fn plain_with_images(text: impl Into<String>) -> Self {
        Self::new(text, true, QueuedFollowUpKind::Plain)
    }

    pub fn bash(text: impl Into<String>) -> Self {
        Self::new(text, false, QueuedFollowUpKind::Bash)
    }

    pub fn skill(text: impl Into<String>) -> Self {
        Self::new(text, false, QueuedFollowUpKind::Skill)
    }

    pub fn synthetic_wake(text: impl Into<String>) -> Self {
        Self::new(text, false, QueuedFollowUpKind::SyntheticWake)
    }

    fn joins_plain_merge(&self) -> bool {
        self.kind == QueuedFollowUpKind::Plain && !self.has_images
    }
}

/// One model request built from one or more queued rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedModelTurn {
    pub body: String,
    pub has_images: bool,
    pub kind: QueuedFollowUpKind,
    /// Rows folded into this request. The transcript copy is separate.
    pub source: Vec<QueuedRow>,
}

/// Model turns plus the queued rows in their original order.
///
/// Merging does not drop rows. Callers still show every queued follow-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedFollowUpMerge {
    pub turns: Vec<MergedModelTurn>,
    pub rows: Vec<QueuedRow>,
}

/// Fold adjacent plain follow-ups into model turns.
///
/// A leading plain row may carry images. A later row joins that turn only when
/// it is plain text with no images. Bash, expanded skill, and synthetic-wake
/// rows stay their own turns and end the current run. Later plain rows can
/// start another run. The returned `rows` are the input, unchanged.
pub fn classify_queued_text(text: &str) -> QueuedRow {
    let trimmed = text.trim_start();
    if trimmed.starts_with('!') || trimmed.starts_with("/bash") {
        QueuedRow::bash(text)
    } else if trimmed.starts_with("/skill") {
        QueuedRow::skill(text)
    } else if trimmed.starts_with("[synthetic]") {
        QueuedRow::synthetic_wake(text)
    } else {
        QueuedRow::plain(text)
    }
}

/// How many leading queued texts belong to the first model turn, and that body.
pub fn merged_drain_prefix(texts: &[String]) -> (String, usize) {
    if texts.is_empty() {
        return (String::new(), 0);
    }
    let rows = texts
        .iter()
        .map(|text| classify_queued_text(text))
        .collect::<Vec<_>>();
    let merged = merge_plain_queued_follow_ups(&rows);
    let turn = &merged.turns[0];
    (turn.body.clone(), turn.source.len())
}

pub fn merge_plain_queued_follow_ups(rows: &[QueuedRow]) -> QueuedFollowUpMerge {
    let mut turns = Vec::new();
    let mut index = 0;
    while index < rows.len() {
        if rows[index].kind != QueuedFollowUpKind::Plain {
            turns.push(singleton_turn(&rows[index]));
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while end < rows.len() && rows[end].joins_plain_merge() {
            end += 1;
        }
        let source = rows[index..end].to_vec();
        turns.push(MergedModelTurn {
            body: join_plain_bodies(&source),
            has_images: source[0].has_images,
            kind: QueuedFollowUpKind::Plain,
            source,
        });
        index = end;
    }
    QueuedFollowUpMerge {
        turns,
        rows: rows.to_vec(),
    }
}

fn singleton_turn(row: &QueuedRow) -> MergedModelTurn {
    MergedModelTurn {
        body: row.text.clone(),
        has_images: row.has_images,
        kind: row.kind,
        source: vec![row.clone()],
    }
}

fn join_plain_bodies(rows: &[QueuedRow]) -> String {
    let mut body = String::new();
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            body.push_str("\n\n");
        }
        body.push_str(&row.text);
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_preserves_order_and_stable_ids() {
        let mut queue = InputQueue::default();
        let first = queue.enqueue("first").unwrap();
        let second = queue.enqueue("second").unwrap();

        assert_eq!(
            queue.items().map(|item| item.id).collect::<Vec<_>>(),
            vec![first, second]
        );
        assert_eq!(queue.pop_front().unwrap().id, first);
        assert_eq!(queue.pop_front().unwrap().id, second);
    }

    #[test]
    fn edit_replaces_in_place_without_changing_identity() {
        let mut queue = InputQueue::default();
        let id = queue.enqueue("draft").unwrap();
        assert_eq!(queue.begin_edit_selected().as_deref(), Some("draft"));
        assert!(queue.save_edit("edited"));

        let item = queue.selected().unwrap();
        assert_eq!(item.id, id);
        assert_eq!(item.text, "edited");
    }

    #[test]
    fn edit_last_targets_the_newest_item() {
        let mut queue = InputQueue::default();
        queue.enqueue("first");
        let last = queue.enqueue("last").unwrap();
        assert_eq!(queue.begin_edit_last().as_deref(), Some("last"));
        assert_eq!(queue.editing_id, Some(last));
        assert_eq!(queue.selected_index, 1);
    }

    #[test]
    fn delete_and_reorder_update_selection() {
        let mut queue = InputQueue::default();
        queue.enqueue("one");
        let two = queue.enqueue("two").unwrap();
        queue.enqueue("three");
        queue.select_next();
        assert!(queue.move_selected_down());
        assert_eq!(queue.selected().unwrap().id, two);
        assert!(queue.move_selected_up());
        assert_eq!(queue.delete_selected().unwrap().id, two);
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn remove_edited_item_by_id_clears_edit_state() {
        let mut queue = InputQueue::default();
        let id = queue.enqueue("steer me").unwrap();
        queue.begin_edit_selected();
        assert_eq!(queue.remove_by_id(id).unwrap().text, "steer me");
        assert!(queue.editing_id.is_none());
        assert!(queue.is_empty());
    }

    #[test]
    fn steer_delivery_round_trips_and_old_rows_stay_plain() {
        let mut queue = InputQueue::default();
        queue.enqueue_with_delivery("steer me", QueueDelivery::Steer);
        queue.enqueue("later");
        let json = serde_json::to_string(&queue.persisted_items()).unwrap();
        assert!(json.contains("\"delivery\":\"steer\""));
        assert!(!json.contains("\"delivery\":\"plain\""));
        let restored: Vec<PendingInput> = serde_json::from_str(&json).unwrap();
        let mut queue = InputQueue::default();
        queue.restore(restored);
        let taken = queue.take_delivery(QueueDelivery::Steer);
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].text, "steer me");
        assert_eq!(queue.front().unwrap().delivery, QueueDelivery::Plain);
        let legacy = r#"{"id":"00000000-0000-0000-0000-000000000001","text":"old","created_at":"2026-01-01T00:00:00Z"}"#;
        let item: PendingInput = serde_json::from_str(legacy).unwrap();
        assert_eq!(item.delivery, QueueDelivery::Plain);
        assert_eq!(item.text, "old");
    }

    #[test]
    fn persisted_items_restore_order_but_not_transient_editor_state() {
        let mut queue = InputQueue::default();
        let first = queue.enqueue("first").unwrap();
        let second = queue.enqueue("second").unwrap();
        queue.select_next();
        queue.begin_edit_selected();
        let persisted = queue.persisted_items();

        let mut restored = InputQueue::default();
        restored.enqueue("discarded");
        restored.restore(persisted);

        assert_eq!(
            restored.items().map(|item| item.id).collect::<Vec<_>>(),
            [first, second]
        );
        assert_eq!(restored.selected_index, 0);
        assert!(restored.editing_id.is_none());
    }

    #[test]
    fn only_successful_idle_completion_allows_drain() {
        use crate::activity::TurnOutcome;
        assert!(should_drain_after(TurnOutcome::Completed));
        assert!(!should_drain_after(TurnOutcome::None));
        assert!(!should_drain_after(TurnOutcome::InProgress));
        assert!(!should_drain_after(TurnOutcome::Cancelled));
        assert!(!should_drain_after(TurnOutcome::Failed));
    }

    #[test]
    fn three_plain_follow_ups_merge_into_one_body() {
        let rows = vec![
            QueuedRow::plain("alpha"),
            QueuedRow::plain("beta"),
            QueuedRow::plain("gamma"),
        ];
        let merged = merge_plain_queued_follow_ups(&rows);
        assert_eq!(merged.rows, rows);
        assert_eq!(merged.turns.len(), 1);
        assert_eq!(merged.turns[0].body, "alpha\n\nbeta\n\ngamma");
        assert!(!merged.turns[0].has_images);
        assert_eq!(merged.turns[0].kind, QueuedFollowUpKind::Plain);
        assert_eq!(merged.turns[0].source, rows);
    }

    #[test]
    fn image_on_a_follower_stops_the_merge() {
        let rows = vec![
            QueuedRow::plain("alpha"),
            QueuedRow::plain("beta"),
            QueuedRow::plain_with_images("gamma"),
            QueuedRow::plain("delta"),
        ];
        let merged = merge_plain_queued_follow_ups(&rows);
        assert_eq!(merged.rows, rows);
        assert_eq!(merged.turns.len(), 2);
        assert_eq!(merged.turns[0].body, "alpha\n\nbeta");
        assert!(!merged.turns[0].has_images);
        assert_eq!(&merged.turns[0].source[..], &rows[..2]);
        assert_eq!(merged.turns[1].body, "gamma\n\ndelta");
        assert!(merged.turns[1].has_images);
        assert_eq!(&merged.turns[1].source[..], &rows[2..]);
    }

    #[test]
    fn bash_row_stops_the_merge() {
        let rows = vec![
            QueuedRow::plain("alpha"),
            QueuedRow::plain("beta"),
            QueuedRow::bash("echo hi"),
            QueuedRow::plain("gamma"),
            QueuedRow::plain("delta"),
        ];
        let merged = merge_plain_queued_follow_ups(&rows);
        assert_eq!(merged.rows, rows);
        assert_eq!(merged.turns.len(), 3);
        assert_eq!(merged.turns[0].body, "alpha\n\nbeta");
        assert_eq!(merged.turns[1].kind, QueuedFollowUpKind::Bash);
        assert_eq!(merged.turns[1].body, "echo hi");
        assert_eq!(merged.turns[1].source.len(), 1);
        assert_eq!(merged.turns[2].body, "gamma\n\ndelta");
        assert_eq!(merged.turns[2].kind, QueuedFollowUpKind::Plain);
    }

    #[test]
    fn skill_and_synthetic_wake_stay_separate() {
        let rows = vec![
            QueuedRow::plain("alpha"),
            QueuedRow::skill("expanded skill"),
            QueuedRow::plain("beta"),
            QueuedRow::synthetic_wake("wake"),
            QueuedRow::plain("gamma"),
        ];
        let merged = merge_plain_queued_follow_ups(&rows);
        assert_eq!(merged.rows, rows);
        assert_eq!(
            merged
                .turns
                .iter()
                .map(|turn| turn.kind)
                .collect::<Vec<_>>(),
            vec![
                QueuedFollowUpKind::Plain,
                QueuedFollowUpKind::Skill,
                QueuedFollowUpKind::Plain,
                QueuedFollowUpKind::SyntheticWake,
                QueuedFollowUpKind::Plain,
            ]
        );
        assert_eq!(merged.turns[1].body, "expanded skill");
        assert_eq!(merged.turns[3].body, "wake");
        assert_eq!(merged.turns[0].body, "alpha");
        assert_eq!(merged.turns[2].body, "beta");
    }

    #[test]
    fn leading_image_still_merges_later_plain_text() {
        let rows = vec![
            QueuedRow::plain_with_images("alpha"),
            QueuedRow::plain("beta"),
            QueuedRow::plain("gamma"),
        ];
        let merged = merge_plain_queued_follow_ups(&rows);
        assert_eq!(merged.rows, rows);
        assert_eq!(merged.turns.len(), 1);
        assert!(merged.turns[0].has_images);
        assert_eq!(merged.turns[0].body, "alpha\n\nbeta\n\ngamma");
        assert_eq!(merged.turns[0].source, rows);
    }
}
