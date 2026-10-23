//! Bounded, process-only draft history. Large paste and image data stay shared.
use super::{AtomicKind, HistoryDraft};
use sha2::{Digest, Sha256};
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

const MAX_ENTRIES: usize = 64;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
// Includes shared paste/image payloads once; metadata has its own tighter cap.
const MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Typing,
    Backspace,
    Delete,
    Discrete,
}

#[derive(Default)]
pub struct ComposerHistory {
    undo: VecDeque<HistoryDraft>,
    redo: VecDeque<HistoryDraft>,
    expected: Option<[u8; 32]>,
    group: Option<(EditKind, Instant, usize)>,
    revision: u64,
}

impl std::fmt::Debug for ComposerHistory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComposerHistory")
            .field("undo_entries", &self.undo.len())
            .field("redo_entries", &self.redo.len())
            .field("retained_bytes", &self.retained_bytes().1)
            .finish()
    }
}

fn stamp(draft: &HistoryDraft) -> [u8; 32] {
    let mut hash = Sha256::new();
    fn text(hash: &mut Sha256, value: &str) {
        hash.update(value.len().to_le_bytes());
        hash.update(value.as_bytes());
    }
    text(&mut hash, &draft.text);
    hash.update(draft.attachments.len().to_le_bytes());
    for attachment in &draft.attachments {
        text(&mut hash, &attachment.reference);
        match &attachment.kind {
            crate::attachments::ComposerAttachmentKind::File { root_id, relative } => {
                text(&mut hash, "file");
                text(&mut hash, root_id);
                text(&mut hash, relative);
            }
            crate::attachments::ComposerAttachmentKind::Mcp { server, uri, name } => {
                text(&mut hash, "mcp");
                text(&mut hash, server);
                text(&mut hash, uri);
                text(&mut hash, name);
            }
        }
    }
    hash.update(draft.atomic_elements.len().to_le_bytes());
    for element in &draft.atomic_elements {
        hash.update(element.range.start.to_le_bytes());
        hash.update(element.range.end.to_le_bytes());
        match &element.kind {
            AtomicKind::Reference => hash.update([0]),
            AtomicKind::File => hash.update([1]),
            AtomicKind::Folder { outside_workspace } => {
                hash.update([2, u8::from(*outside_workspace)])
            }
            AtomicKind::LargePaste { payload } => {
                hash.update([3]);
                hash.update((payload.as_ptr() as usize).to_le_bytes());
                hash.update(payload.len().to_le_bytes());
            }
        }
    }
    hash.update(draft.images.len().to_le_bytes());
    for image in &draft.images {
        hash.update((image.png_bytes().as_ptr() as usize).to_le_bytes());
        hash.update(image.encoded_len().to_le_bytes());
        hash.update(image.width().to_le_bytes());
        hash.update(image.height().to_le_bytes());
    }
    hash.finalize().into()
}

