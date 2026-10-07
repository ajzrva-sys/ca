//! Local branch navigation and explicit history anchors. These records never
//! authorize tools, enqueue work, or replace canonical conversation messages.
use crate::app::Message;
use crate::session::{SessionFamilyMetadata, SessionV4};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use uuid::Uuid;

pub const MAX_BOOKMARKS: usize = 128;
pub const MAX_BOOKMARK_LABEL_BYTES: usize = 256;
pub const MAX_HANDOFFS: usize = 16;
pub const MAX_HANDOFF_SUMMARY_BYTES: usize = 8 * 1024;
pub const MAX_HANDOFF_LEDGER_BYTES: usize = 128 * 1024;
pub const MAX_FAMILY_PAGE_SIZE: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FamilyRelation {
    Current,
    Ancestor,
    Descendant,
    Sibling,
    Relative,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FamilyIssue {
    DuplicateIdentity(Uuid),
    MissingParent { child: Uuid, parent: Uuid },
    Cycle(Uuid),
}

#[derive(Debug, Clone)]
pub struct FamilyRow {
    pub metadata: SessionFamilyMetadata,
    pub depth: usize,
    pub relation: FamilyRelation,
}

#[derive(Debug, Clone)]
pub struct FamilyPage {
    pub rows: Vec<FamilyRow>,
    pub issues: Vec<FamilyIssue>,
    pub total: usize,
    pub offset: usize,
    pub next_offset: Option<usize>,
}

/// Deterministic, paginated view of the connected local family, including
/// archived relatives. Duplicate identities choose the active, newest record.
pub fn family_page(
    metadata: &[SessionFamilyMetadata],
    current: Uuid,
    offset: usize,
    limit: usize,
) -> FamilyPage {
    let mut nodes = BTreeMap::<Uuid, SessionFamilyMetadata>::new();
    let mut duplicates = BTreeSet::new();
    for node in metadata {
        let id = node.summary.id;
        if let Some(previous) = nodes.get(&id) {
            duplicates.insert(id);
            let priority = |n: &SessionFamilyMetadata| {
                (
                    !n.archived,
                    n.summary.updated_at,
                    n.summary.storage_key.clone(),
                )
            };
            if priority(node) <= priority(previous) {
                continue;
            }
        }
        nodes.insert(id, node.clone());
    }
    let mut links = BTreeMap::<Uuid, BTreeSet<Uuid>>::new();
    for (id, node) in &nodes {
        if let Some(parent) = &node.parent {
            links.entry(*id).or_default().insert(parent.session_id);
            links.entry(parent.session_id).or_default().insert(*id);
        }
    }
    let mut family = BTreeSet::new();
    let mut pending = vec![current];
    while let Some(id) = pending.pop() {
        if family.insert(id) {
            pending.extend(links.get(&id).into_iter().flatten().copied());
        }
    }
    let mut issues: Vec<_> = duplicates
        .into_iter()
        .filter(|id| family.contains(id))
        .map(FamilyIssue::DuplicateIdentity)
        .collect();
    let mut cycle_members = BTreeSet::new();
    for id in family.iter().filter(|id| nodes.contains_key(id)) {
        let mut chain = Vec::new();
        let mut at = *id;
        while let Some(node) = nodes.get(&at) {
            if let Some(position) = chain.iter().position(|previous| *previous == at) {
                cycle_members.extend(chain[position..].iter().copied());
                break;
            }
            chain.push(at);
            let Some(parent) = &node.parent else {
                break;
            };
            at = parent.session_id;
        }
        if let Some(parent) = &nodes[id].parent {
            if !nodes.contains_key(&parent.session_id) {
                issues.push(FamilyIssue::MissingParent {
                    child: *id,
                    parent: parent.session_id,
                });
            }
        }
    }
    issues.extend(cycle_members.iter().copied().map(FamilyIssue::Cycle));
    let ancestors = |start: Uuid| {
        let mut seen = HashSet::new();
        let mut at = start;
        while let Some(parent) = nodes.get(&at).and_then(|n| n.parent.as_ref()) {
            if !seen.insert(parent.session_id) {
                break;
            }
            at = parent.session_id;
        }
        seen
    };
    let current_ancestors = ancestors(current);
    let current_parent = nodes
        .get(&current)
        .and_then(|n| n.parent.as_ref())
        .map(|p| p.session_id);
    let mut children = BTreeMap::<Uuid, Vec<Uuid>>::new();
    let mut roots = Vec::new();
    for id in family.iter().filter(|id| nodes.contains_key(id)) {
        match nodes[id].parent.as_ref() {
            Some(parent)
                if nodes.contains_key(&parent.session_id) && !cycle_members.contains(id) =>
            {
                children.entry(parent.session_id).or_default().push(*id)
            }
            _ => roots.push(*id),
        }
    }
    let ordering = |left: &Uuid, right: &Uuid| {
        nodes[left]
            .parent
            .as_ref()
            .map(|p| p.branch_turn)
            .cmp(&nodes[right].parent.as_ref().map(|p| p.branch_turn))
            .then_with(|| nodes[left].created_at.cmp(&nodes[right].created_at))
            .then_with(|| left.cmp(right))
    };
    roots.sort_by(ordering);
    for ids in children.values_mut() {
        ids.sort_by(ordering);
    }
    let mut ordered = Vec::new();
    let mut visited = HashSet::new();
    let mut stack: Vec<_> = roots.into_iter().rev().map(|id| (id, 0)).collect();
    while let Some((id, depth)) = stack.pop() {
        if !visited.insert(id) {
            continue;
        }
        ordered.push((id, depth));
        stack.extend(
            children
                .get(&id)
                .into_iter()
                .flatten()
                .rev()
                .map(|child| (*child, depth + 1)),
        );
    }
    let total = ordered.len();
    let offset = offset.min(total);
    let end = offset
        .saturating_add(limit.clamp(1, MAX_FAMILY_PAGE_SIZE))
        .min(total);
    let rows = ordered[offset..end]
        .iter()
        .map(|(id, depth)| {
            let node = &nodes[id];
            let relation = if *id == current {
                FamilyRelation::Current
            } else if current_ancestors.contains(id) {
                FamilyRelation::Ancestor
            } else if ancestors(*id).contains(&current) {
                FamilyRelation::Descendant
            } else if current_parent.is_some()
                && node.parent.as_ref().map(|p| p.session_id) == current_parent
            {
                FamilyRelation::Sibling
            } else {
                FamilyRelation::Relative
            };
            FamilyRow {
                metadata: node.clone(),
                depth: *depth,
                relation,
            }
        })
        .collect();
    FamilyPage {
        rows,
        issues,
        total,
        offset,
        next_offset: (end < total).then_some(end),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionBookmark {
    pub id: Uuid,
    pub label: String,
    pub through_message_count: usize,
    pub prefix_sha256: String,
    pub created_at: DateTime<Utc>,
}

impl SessionBookmark {
    pub fn matches_history(&self, messages: &[Message]) -> bool {
        crate::session::canonical_prefix_sha256(messages, self.through_message_count)
            .is_ok_and(|digest| digest == self.prefix_sha256)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionBookmarks {
    pub entries: Vec<SessionBookmark>,
}

impl SessionBookmarks {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        let mut labels = HashSet::new();
        if self.entries.len() > MAX_BOOKMARKS
            || self.entries.iter().any(|entry| {
                !ids.insert(entry.id)
                    || !labels.insert(&entry.label)
                    || !valid_label(&entry.label)
                    || !valid_digest(&entry.prefix_sha256)
            })
        {
            return Err("Bookmarks exceed their bounded identity and label contract.".into());
        }
        Ok(())
    }
    pub fn add(
        &mut self,
        messages: &[Message],
        through_message_count: usize,
        label: impl Into<String>,
    ) -> Result<Uuid, String> {
        let label = label.into().trim().to_owned();
        if !valid_boundary(messages, through_message_count) {
            return Err("Bookmarks require a completed turn boundary.".into());
        }
        let entry = SessionBookmark {
            id: Uuid::new_v4(),
            label,
            through_message_count,
            prefix_sha256: crate::session::canonical_prefix_sha256(messages, through_message_count)
                .map_err(|e| e.to_string())?,
            created_at: Utc::now(),
        };
        let id = entry.id;
        let mut next = self.clone();
        next.entries.push(entry);
        next.validate()?;
        *self = next;
        Ok(id)
    }
    pub fn rename(&mut self, id: Uuid, label: impl Into<String>) -> Result<(), String> {
        let mut next = self.clone();
        let entry = next
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or("Unknown bookmark.")?;
        entry.label = label.into().trim().to_owned();
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn remove(&mut self, id: Uuid) -> Result<(), String> {
        let old = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        if self.entries.len() == old {
            return Err("Unknown bookmark.".into());
        }
        Ok(())
    }
    pub fn retained_prefix(&self, messages: &[Message], through_message_count: usize) -> Self {
        Self {
            entries: self
                .entries
                .iter()
                .filter(|entry| {
                    entry.through_message_count <= through_message_count
                        && entry.matches_history(messages)
                })
                .cloned()
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BranchHandoff {
    pub id: Uuid,
    pub source_session_id: Uuid,
    pub target_session_id: Uuid,
    pub through_message_count: usize,
    pub source_prefix_sha256: String,
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BranchHandoffLedger {
    pub entries: Vec<BranchHandoff>,
}

impl BranchHandoffLedger {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn validate(&self, target: Uuid) -> Result<(), String> {
        let mut ids = HashSet::new();
        if self.entries.len() > MAX_HANDOFFS
            || self.entries.iter().any(|entry| {
                !ids.insert(entry.id)
                    || entry.target_session_id != target
                    || entry.source_session_id == target
                    || !valid_digest(&entry.source_prefix_sha256)
                    || entry.summary.trim().is_empty()
                    || entry.summary.len() > MAX_HANDOFF_SUMMARY_BYTES
            })
        {
            return Err("Branch handoff provenance exceeds its bounds.".into());
        }
        if serde_json::to_vec(self)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_HANDOFF_LEDGER_BYTES
        {
            return Err("Branch handoff provenance exceeds its metadata bound.".into());
        }
        Ok(())
    }
    pub fn record(
        &mut self,
        source: &SessionV4,
        target: Uuid,
        summary: String,
    ) -> Result<Uuid, String> {
        if !valid_boundary(&source.messages, source.messages.len()) {
            return Err("Branch handoffs require a completed source turn.".into());
        }
        let entry = BranchHandoff {
            id: Uuid::new_v4(),
            source_session_id: source.id,
            target_session_id: target,
            through_message_count: source.messages.len(),
            source_prefix_sha256: crate::session::canonical_prefix_sha256(
                &source.messages,
                source.messages.len(),
            )
            .map_err(|e| e.to_string())?,
            summary,
            created_at: Utc::now(),
        };
        let id = entry.id;
        let mut next = self.clone();
        next.entries.push(entry);
        if next.entries.len() > MAX_HANDOFFS {
            next.entries.remove(0);
        }
        next.validate(target)?;
        *self = next;
        Ok(id)
    }
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
fn valid_label(label: &str) -> bool {
    !label.trim().is_empty()
        && label.len() <= MAX_BOOKMARK_LABEL_BYTES
        && !label.chars().any(char::is_control)
}
fn valid_boundary(messages: &[Message], count: usize) -> bool {
    count == 0
        || crate::session::completed_turns(messages)
            .iter()
            .any(|turn| turn.through_message_count == count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Role;
    use crate::session::{SessionParent, SessionSummary};

    fn message(role: Role, content: &str, second: i64) -> Message {
        Message {
            role,
            content: content.into(),
            tool_calls: None,
            thinking: None,
            timestamp: DateTime::from_timestamp(second, 0).unwrap(),
            response_items: Vec::new(),
        }
    }
    fn history() -> Vec<Message> {
        vec![
            message(Role::User, "first", 0),
            message(Role::Assistant, "answer", 1),
            message(Role::User, "second", 2),
            message(Role::Assistant, "answer two", 3),
        ]
    }
    fn node(id: u128, parent: Option<u128>, archived: bool) -> SessionFamilyMetadata {
        let id = Uuid::from_u128(id);
        SessionFamilyMetadata {
            summary: SessionSummary {
                storage_key: id.to_string(),
                id,
                title: format!("{id}"),
                pinned: false,
                workspace: "/workspace".into(),
                updated_at: DateTime::from_timestamp(0, 0).unwrap(),
            },
            parent: parent.map(|id| SessionParent {
                session_id: Uuid::from_u128(id),
                branch_turn: 1,
            }),
            archived,
            created_at: DateTime::from_timestamp(0, 0).unwrap(),
        }
    }

    #[test]
    fn family_includes_archived_ancestors_siblings_and_paginated_descendants() {
        let metadata = vec![
            node(4, Some(2), false),
            node(9, None, false),
            node(3, Some(1), false),
            node(1, None, true),
            node(2, Some(1), false),
        ];
        let page = family_page(&metadata, Uuid::from_u128(2), 0, 2);
        assert_eq!(page.total, 4);
        assert_eq!(page.next_offset, Some(2));
        assert!(page.rows[0].metadata.archived);
        assert_eq!(page.rows[0].relation, FamilyRelation::Ancestor);
        assert_eq!(page.rows[1].relation, FamilyRelation::Current);
        let next = family_page(&metadata, Uuid::from_u128(2), 2, 1000);
        assert_eq!(next.rows[0].relation, FamilyRelation::Descendant);
        assert_eq!(next.rows[1].relation, FamilyRelation::Sibling);
        assert!(next.next_offset.is_none());
        assert!(next.issues.is_empty());
        assert!(family_page(&metadata, Uuid::from_u128(2), usize::MAX, 0)
            .rows
            .is_empty());
    }

    #[test]
    fn family_reports_cycles_duplicate_identities_and_missing_parents_deterministically() {
        let cycle = family_page(
            &[
                node(1, Some(2), false),
                node(2, Some(1), false),
                node(1, Some(2), true),
            ],
            Uuid::from_u128(1),
            0,
            10,
        );
        assert_eq!(cycle.total, 2);
        assert_eq!(
            cycle
                .issues
                .iter()
                .filter(|i| matches!(i, FamilyIssue::Cycle(_)))
                .count(),
            2
        );
        assert!(cycle
            .issues
            .contains(&FamilyIssue::DuplicateIdentity(Uuid::from_u128(1))));
        assert!(
            !cycle
                .rows
                .iter()
                .find(|r| r.metadata.summary.id == Uuid::from_u128(1))
                .unwrap()
                .metadata
                .archived
        );
        let missing = family_page(
            &[node(2, Some(1), false), node(3, Some(1), true)],
            Uuid::from_u128(2),
            0,
            10,
        );
        assert_eq!(missing.total, 2);
        assert_eq!(missing.issues.len(), 2);
        assert_eq!(missing.rows[1].relation, FamilyRelation::Sibling);
    }

    #[test]
    fn bookmarks_anchor_exact_completed_prefix_and_forks_prune_invalid_or_later_anchors() {
        let messages = history();
        let mut source = SessionV4::new("source", "/workspace", "provider", "model", messages);
        let early = source
            .bookmarks
            .add(&source.messages, 2, "First answer")
            .unwrap();
        source
            .bookmarks
            .add(&source.messages, 4, "Second answer")
            .unwrap();
        assert!(source
            .bookmarks
            .add(&source.messages, 1, "Partial turn")
            .is_err());
        let branch = crate::session::branch_in_memory(&source, 2, 1, "fork").unwrap();
        assert_eq!(branch.bookmarks.entries.len(), 1);
        assert_eq!(branch.bookmarks.entries[0].id, early);
        assert!(branch.bookmarks.entries[0].matches_history(&branch.messages));
        source.messages[0].content = "changed".into();
        assert!(!source.bookmarks.entries[0].matches_history(&source.messages));
        assert!(crate::session::branch_in_memory(&source, 2, 1, "fork")
            .unwrap()
            .bookmarks
            .is_empty());
    }

    #[test]
    fn bookmark_labels_and_handoff_provenance_are_bounded_and_target_specific() {
        let messages = history();
        let mut marks = SessionBookmarks::default();
        let id = marks.add(&messages, 2, "a").unwrap();
        assert!(marks.rename(id, "é".repeat(129)).is_err());
        assert!(marks.add(&messages, 2, "a").is_err());
        marks.rename(id, "renamed").unwrap();
        marks.remove(id).unwrap();
        for n in 0..MAX_BOOKMARKS {
            marks.add(&messages, 2, format!("{n}")).unwrap();
        }
        assert!(marks.add(&messages, 2, "overflow").is_err());
        let source = SessionV4::new("source", "/workspace", "provider", "model", messages);
        let target = Uuid::new_v4();
        let mut handoffs = BranchHandoffLedger::default();
        for n in 0..=MAX_HANDOFFS {
            handoffs
                .record(&source, target, format!("summary {n}"))
                .unwrap();
        }
        assert_eq!(handoffs.entries.len(), MAX_HANDOFFS);
        assert!(handoffs.validate(Uuid::new_v4()).is_err());
        assert!(handoffs
            .record(&source, target, "x".repeat(MAX_HANDOFF_SUMMARY_BYTES + 1))
            .is_err());
        assert!(handoffs
            .record(&source, source.id, "invalid self summary".into())
            .is_err());
    }
}
