use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingInput {
    pub id: Uuid,
    pub text: String,
    pub created_at: DateTime<Utc>,
}

impl PendingInput {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            text: text.into(),
            created_at: Utc::now(),
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
        let text = text.into();
        if text.trim().is_empty() {
            return None;
        }
        let item = PendingInput::new(text);
        let id = item.id;
        self.items.push_back(item);
        Some(id)
    }

    pub fn enqueue_with_id(&mut self, id: Uuid, text: impl Into<String>) {
        if self.items.iter().any(|item| item.id == id) {
            return;
        }
        self.items.push_back(PendingInput {
            id,
            text: text.into(),
            created_at: DateTime::from_timestamp(0, 0).unwrap(),
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
}