impl ComposerHistory {
    pub fn separate_actions(&mut self) {
        self.group = None;
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn reset(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.expected = None;
        self.group = None;
        self.revision = self.revision.wrapping_add(1);
    }

    /// Untracked replacements start a fresh history instead of reviving stale work.
    pub fn observe(&mut self, draft: &HistoryDraft) {
        let current = stamp(draft);
        if self.expected.is_some_and(|expected| expected != current) {
            self.reset();
        }
        self.expected = Some(current);
    }

    pub fn record(&mut self, before: HistoryDraft, after: &HistoryDraft, kind: EditKind) {
        self.record_at(before, after, kind, Instant::now());
    }

    fn record_at(
        &mut self,
        before: HistoryDraft,
        after: &HistoryDraft,
        kind: EditKind,
        now: Instant,
    ) {
        self.observe(&before);
        let next = stamp(after);
        if self.expected == Some(next) {
            if before.cursor != after.cursor || kind == EditKind::Discrete {
                self.group = None;
            }
            return;
        }
        let coalesce = kind != EditKind::Discrete
            && self.group.is_some_and(|(previous, at, cursor)| {
                previous == kind
                    && cursor == before.cursor
                    && now.saturating_duration_since(at) < Duration::from_secs(1)
            });
        if !coalesce {
            self.undo.push_back(before);
        }
        self.redo.clear();
        self.expected = Some(next);
        self.group = Some((kind, now, after.cursor));
        self.trim();
    }

    pub fn undo(&mut self, current: HistoryDraft) -> Option<HistoryDraft> {
        self.observe(&current);
        self.group = None;
        let previous = self.undo.pop_back()?;
        self.redo.push_back(current);
        self.expected = Some(stamp(&previous));
        self.revision = self.revision.wrapping_add(1);
        self.trim();
        Some(previous)
    }

    pub fn redo(&mut self, current: HistoryDraft) -> Option<HistoryDraft> {
        self.observe(&current);
        self.group = None;
        let next = self.redo.pop_back()?;
        self.undo.push_back(current);
        self.expected = Some(stamp(&next));
        self.revision = self.revision.wrapping_add(1);
        self.trim();
        Some(next)
    }

    /// Text/metadata ownership and total ownership, counting shared payloads once.
    fn retained_bytes(&self) -> (usize, usize) {
        let mut text = 0_usize;
        let mut payload = 0_usize;
        let mut shared = HashSet::new();
        for draft in self.undo.iter().chain(&self.redo) {
            text = text.saturating_add(draft.text.len());
            for attachment in &draft.attachments {
                text = text.saturating_add(attachment.reference.len());
                text = text.saturating_add(match &attachment.kind {
                    crate::attachments::ComposerAttachmentKind::File { root_id, relative } => {
                        root_id.len() + relative.len()
                    }
                    crate::attachments::ComposerAttachmentKind::Mcp { server, uri, name } => {
                        server.len() + uri.len() + name.len()
                    }
                });
            }
            for element in &draft.atomic_elements {
                text = text.saturating_add(std::mem::size_of_val(element));
                if let AtomicKind::LargePaste { payload: value } = &element.kind {
                    if shared.insert((value.as_ptr() as usize, value.len())) {
                        payload = payload.saturating_add(value.len());
                    }
                }
            }
            for image in &draft.images {
                text = text.saturating_add(std::mem::size_of_val(image));
                if shared.insert((image.png_bytes().as_ptr() as usize, image.encoded_len())) {
                    payload = payload.saturating_add(image.encoded_len());
                }
            }
        }
        (text, text.saturating_add(payload))
    }

    fn trim(&mut self) {
        loop {
            let (text, total) = self.retained_bytes();
            if self.undo.len() + self.redo.len() <= MAX_ENTRIES
                && text <= MAX_TEXT_BYTES
                && total <= MAX_RETAINED_BYTES
            {
                break;
            }
            if self.undo.pop_front().is_none() && self.redo.pop_front().is_none() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(text: &str) -> HistoryDraft {
        HistoryDraft {
            text: text.into(),
            cursor: text.len(),
            atomic_elements: vec![],
            attachments: vec![],
            images: vec![],
        }
    }

    #[test]
    fn composer_history_groups_typing_until_cursor_action_or_timeout() {
        let mut history = ComposerHistory::default();
        let now = Instant::now();
        history.record_at(draft(""), &draft("a"), EditKind::Typing, now);
        history.record_at(
            draft("a"),
            &draft("ab"),
            EditKind::Typing,
            now + Duration::from_millis(10),
        );
        assert_eq!(history.undo(draft("ab")).unwrap().text, "");
        assert_eq!(history.redo(draft("")).unwrap().text, "ab");
        history.record_at(
            draft("ab"),
            &draft("abc"),
            EditKind::Typing,
            now + Duration::from_secs(2),
        );
        assert_eq!(history.undo(draft("abc")).unwrap().text, "ab");
        history.separate_actions();
        let before = HistoryDraft {
            cursor: 1,
            ..draft("ab")
        };
        let after = HistoryDraft {
            cursor: 2,
            ..draft("aXb")
        };
        history.record_at(
            before,
            &after,
            EditKind::Typing,
            now + Duration::from_secs(3),
        );
        let previous = history.undo(after).unwrap();
        assert_eq!((previous.text.as_str(), previous.cursor), ("ab", 1));
    }

    #[test]
    fn composer_history_is_bounded_and_debug_omits_draft_payloads() {
        let mut history = ComposerHistory::default();
        let mut current = draft("private draft");
        for index in 0..MAX_ENTRIES + 20 {
            let next = draft(&format!("private draft {index}"));
            history.record(current, &next, EditKind::Discrete);
            current = next;
        }
        assert!(history.undo.len() <= MAX_ENTRIES);
        assert!(!format!("{history:?}").contains("private draft"));
        let oversized = draft(&"x".repeat(MAX_TEXT_BYTES + 1));
        history.reset();
        history.record(oversized, &draft("small"), EditKind::Discrete);
        assert!(history.undo(draft("small")).is_none());
        assert_eq!(history.retained_bytes(), (0, 0));
    }

    #[test]
    fn composer_history_counts_shared_payload_once_and_clears_redo_on_new_edit() {
        let mut history = ComposerHistory::default();
        let payload: std::sync::Arc<str> = "private payload".repeat(100_000).into();
        let mut before = draft("[paste]");
        before.atomic_elements.push(super::super::AtomicElement {
            range: 0..7,
            kind: AtomicKind::LargePaste {
                payload: payload.clone(),
            },
        });
        let mut after = before.clone();
        after.text.push('1');
        history.record(before.clone(), &after, EditKind::Discrete);
        let mut next = after.clone();
        next.text.push('2');
        history.record(after.clone(), &next, EditKind::Discrete);
        let (metadata, total) = history.retained_bytes();
        assert_eq!(total - metadata, payload.len());
        assert_eq!(history.undo(next).unwrap(), after);
        let replacement = draft("replacement");
        history.record(after, &replacement, EditKind::Discrete);
        assert!(history.redo(replacement).is_none());
    }
}
