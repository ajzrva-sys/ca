use crate::config::ReasoningEffort;
use crate::context::estimate_text_tokens;
use crate::context_pruning::PruningStats;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const MAX_LOCAL: usize = 8;
const MAX_CONSTRAINTS: usize = 16;
const MAX_PATHS: usize = 32;
const MAX_QUESTIONS: usize = 8;
const MAX_QUERIES: usize = 32;
const MAX_SLOT_BYTES: usize = 2 * 1024;
const MAX_CONSTRAINT_BYTES: usize = 4 * 1024;
const MAX_QUESTION_BYTES: usize = 512;
const MAX_LOCAL_BYTES: usize = 2 * 1024;
const MAX_CITATION_BYTES: usize = 512;
const MAX_INDEX_BODY: usize = 64 * 1024;
const MAX_STAT_BYTES: usize = 1024 * 1024;
const PASS1_ALPHA: u16 = 250;
const OPEN_ALPHA: u16 = 1000;
pub const INSTRUCTION: &str = "End a pass with a line LOOP_DELTA and one JSON object. Fields are goal, constraints, drop_constraint_ids, paths, questions, and local. The host keeps constraint ids and caps each write.";
const GLOBAL_MARK: &str = "\n## Loop global\n";
const WINDOW_MARK: &str = "Loop window\n";
const REUSED_MARK: &str = "reused_index: true\n";

tokio::task_local! {
    static ACTIVE_INDEX: Arc<Mutex<AttentionIndex>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathStat {
    pub mtime: u64,
    pub size: u64,
    pub content_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Constraint {
    pub id: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathFact {
    pub path: String,
    pub content_hash: String,
    pub citation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct GlobalState {
    pub goal: String,
    pub constraints: Vec<Constraint>,
    pub paths: Vec<PathFact>,
    pub questions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalDelta {
    pub kind: String,
    pub text: String,
    pub path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LoopState {
    pub global: GlobalState,
    pub local: VecDeque<LocalDelta>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeltaBody {
    #[serde(default)]
    goal: Option<String>,
    #[serde(default)]
    constraints: Vec<DeltaConstraint>,
    #[serde(default)]
    drop_constraint_ids: Vec<String>,
    #[serde(default)]
    paths: Vec<DeltaPath>,
    #[serde(default)]
    questions: Vec<String>,
    #[serde(default)]
    local: Vec<DeltaLocal>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeltaConstraint {
    id: String,
    text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeltaPath {
    path: String,
    content_hash: String,
    citation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeltaLocal {
    kind: String,
    text: String,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeCandidate {
    pub goal: Option<String>,
    pub constraints: Vec<Constraint>,
    pub drop_constraint_ids: Vec<String>,
    pub paths: Vec<PathFact>,
    pub questions: Vec<String>,
    pub local: Vec<LocalDelta>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MergeReject {
    Schema,
    Oversize,
    ConstraintDeleted,
    LowOverlap,
    ExtraPassGlobal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeOutcome {
    pub state: LoopState,
    pub rejected: Vec<MergeReject>,
    pub changed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ChainKind {
    #[default]
    Long,
    Short,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnFacts {
    pub pass: u8,
    pub single_known_file: bool,
    pub status_or_slash: bool,
    pub index_answered: bool,
    pub retry_has_new_fact: bool,
    pub verification_output_changed: bool,
    pub cross_module: bool,
    pub conflicting: bool,
    pub review_high: bool,
    pub merge_conflict: bool,
    pub no_file_continuations: u32,
    pub checkpoint_or_pruning: bool,
    pub loop_guard_seen: bool,
}

impl Default for TurnFacts {
    fn default() -> Self {
        Self {
            pass: 1,
            single_known_file: false,
            status_or_slash: false,
            index_answered: false,
            retry_has_new_fact: false,
            verification_output_changed: false,
            cross_module: false,
            conflicting: false,
            review_high: false,
            merge_conflict: false,
            no_file_continuations: 0,
            checkpoint_or_pruning: false,
            loop_guard_seen: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthDecision {
    Continue,
    Done,
    StopUnchanged,
    PauseAfterCap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptItem {
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopPreface {
    pub prefix: String,
    pub window: String,
    pub stats: PruningStats,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedPath {
    pub path: String,
    pub stat: PathStat,
    pub citation: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IndexHit {
    body: String,
    paths: Vec<IndexedPath>,
}

#[derive(Clone, Debug, Default)]
pub struct AttentionIndex {
    order: VecDeque<String>,
    hits: HashMap<String, IndexHit>,
}

#[derive(Clone, Debug)]
pub struct Session {
    state: LoopState,
    merged: bool,
    pass: u8,
    hard: bool,
    facts: TurnFacts,
    index: Arc<Mutex<AttentionIndex>>,
    last_hash: [u8; 32],
    last_worktree: u64,
    worktree: u64,
    last_rejects: Vec<MergeReject>,
    index_answered: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            state: LoopState::default(),
            merged: false,
            pass: 0,
            hard: false,
            facts: TurnFacts::default(),
            index: Arc::new(Mutex::new(AttentionIndex::default())),
            last_hash: [0; 32],
            last_worktree: 0,
            worktree: 0,
            last_rejects: Vec::new(),
            index_answered: false,
        }
    }
}

pub fn alpha_milli(pass: u8, hard: bool) -> u16 {
    if pass >= 2 || hard {
        OPEN_ALPHA
    } else {
        PASS1_ALPHA
    }
}

pub fn classify(facts: &TurnFacts) -> ChainKind {
    if facts.loop_guard_seen
        || facts.verification_output_changed
        || facts.review_high
        || facts.merge_conflict
        || facts.conflicting
        || facts.cross_module
        || facts.no_file_continuations > 0
        || facts.checkpoint_or_pruning
    {
        return ChainKind::Long;
    }
    if facts.status_or_slash
        || facts.index_answered
        || facts.single_known_file
        || facts.retry_has_new_fact
    {
        return ChainKind::Short;
    }
    if facts.pass <= 1 {
        ChainKind::Long
    } else {
        ChainKind::Short
    }
}

pub fn runs_second_pass(facts: &TurnFacts) -> bool {
    if facts.status_or_slash || facts.index_answered {
        return false;
    }
    if facts.single_known_file
        && !facts.verification_output_changed
        && !facts.review_high
        && !facts.merge_conflict
        && !facts.cross_module
        && !facts.conflicting
        && facts.no_file_continuations == 0
        && !facts.checkpoint_or_pruning
    {
        return false;
    }
    classify(facts) == ChainKind::Long
}

pub fn effort_for_chain(
    enabled: bool,
    chain: ChainKind,
    selected: Option<ReasoningEffort>,
    allowed: &[ReasoningEffort],
) -> Option<ReasoningEffort> {
    if !enabled || chain == ChainKind::Long {
        return selected;
    }
    if allowed.contains(&ReasoningEffort::Low) {
        Some(ReasoningEffort::Low)
    } else if allowed.contains(&ReasoningEffort::Minimal) {
        Some(ReasoningEffort::Minimal)
    } else if allowed.contains(&ReasoningEffort::None) {
        Some(ReasoningEffort::None)
    } else {
        None
    }
}

pub fn is_read_tool(name: &str) -> bool {
    matches!(
        name,
        "read_file"
            | "list_dir"
            | "glob"
            | "search_files"
            | "lsp_definition"
            | "lsp_references"
            | "lsp_hover"
            | "lsp_implementation"
            | "lsp_call_hierarchy"
    )
}

pub fn project_preface(
    task: &str,
    state: &LoopState,
    transcript: &[TranscriptItem],
    latest_batch: &str,
) -> LoopPreface {
    let prefix = render_prefix(task, &state.global);
    let window = render_window(&state.global, &state.local, latest_batch);
    let transcript_tokens = transcript.iter().fold(0_u64, |sum, item| {
        sum.saturating_add(estimate_text_tokens(&item.body))
    });
    let window_tokens = estimate_text_tokens(&window);
    LoopPreface {
        prefix: prefix.clone(),
        window,
        stats: PruningStats {
            results: state.local.len(),
            saved_tokens: transcript_tokens.saturating_sub(window_tokens),
            added_tokens: estimate_text_tokens(&prefix).saturating_add(window_tokens),
        },
    }
}

pub fn install_strings(stable: &mut String, transient: &mut Vec<String>, preface: &LoopPreface) {
    if !stable.contains(GLOBAL_MARK) && !preface.prefix.is_empty() {
        stable.push_str(GLOBAL_MARK);
        stable.push_str(&preface.prefix);
    }
    transient.retain(|item| !item.starts_with(WINDOW_MARK));
    let mut window = String::from(WINDOW_MARK);
    window.push_str(&preface.window);
    transient.push(window);
}

pub fn install_instruction(stable: &mut String) {
    const MARK: &str = "\n## Loop refine\n";
    if !stable.contains(MARK) {
        stable.push_str(MARK);
        stable.push_str(INSTRUCTION);
    }
}

pub async fn scope_index<T>(
    index: Arc<Mutex<AttentionIndex>>,
    future: impl std::future::Future<Output = T>,
) -> T {
    ACTIVE_INDEX.scope(index, future).await
}

pub fn reuse_indexed(name: &str, arguments: &Value, cwd: &Path) -> Option<String> {
    let index = ACTIVE_INDEX.try_with(Arc::clone).ok()?;
    let mut guard = index.lock().unwrap_or_else(|poison| poison.into_inner());
    guard.reuse(name, &arguments.to_string(), &|path| live_stat(cwd, path))
}

pub fn store_indexed(name: &str, arguments: &Value, success: bool, body: &str, cwd: &Path) {
    let Ok(index) = ACTIVE_INDEX.try_with(Arc::clone) else {
        return;
    };
    let mut guard = index.lock().unwrap_or_else(|poison| poison.into_inner());
    guard.observe(name, arguments, success, body, cwd);
}

impl Session {
    pub fn index_handle(&self) -> Arc<Mutex<AttentionIndex>> {
        Arc::clone(&self.index)
    }

    pub fn pass(&self) -> u8 {
        self.pass
    }

    pub fn chain(&self) -> ChainKind {
        classify(&self.facts)
    }

    pub fn index_answered(&self) -> bool {
        self.index_answered
    }

    pub fn has_local_fact(&self) -> bool {
        self.state
            .local
            .iter()
            .any(|delta| delta.kind == "fact" && !delta.text.is_empty())
    }

    pub fn state(&self) -> &LoopState {
        &self.state
    }

    pub fn last_rejects(&self) -> &[MergeReject] {
        &self.last_rejects
    }

    pub fn begin_user_message(&mut self) {
        *self = Self {
            index: Arc::clone(&self.index),
            ..Self::default()
        };
        self.index
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clear();
    }

    pub fn note_facts(&mut self, facts: &TurnFacts) {
        if facts.verification_output_changed || facts.review_high || facts.merge_conflict {
            self.hard = true;
        }
        self.facts = facts.clone();
        self.facts.index_answered = self.index_answered;
        self.facts.retry_has_new_fact = self.has_local_fact();
    }

    pub fn set_worktree(&mut self, hash: u64) {
        self.worktree = hash;
    }

    pub fn merge(&mut self, pass: u8, hard: bool, candidate: &MergeCandidate) -> MergeOutcome {
        if hard {
            self.hard = true;
        }
        let outcome = gated_merge(&self.state, pass, hard || self.hard, candidate);
        self.last_rejects = outcome.rejected.clone();
        if outcome.changed {
            self.state = outcome.state.clone();
            self.merged = true;
        } else if outcome.rejected.is_empty() && pass >= 2 {
            self.merged = true;
        }
        outcome
    }

    pub fn merge_json(&mut self, pass: u8, hard: bool, raw: &str) -> MergeOutcome {
        let Some(candidate) = parse_delta(raw) else {
            let outcome = MergeOutcome {
                state: self.state.clone(),
                rejected: vec![MergeReject::Schema],
                changed: false,
            };
            self.last_rejects = outcome.rejected.clone();
            return outcome;
        };
        self.merge(pass, hard, &candidate)
    }

    pub fn absorb(&mut self, content: &str) {
        let Some(raw) = delta_json(content) else {
            return;
        };
        let _ = self.merge_json(self.pass.max(1), self.hard, &raw);
    }

    pub fn preface(&self) -> Option<LoopPreface> {
        self.merged
            .then(|| project_preface("", &self.state, &[], ""))
    }

    pub fn complete_response(&mut self) -> DepthDecision {
        if self.pass == 0 && !self.hard && !runs_second_pass(&self.facts) {
            return DepthDecision::Done;
        }
        self.pass = self.pass.saturating_add(1);
        let hash = state_hash(&self.state);
        let unchanged =
            self.merged && hash == self.last_hash && self.worktree == self.last_worktree;
        self.last_hash = hash;
        self.last_worktree = self.worktree;
        if self.pass >= 2 && unchanged {
            self.reset_depth();
            return DepthDecision::StopUnchanged;
        }
        if self.pass >= 3 {
            self.reset_depth();
            return DepthDecision::PauseAfterCap;
        }
        if self.pass == 2 && !self.hard {
            self.reset_depth();
            return DepthDecision::Done;
        }
        DepthDecision::Continue
    }

    pub fn resolve_query<F>(
        &mut self,
        tool: &str,
        args: &str,
        live: &HashMap<String, PathStat>,
        execute: F,
    ) -> String
    where
        F: FnOnce() -> (String, Vec<IndexedPath>),
    {
        let mut index = self
            .index
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(body) = index.reuse(tool, args, &|path| live.get(path).cloned()) {
            self.index_answered = true;
            return body;
        }
        drop(index);
        let (body, paths) = execute();
        let mut index = self
            .index
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if !body.starts_with("error") {
            index.store(tool, args, &body, paths);
        }
        body
    }

    fn reset_depth(&mut self) {
        self.pass = 0;
        self.hard = false;
    }
}

pub fn gated_merge(
    state: &LoopState,
    pass: u8,
    hard: bool,
    candidate: &MergeCandidate,
) -> MergeOutcome {
    let alpha = alpha_milli(pass, hard);
    let mut next = state.clone();
    let mut rejected = Vec::new();
    let extra = pass >= 3;
    if !candidate.drop_constraint_ids.is_empty() {
        rejected.push(MergeReject::ConstraintDeleted);
    }
    if extra
        && (candidate.goal.is_some()
            || !candidate.constraints.is_empty()
            || !candidate.questions.is_empty())
    {
        rejected.push(MergeReject::ExtraPassGlobal);
    }
    if !extra {
        if let Some(goal) = candidate.goal.as_deref() {
            match write_slot(&next.global.goal, goal, alpha, MAX_SLOT_BYTES) {
                SlotWrite::Keep => {}
                SlotWrite::Write(text) => next.global.goal = text,
                SlotWrite::Reject(reason) => rejected.push(reason),
            }
        }
        for constraint in &candidate.constraints {
            if constraint.id.is_empty() || constraint.id.len() > 64 {
                rejected.push(MergeReject::Schema);
                continue;
            }
            if let Some(existing) = next
                .global
                .constraints
                .iter_mut()
                .find(|item| item.id == constraint.id)
            {
                match write_slot(
                    &existing.text,
                    &constraint.text,
                    alpha,
                    MAX_CONSTRAINT_BYTES,
                ) {
                    SlotWrite::Keep => {}
                    SlotWrite::Write(text) => existing.text = text,
                    SlotWrite::Reject(reason) => rejected.push(reason),
                }
            } else if next.global.constraints.len() >= MAX_CONSTRAINTS {
                rejected.push(MergeReject::Oversize);
            } else {
                match fresh_text(&constraint.text, MAX_CONSTRAINT_BYTES) {
                    Ok(text) => next.global.constraints.push(Constraint {
                        id: constraint.id.clone(),
                        text,
                    }),
                    Err(reason) => rejected.push(reason),
                }
            }
        }
        next.global
            .constraints
            .sort_by(|left, right| left.id.cmp(&right.id));
        for question in &candidate.questions {
            if next.global.questions.iter().any(|item| item == question) {
                continue;
            }
            if next.global.questions.len() >= MAX_QUESTIONS {
                rejected.push(MergeReject::Oversize);
                break;
            }
            match fresh_text(question, MAX_QUESTION_BYTES) {
                Ok(text) => next.global.questions.push(text),
                Err(reason) => rejected.push(reason),
            }
        }
    }
    for fact in &candidate.paths {
        if next.global.paths.len() >= MAX_PATHS
            && !next.global.paths.iter().any(|item| item.path == fact.path)
        {
            rejected.push(MergeReject::Oversize);
            continue;
        }
        let citation = match fresh_text(&fact.citation, MAX_CITATION_BYTES) {
            Ok(text) => text,
            Err(reason) => {
                rejected.push(reason);
                continue;
            }
        };
        let hash = match fresh_text(&fact.content_hash, 128) {
            Ok(text) => text,
            Err(reason) => {
                rejected.push(reason);
                continue;
            }
        };
        if fact.path.is_empty() || fact.path.len() > 512 {
            rejected.push(MergeReject::Schema);
            continue;
        }
        if let Some(existing) = next
            .global
            .paths
            .iter_mut()
            .find(|item| item.path == fact.path)
        {
            if alpha < OPEN_ALPHA && existing.content_hash != hash {
                rejected.push(MergeReject::LowOverlap);
            } else {
                existing.content_hash = hash;
                existing.citation = citation;
            }
        } else {
            next.global.paths.push(PathFact {
                path: fact.path.clone(),
                content_hash: hash,
                citation,
            });
        }
    }
    next.global
        .paths
        .sort_by(|left, right| left.path.cmp(&right.path));
    if alpha >= OPEN_ALPHA && !candidate.local.is_empty() {
        next.local.clear();
    }
    for delta in &candidate.local {
        match fresh_text(&delta.text, MAX_LOCAL_BYTES) {
            Ok(text) => {
                if next.local.len() == MAX_LOCAL {
                    next.local.pop_front();
                }
                next.local.push_back(LocalDelta {
                    kind: delta.kind.clone(),
                    text,
                    path: delta.path.clone(),
                });
            }
            Err(reason) => rejected.push(reason),
        }
    }
    rejected.sort_by_key(|reason| *reason as u8);
    rejected.dedup();
    let changed = next != *state;
    MergeOutcome {
        state: next,
        rejected,
        changed,
    }
}

enum SlotWrite {
    Keep,
    Write(String),
    Reject(MergeReject),
}

fn write_slot(existing: &str, incoming: &str, alpha: u16, cap: usize) -> SlotWrite {
    if incoming.len() > cap {
        return SlotWrite::Reject(MergeReject::Oversize);
    }
    if existing.is_empty() {
        return SlotWrite::Write(incoming.to_string());
    }
    if existing == incoming {
        return SlotWrite::Keep;
    }
    if alpha < OPEN_ALPHA && !high_overlap(existing, incoming) {
        return SlotWrite::Reject(MergeReject::LowOverlap);
    }
    if alpha < OPEN_ALPHA {
        let growth = incoming.len().saturating_sub(existing.len());
        let budget = cap.saturating_mul(usize::from(alpha)) / usize::from(OPEN_ALPHA);
        if growth > budget {
            return SlotWrite::Reject(MergeReject::Oversize);
        }
    }
    SlotWrite::Write(incoming.to_string())
}

fn fresh_text(text: &str, cap: usize) -> Result<String, MergeReject> {
    if text.len() > cap {
        Err(MergeReject::Oversize)
    } else {
        Ok(text.to_string())
    }
}

fn high_overlap(left: &str, right: &str) -> bool {
    let left_tokens = tokens(left);
    let right_tokens = tokens(right);
    if left_tokens.is_empty() && right_tokens.is_empty() {
        return true;
    }
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return false;
    }
    let shared = left_tokens.intersection(&right_tokens).count();
    let union = left_tokens.union(&right_tokens).count();
    shared.saturating_mul(2) >= union
}

fn tokens(text: &str) -> HashSet<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn render_prefix(task: &str, global: &GlobalState) -> String {
    let mut out = String::new();
    if !task.is_empty() {
        out.push_str(task.trim());
        out.push('\n');
    }
    if !global.goal.is_empty() {
        out.push_str("goal ");
        out.push_str(&global.goal);
        out.push('\n');
    }
    for constraint in &global.constraints {
        out.push_str("constraint ");
        out.push_str(&constraint.id);
        out.push(' ');
        out.push_str(&constraint.text);
        out.push('\n');
    }
    for fact in &global.paths {
        out.push_str("path ");
        out.push_str(&fact.path);
        out.push(' ');
        out.push_str(&fact.content_hash);
        out.push(' ');
        out.push_str(&fact.citation);
        out.push('\n');
    }
    for question in &global.questions {
        out.push_str("question ");
        out.push_str(question);
        out.push('\n');
    }
    out
}

fn render_window(global: &GlobalState, local: &VecDeque<LocalDelta>, latest_batch: &str) -> String {
    let mut out = String::new();
    let cited = strip_cited(latest_batch, global);
    if !cited.is_empty() {
        out.push_str(cited.trim_end());
        out.push('\n');
    }
    for delta in local {
        if let Some(path) = delta.path.as_deref() {
            if let Some(fact) = global.paths.iter().find(|fact| fact.path == path) {
                out.push_str("overlap ");
                out.push_str(path);
                out.push(' ');
                out.push_str(&fact.content_hash);
                out.push(' ');
                out.push_str(&fact.citation);
                out.push('\n');
            }
        }
        out.push_str(&delta.kind);
        out.push(' ');
        out.push_str(&delta.text);
        out.push('\n');
    }
    out
}

fn strip_cited(batch: &str, global: &GlobalState) -> String {
    let hashes = global
        .paths
        .iter()
        .map(|fact| fact.content_hash.as_str())
        .collect::<HashSet<_>>();
    let mut out = String::new();
    let mut lines = batch.lines();
    while let Some(line) = lines.next() {
        if let Some(hash) = file_line_hash(line) {
            if hashes.contains(hash) {
                out.push_str("cited ");
                out.push_str(hash);
                out.push('\n');
                for body in lines.by_ref() {
                    if body.is_empty() {
                        break;
                    }
                }
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn file_line_hash(line: &str) -> Option<&str> {
    let mut parts = line.split_whitespace();
    if parts.next() != Some("FILE") {
        return None;
    }
    parts.next()?;
    parts.next()
}

fn state_hash(state: &LoopState) -> [u8; 32] {
    let rendered = format!(
        "{}\n{}",
        render_prefix("", &state.global),
        render_window(&state.global, &state.local, "")
    );
    let digest = Sha256::digest(rendered.as_bytes());
    let mut out = [0_u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn parse_delta(raw: &str) -> Option<MergeCandidate> {
    let body: DeltaBody = serde_json::from_str(raw).ok()?;
    Some(MergeCandidate {
        goal: body.goal,
        constraints: body
            .constraints
            .into_iter()
            .map(|item| Constraint {
                id: item.id,
                text: item.text,
            })
            .collect(),
        drop_constraint_ids: body.drop_constraint_ids,
        paths: body
            .paths
            .into_iter()
            .map(|item| PathFact {
                path: item.path,
                content_hash: item.content_hash,
                citation: item.citation,
            })
            .collect(),
        questions: body.questions,
        local: body
            .local
            .into_iter()
            .map(|item| LocalDelta {
                kind: item.kind,
                text: item.text,
                path: item.path,
            })
            .collect(),
    })
}

fn delta_json(content: &str) -> Option<String> {
    let marker = content.find("LOOP_DELTA")?;
    let rest = content[marker + "LOOP_DELTA".len()..].trim_start();
    let start = rest.find('{')?;
    let raw = &rest[start..];
    let end = matching_brace(raw)?;
    Some(raw[..=end].to_string())
}

fn matching_brace(raw: &str) -> Option<usize> {
    let bytes = raw.as_bytes();
    if bytes.first() != Some(&b'{') {
        return None;
    }
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escape = false;
    for (index, byte) in bytes.iter().enumerate() {
        if in_string {
            if escape {
                escape = false;
            } else if *byte == b'\\' {
                escape = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

impl AttentionIndex {
    pub fn clear(&mut self) {
        self.order.clear();
        self.hits.clear();
    }

    pub fn store(&mut self, tool: &str, args: &str, body: &str, paths: Vec<IndexedPath>) {
        if !is_read_tool(tool) || paths.is_empty() || body.len() > MAX_INDEX_BODY || body.is_empty()
        {
            return;
        }
        let key = query_key(tool, args);
        if key.len() > 4 * 1024 {
            return;
        }
        let stored = if let Some(stripped) = body.strip_prefix(REUSED_MARK) {
            stripped.to_string()
        } else {
            body.to_string()
        };
        if self
            .hits
            .insert(
                key.clone(),
                IndexHit {
                    body: stored,
                    paths,
                },
            )
            .is_none()
        {
            self.order.push_back(key);
        }
        while self.order.len() > MAX_QUERIES {
            if let Some(old) = self.order.pop_front() {
                self.hits.remove(&old);
            }
        }
    }

    pub fn reuse(
        &mut self,
        tool: &str,
        args: &str,
        live: &dyn Fn(&str) -> Option<PathStat>,
    ) -> Option<String> {
        if !is_read_tool(tool) {
            return None;
        }
        let key = query_key(tool, args);
        let hit = self.hits.get(&key)?.clone();
        for path in &hit.paths {
            match live(&path.path) {
                Some(stat) if stat == path.stat => {}
                _ => {
                    self.invalidate_path(&path.path);
                    return None;
                }
            }
        }
        Some(format!("{REUSED_MARK}{}", hit.body))
    }

    pub fn observe(
        &mut self,
        name: &str,
        arguments: &Value,
        success: bool,
        body: &str,
        cwd: &Path,
    ) {
        if is_mutation(name) {
            if success {
                if let Some(path) = argument_path(arguments) {
                    self.invalidate_path(&path);
                }
            }
            return;
        }
        if !success || !is_read_tool(name) || body.len() > MAX_INDEX_BODY {
            return;
        }
        let mut paths = Vec::new();
        for cited in cited_paths(body) {
            let Some(stat) = live_stat(cwd, &cited) else {
                continue;
            };
            paths.push(IndexedPath {
                path: cited.clone(),
                stat,
                citation: cited,
            });
        }
        self.store(name, &arguments.to_string(), body, paths);
    }

    pub fn invalidate_path(&mut self, path: &str) {
        self.hits
            .retain(|_, hit| !hit.paths.iter().any(|item| item.path == path));
        self.order.retain(|key| self.hits.contains_key(key));
    }
}

fn is_mutation(name: &str) -> bool {
    matches!(
        name,
        "write_file"
            | "edit_file"
            | "search_replace"
            | "apply_patch"
            | "hashline_edit"
            | "delete_file"
    )
}

fn argument_path(arguments: &Value) -> Option<String> {
    arguments
        .get("path")
        .and_then(Value::as_str)
        .or_else(|| arguments.get("file").and_then(Value::as_str))
        .filter(|path| !path.is_empty())
        .map(str::to_string)
}

fn cited_paths(body: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in body.lines() {
        let Some((path, _rest)) = line.split_once(": ") else {
            continue;
        };
        if path.is_empty() || path.contains(' ') || path.starts_with("reused_index") {
            continue;
        }
        if !paths.iter().any(|item| item == path) {
            paths.push(path.to_string());
        }
    }
    paths
}

fn query_key(tool: &str, args: &str) -> String {
    let normalized = if let Ok(value) = serde_json::from_str::<Value>(args) {
        canonical_json(&value)
    } else {
        args.trim().to_string()
    };
    format!("{tool}\n{normalized}")
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            let fields = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(&key).unwrap_or_else(|_| "\"\"".into()),
                        canonical_json(map.get(&key).unwrap_or(&Value::Null))
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        }
        Value::Array(items) => {
            let fields = items
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{fields}]")
        }
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".into()),
    }
}

fn live_stat(root: &Path, path: &str) -> Option<PathStat> {
    let full = resolve_indexed_path(root, path)?;
    let meta = std::fs::symlink_metadata(&full).ok()?;
    if !meta.is_file() || meta.len() > u64::try_from(MAX_STAT_BYTES).unwrap_or(u64::MAX) {
        return None;
    }
    let bytes = std::fs::read(&full).ok()?;
    if bytes.len() > MAX_STAT_BYTES {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |time| time.as_secs());
    let digest = Sha256::digest(&bytes);
    Some(PathStat {
        mtime,
        size: meta.len(),
        content_hash: format!("{digest:x}"),
    })
}

fn resolve_indexed_path(root: &Path, path: &str) -> Option<PathBuf> {
    let candidate = Path::new(path);
    let full = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let parent = full.parent()?.canonicalize().ok()?;
    if parent != root && !parent.starts_with(&root) {
        return None;
    }
    Some(full)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constraint(id: &str, text: &str) -> Constraint {
        Constraint {
            id: id.into(),
            text: text.into(),
        }
    }

    fn stat() -> PathStat {
        PathStat {
            mtime: 7,
            size: 4,
            content_hash: "abc".into(),
        }
    }

    #[test]
    fn long_fixture_reuses_prefix_window_and_index_and_stops() {
        let mut session = Session::default();
        let fill = MergeCandidate {
            goal: Some("keep the parser total".into()),
            constraints: vec![constraint("c1", "do not drop digits")],
            drop_constraint_ids: Vec::new(),
            paths: vec![PathFact {
                path: "src/a.rs".into(),
                content_hash: "abc".into(),
                citation: "src/a.rs:1".into(),
            }],
            questions: Vec::new(),
            local: Vec::new(),
        };
        let filled = session.merge(1, false, &fill);
        assert!(filled.rejected.is_empty());
        assert!(filled.changed);
        let transcript = vec![TranscriptItem {
            body: format!("FILE src/a.rs abc\n{}", "x".repeat(8_000)),
        }];
        let latest = format!("FILE src/a.rs abc\n{}\n\n", "y".repeat(4_000));
        let first = project_preface("fix the total", session.state(), &transcript, &latest);
        let again = project_preface("fix the total", session.state(), &transcript, &latest);
        assert_eq!(first.prefix, again.prefix);
        assert!(first.prefix.contains("goal keep the parser total"));
        assert!(first.window.contains("cited abc"));
        assert!(!first.window.contains(&"y".repeat(100)));
        assert!(first.stats.saved_tokens > 0);
        assert!(estimate_text_tokens(&first.window) < estimate_text_tokens(&transcript[0].body));

        let dropped = session.merge(
            1,
            false,
            &MergeCandidate {
                goal: None,
                constraints: Vec::new(),
                drop_constraint_ids: vec!["c1".into()],
                paths: Vec::new(),
                questions: Vec::new(),
                local: Vec::new(),
            },
        );
        assert!(dropped.rejected.contains(&MergeReject::ConstraintDeleted));
        assert!(session
            .state()
            .global
            .constraints
            .iter()
            .any(|item| item.id == "c1"));
        assert_eq!(session.complete_response(), DepthDecision::Continue);

        let local = session.merge(
            2,
            false,
            &MergeCandidate {
                goal: None,
                constraints: Vec::new(),
                drop_constraint_ids: Vec::new(),
                paths: Vec::new(),
                questions: Vec::new(),
                local: vec![LocalDelta {
                    kind: "verification".into(),
                    text: "total still short by one".into(),
                    path: Some("src/a.rs".into()),
                }],
            },
        );
        assert!(local.changed);
        let second = project_preface("fix the total", session.state(), &transcript, "tail");
        assert_eq!(second.prefix, first.prefix);
        assert!(second.window.contains("overlap src/a.rs abc src/a.rs:1"));
        assert!(second.window.contains("total still short by one"));

        let mut calls = 0_u8;
        let live = HashMap::from([("src/a.rs".into(), stat())]);
        let args = "{\"query\":\"total\"}";
        let run = |calls: &mut u8| {
            *calls += 1;
            (
                "src/a.rs: 1: total".into(),
                vec![IndexedPath {
                    path: "src/a.rs".into(),
                    stat: stat(),
                    citation: "src/a.rs:1".into(),
                }],
            )
        };
        let seen = session.resolve_query("search_files", args, &live, || run(&mut calls));
        assert_eq!(calls, 1);
        assert_eq!(seen, "src/a.rs: 1: total");
        let reused = session.resolve_query("search_files", args, &live, || run(&mut calls));
        assert_eq!(calls, 1);
        assert!(reused.starts_with(REUSED_MARK));
        let mut moved = live.clone();
        moved.get_mut("src/a.rs").expect("path").content_hash = "zzz".into();
        let _ = session.resolve_query("search_files", args, &moved, || run(&mut calls));
        assert_eq!(calls, 2);

        session.note_facts(&TurnFacts {
            review_high: true,
            ..TurnFacts::default()
        });
        assert_eq!(session.complete_response(), DepthDecision::Continue);
        let before = state_hash(session.state());
        let rewrite = session.merge(
            3,
            true,
            &MergeCandidate {
                goal: Some("replace the goal with a different task".into()),
                constraints: vec![constraint("c1", "rewrite the constraint completely")],
                drop_constraint_ids: vec!["c1".into()],
                paths: Vec::new(),
                questions: vec!["start over".into()],
                local: Vec::new(),
            },
        );
        assert!(rewrite.rejected.contains(&MergeReject::ExtraPassGlobal));
        assert!(rewrite.rejected.contains(&MergeReject::ConstraintDeleted));
        assert!(!rewrite.changed);
        assert_eq!(state_hash(session.state()), before);
        assert_eq!(session.complete_response(), DepthDecision::StopUnchanged);
    }

    #[test]
    fn short_fixture_stays_on_one_pass() {
        let facts = TurnFacts {
            status_or_slash: true,
            ..TurnFacts::default()
        };
        assert_eq!(classify(&facts), ChainKind::Short);
        assert!(!runs_second_pass(&facts));
        let read = TurnFacts {
            single_known_file: true,
            ..TurnFacts::default()
        };
        assert!(!runs_second_pass(&read));
        let long = TurnFacts {
            checkpoint_or_pruning: true,
            ..TurnFacts::default()
        };
        assert!(runs_second_pass(&long));
        assert_eq!(
            effort_for_chain(
                true,
                ChainKind::Short,
                Some(ReasoningEffort::High),
                &[ReasoningEffort::Low, ReasoningEffort::High]
            ),
            Some(ReasoningEffort::Low)
        );
        assert_eq!(
            effort_for_chain(
                false,
                ChainKind::Short,
                Some(ReasoningEffort::High),
                &[ReasoningEffort::Low]
            ),
            Some(ReasoningEffort::High)
        );
        assert!(!crate::config::AgentConfig::default().loop_refine);
    }

    #[test]
    fn pass_one_rejects_a_low_overlap_rewrite_and_a_hard_token_writes() {
        let state = LoopState {
            global: GlobalState {
                goal: "keep the parser total stable".into(),
                ..GlobalState::default()
            },
            ..LoopState::default()
        };
        let quiet = gated_merge(
            &state,
            1,
            false,
            &MergeCandidate {
                goal: Some("unrelated wallpaper change".into()),
                constraints: Vec::new(),
                drop_constraint_ids: Vec::new(),
                paths: Vec::new(),
                questions: Vec::new(),
                local: Vec::new(),
            },
        );
        assert!(quiet.rejected.contains(&MergeReject::LowOverlap));
        assert_eq!(quiet.state.global.goal, state.global.goal);
        let hard = gated_merge(
            &state,
            1,
            true,
            &MergeCandidate {
                goal: Some("the failing total is the hard token".into()),
                constraints: Vec::new(),
                drop_constraint_ids: Vec::new(),
                paths: Vec::new(),
                questions: Vec::new(),
                local: Vec::new(),
            },
        );
        assert!(hard.changed);
        assert_eq!(
            hard.state.global.goal,
            "the failing total is the hard token"
        );
    }

    #[test]
    fn unknown_delta_field_keeps_the_previous_state() {
        let mut session = Session::default();
        let outcome = session.merge_json(1, false, "{\"goal\":\"one\",\"extra\":true}");
        assert_eq!(outcome.rejected, vec![MergeReject::Schema]);
        assert!(!outcome.changed);
        assert!(session.state().global.goal.is_empty());
    }

    #[test]
    fn failed_search_is_not_reused_and_an_edit_drops_the_path() {
        let mut session = Session::default();
        let live = HashMap::from([("src/a.rs".into(), stat())]);
        let mut calls = 0_u8;
        let first = session.resolve_query("search_files", "q", &live, || {
            calls += 1;
            ("error missing".into(), Vec::new())
        });
        assert_eq!(first, "error missing");
        let second = session.resolve_query("search_files", "q", &live, || {
            calls += 1;
            (
                "src/a.rs: 1: total".into(),
                vec![IndexedPath {
                    path: "src/a.rs".into(),
                    stat: stat(),
                    citation: "src/a.rs:1".into(),
                }],
            )
        });
        assert_eq!(second, "src/a.rs: 1: total");
        assert_eq!(calls, 2);
        let third = session.resolve_query("search_files", "q", &live, || {
            calls += 1;
            ("unused".into(), Vec::new())
        });
        assert!(third.starts_with(REUSED_MARK));
        assert_eq!(calls, 2);
        session
            .index
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .observe(
                "edit_file",
                &serde_json::json!({"path": "src/a.rs"}),
                true,
                "edited",
                Path::new("."),
            );
        let fourth = session.resolve_query("search_files", "q", &live, || {
            calls += 1;
            ("src/a.rs: 2: next".into(), Vec::new())
        });
        assert_eq!(fourth, "src/a.rs: 2: next");
        assert_eq!(calls, 3);
    }

    #[test]
    fn preface_keeps_the_first_global_bytes() {
        let preface = LoopPreface {
            prefix: "goal one\n".into(),
            window: "fact a\n".into(),
            stats: PruningStats::default(),
        };
        let mut stable = String::from("system");
        let mut transient = vec!["Loop window\nold".into()];
        install_strings(&mut stable, &mut transient, &preface);
        let rewritten = LoopPreface {
            prefix: "goal two\n".into(),
            window: "fact b\n".into(),
            stats: PruningStats::default(),
        };
        install_strings(&mut stable, &mut transient, &rewritten);
        assert_eq!(stable, "system\n## Loop global\ngoal one\n");
        assert_eq!(transient, vec!["Loop window\nfact b\n".to_string()]);
    }

    #[test]
    fn a_goal_without_a_hard_signal_does_not_reach_a_third_pass() {
        let mut session = Session::default();
        assert_eq!(session.complete_response(), DepthDecision::Continue);
        assert_eq!(session.complete_response(), DepthDecision::Done);
        assert_eq!(session.pass(), 0);
    }
}
