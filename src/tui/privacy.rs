//! Local privacy policy editor. Parsing, persistence and provider work belong
//! to its owner; this module emits actions and renders already prepared views.
//! V selects a reviewed, exact-version line/character range; R inspects requests.

use crate::privacy::policy::{Action as RuleAction, Policy, Rule};
use crate::privacy::projection::Projection;
use crate::tui::event::{MouseSelectionPhase, ScrollDirection};
use crate::tui::Event;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub policy: Policy,
    pub legacy_report: String,
    pub observed_sources: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SourceView {
    pub path: String,
    pub original: String,
    pub projection: Projection,
    changes: Arc<str>,
}

impl SourceView {
    /// Prepare this in the owner's source/projection worker, not while drawing.
    pub fn new(path: String, original: String, projection: Projection) -> Self {
        let changes = projection
            .changes
            .iter()
            .map(|change| {
                format!(
                    "{} · {}\n- {}\n+ {}",
                    change.selector,
                    action_name(change.action),
                    change.original,
                    change.replacement
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        Self {
            path,
            original,
            projection,
            changes: Arc::from(changes),
        }
    }
}

pub(super) struct RequestView<'a> {
    pub generation: u64,
    pub original: Option<&'a str>,
    pub sent: &'a str,
    pub changes: Option<&'a str>,
    pub latest: &'a str,
    pub opaque_blocks: usize,
    pub complete: bool,
    pub working: bool,
}

#[derive(Debug, Clone)]
pub enum Action {
    LoadSnapshot,
    Close,
    LoadSource { path: String, policy: Policy },
    Preview { path: String, policy: Policy },
    Save { policy: Policy },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Sources,
    Preview,
    Legacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewMode {
    Sent,
    Original,
    Changes,
    Request,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleScope {
    Value,
    Group,
    File,
    MatchingFiles,
}

impl RuleScope {
    fn label(self) -> &'static str {
        match self {
            Self::Value => "Selected value",
            Self::Group => "Containing group",
            Self::File => "Whole file",
            Self::MatchingFiles => "Matching files",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Hit {
    Enabled,
    Page(Page),
    Path,
    Load,
    Observed(usize),
    Node(usize),
    Rule(RuleAction),
    Scope,
    Group,
    Pattern,
    Range,
    Mode(PreviewMode),
    Save,
    Close,
}

#[derive(Debug, Clone)]
struct SelectedRange {
    path: String,
    digest: String,
    bytes: std::ops::Range<usize>,
    coordinates: String,
}

#[derive(Debug)]
pub struct Editor {
    workspace: String,
    snapshot: Option<Snapshot>,
    policy: Option<Policy>,
    source: Option<SourceView>,
    page: Page,
    mode: PreviewMode,
    request_target: bool,
    reuse_selector: bool,
    rule_scope: RuleScope,
    file_pattern: String,
    pattern_cursor: usize,
    editing_pattern: bool,
    confirmed_pattern: Option<String>,
    selected_range: Option<SelectedRange>,
    range_input: String,
    range_cursor: usize,
    editing_range: bool,
    selected: usize,
    path: String,
    path_cursor: usize,
    editing_path: bool,
    loading: bool,
    source_busy: bool,
    saving: bool,
    discard_prompt: bool,
    error: Option<String>,
    notice: Option<String>,
    actions: VecDeque<Action>,
    scroll: Cell<u16>,
    max_scroll: Cell<u16>,
    visible_rows: Cell<u16>,
    request_generation: Cell<u64>,
    window: Cell<usize>,
    max_window: Cell<usize>,
    hits: RefCell<Vec<(Rect, Hit)>>,
}

impl Editor {
    pub fn loading(workspace: String) -> Self {
        Self {
            workspace,
            snapshot: None,
            policy: None,
            source: None,
            page: Page::Sources,
            mode: PreviewMode::Sent,
            request_target: false,
            reuse_selector: false,
            rule_scope: RuleScope::Value,
            file_pattern: String::new(),
            pattern_cursor: 0,
            editing_pattern: false,
            confirmed_pattern: None,
            selected_range: None,
            range_input: String::new(),
            range_cursor: 0,
            editing_range: false,
            selected: 0,
            path: String::new(),
            path_cursor: 0,
            editing_path: false,
            loading: true,
            source_busy: false,
            saving: false,
            discard_prompt: false,
            error: None,
            notice: None,
            actions: VecDeque::from([Action::LoadSnapshot]),
            scroll: Cell::new(0),
            max_scroll: Cell::new(0),
            visible_rows: Cell::new(1),
            request_generation: Cell::new(0),
            window: Cell::new(0),
            max_window: Cell::new(0),
            hits: RefCell::new(Vec::new()),
        }
    }

    #[cfg(test)]
    pub fn from_snapshot(workspace: String, snapshot: Snapshot) -> Self {
        let mut editor = Self::loading(workspace);
        editor.policy = Some(snapshot.policy.clone());
        editor.snapshot = Some(snapshot);
        editor.loading = false;
        editor.actions.clear();
        editor
    }

    pub fn load_completed(&mut self, result: Result<Snapshot, String>) {
        self.loading = false;
        match result {
            Ok(snapshot) => {
                self.policy = Some(snapshot.policy.clone());
                self.snapshot = Some(snapshot);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn source_completed(&mut self, result: Result<SourceView, String>) {
        self.source_busy = false;
        match result {
            Ok(source) => {
                if self.selected_range.as_ref().is_some_and(|range| {
                    range.path != source.path || range.digest != source.projection.digest
                }) {
                    self.selected_range = None;
                    self.editing_range = false;
                    self.notice = Some("The source changed. Select and review a new range; the previous rule applies only to its original file version.".into());
                }
                self.path = source.path.clone();
                self.path_cursor = self.path.len();
                self.selected = self
                    .selected
                    .min(source.projection.nodes.len().saturating_sub(1));
                self.source = Some(source);
                self.reset_scroll();
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn save_completed(&mut self, result: Result<Snapshot, String>) {
        self.saving = false;
        match result {
            Ok(snapshot) => {
                self.policy = Some(snapshot.policy.clone());
                self.snapshot = Some(snapshot);
                self.error = None;
                self.notice = Some("Saved. New requests use these choices.".into());
            }
            Err(error) => {
                self.error = Some(error);
                self.notice = None;
            }
        }
    }

    pub fn take_action(&mut self) -> Option<Action> {
        self.actions.pop_front()
    }

    #[cfg(test)]
    pub fn policy(&self) -> Option<&Policy> {
        self.policy.as_ref()
    }

    pub fn dirty(&self) -> bool {
        match (&self.policy, &self.snapshot) {
            (Some(policy), Some(snapshot)) => policy != &snapshot.policy,
            _ => false,
        }
    }

    fn close(&mut self) {
        if self.dirty() {
            self.discard_prompt = true;
        } else {
            self.actions.push_back(Action::Close);
        }
    }

    fn preview(&mut self) {
        let (Some(source), Some(policy)) = (&self.source, &self.policy) else {
            return;
        };
        self.actions
            .retain(|action| !matches!(action, Action::Preview { .. }));
        self.actions.push_back(Action::Preview {
            path: source.path.clone(),
            policy: policy.clone(),
        });
        self.source_busy = true;
        self.error = None;
        self.notice = None;
    }

    fn set_rule(&mut self, action: RuleAction) {
        let (Some(source), Some(policy)) = (&self.source, &mut self.policy) else {
            return;
        };
        let node = source.projection.nodes.get(self.selected);
        let opaque = self.selected_range.as_ref().map_or_else(
            || node.is_some_and(|node| node.kind == crate::privacy::policy::AliasKind::Opaque),
            |range| {
                source.projection.nodes.iter().any(|node| {
                    node.kind == crate::privacy::policy::AliasKind::Opaque
                        && node.range.start < range.bytes.end
                        && range.bytes.start < node.range.end
                })
            },
        );
        if opaque && action == RuleAction::Allow && policy.default_action != RuleAction::Allow {
            self.error = Some("This region could not be parsed safely and stays hidden. Select a supported source region to allow it.".into());
            return;
        }
        let file = if self.selected_range.is_none() && self.rule_scope == RuleScope::MatchingFiles {
            let Some(pattern) = self.confirmed_pattern.clone() else {
                self.error =
                    Some("Press F and confirm a file pattern before applying this rule.".into());
                return;
            };
            pattern
        } else {
            globset::escape(&source.path.replace('\\', "/"))
        };
        let selector = if let Some(range) = &self.selected_range {
            if range.path != source.path || range.digest != source.projection.digest {
                self.error = Some(
                    "The selected range belongs to an older source. Select and review it again."
                        .into(),
                );
                return;
            }
            format!(
                "/@digest/{}/{}/{}",
                range.digest, range.bytes.start, range.bytes.end
            )
        } else {
            let Some(node) = node else {
                return;
            };
            match self.rule_scope {
                RuleScope::Value if !self.reuse_selector => exact_selector(source, node),
                RuleScope::Value => node.selector.clone(),
                RuleScope::Group => crate::privacy::projection::region_group_selector(node)
                    .unwrap_or_else(|| group_selector(&node.selector)),
                RuleScope::File | RuleScope::MatchingFiles => whole_selector(&node.selector),
            }
        };
        let mut next = policy.clone();
        next.rules
            .retain(|rule| rule.file != file || rule.selector != selector);
        next.rules.push(Rule {
            file,
            selector,
            action,
        });
        if let Err(error) = next.validate() {
            self.error = Some(error);
            return;
        }
        *policy = next;
        self.preview();
    }

    fn edit_pattern(&mut self) {
        if self.selected_range.is_some() {
            self.error = Some("An explicit range applies only to this file version. Clear it with V and empty Enter, or select a structural value first.".into());
            return;
        }
        if self.file_pattern.is_empty() {
            let path = self
                .source
                .as_ref()
                .map_or(self.path.as_str(), |source| source.path.as_str());
            self.file_pattern = globset::escape(&path.replace('\\', "/"));
        }
        self.pattern_cursor = self.file_pattern.len();
        self.editing_pattern = true;
        self.editing_path = false;
        self.editing_range = false;
        self.rule_scope = RuleScope::MatchingFiles;
        self.error = None;
        self.notice = Some(
            "Enter confirms this explicit file pattern; then choose Allow, Alias, or Hide.".into(),
        );
    }

    fn confirm_pattern(&mut self) {
        let candidate = Policy {
            rules: vec![Rule {
                file: self.file_pattern.clone(),
                selector: "/**".into(),
                action: RuleAction::Hide,
            }],
            ..Policy::default()
        };
        if let Err(error) = candidate.validate() {
            self.error = Some(error);
            return;
        }
        self.confirmed_pattern = Some(self.file_pattern.clone());
        self.editing_pattern = false;
        self.error = None;
        self.notice =
            Some("File pattern selected. Choose Allow, Alias, or Hide to add the rule.".into());
    }

    fn edit_range(&mut self) {
        let Some(source) = &self.source else {
            self.error = Some("Load a source file before selecting a local range.".into());
            return;
        };
        let bytes = self
            .selected_range
            .as_ref()
            .map(|range| range.bytes.clone())
            .or_else(|| {
                source
                    .projection
                    .nodes
                    .get(self.selected)
                    .map(|node| node.range.clone())
            });
        self.range_input = bytes.map_or_else(String::new, |bytes| {
            format!(
                "{}-{}",
                source_position(&source.original, bytes.start),
                source_position(&source.original, bytes.end)
            )
        });
        self.range_cursor = self.range_input.len();
        self.editing_range = true;
        self.editing_path = false;
        self.editing_pattern = false;
        self.page = Page::Sources;
        self.error = None;
        self.notice = Some("Range: line:column-line:column. Lines and character columns start at 1; the end is excluded. Empty Enter clears the range.".into());
    }

    fn confirm_range(&mut self) {
        if self.range_input.trim().is_empty() {
            self.selected_range = None;
            self.editing_range = false;
            self.error = None;
            self.notice =
                Some("Range cleared. Actions apply to the selected structural value.".into());
            return;
        }
        let Some(source) = &self.source else {
            return;
        };
        let bytes = match parse_source_range(&source.original, self.range_input.trim()) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        self.selected_range = Some(SelectedRange {
            path: source.path.clone(),
            digest: source.projection.digest.clone(),
            bytes,
            coordinates: self.range_input.trim().to_owned(),
        });
        self.rule_scope = RuleScope::Value;
        self.reuse_selector = false;
        self.editing_range = false;
        self.page = Page::Preview;
        self.mode = PreviewMode::Original;
        self.request_target = false;
        self.reset_scroll();
        self.error = None;
        self.notice = Some(format!("Review Original locally: {} (end excluded). Then 1 Sources and A/M/H applies only to this exact file version. V and empty Enter clears it.", self.range_input.trim()));
    }

    fn insert_range(&mut self, text: &str) {
        let text: String = text
            .chars()
            .filter(|character| !character.is_control())
            .collect();
        if self.range_input.len().saturating_add(text.len()) > 128 {
            self.error =
                Some("Keep the range under 128 characters: line:column-line:column.".into());
            return;
        }
        self.range_input.insert_str(self.range_cursor, &text);
        self.range_cursor += text.len();
    }

    fn cycle_scope(&mut self) {
        if self.selected_range.is_some() {
            self.error = Some("An explicit range cannot become a reusable group. Clear it with V and empty Enter, or select a structural value first.".into());
            return;
        }
        self.editing_pattern = false;
        self.rule_scope = match self.rule_scope {
            RuleScope::Value => RuleScope::Group,
            RuleScope::Group => RuleScope::File,
            RuleScope::File => RuleScope::MatchingFiles,
            RuleScope::MatchingFiles => RuleScope::Value,
        };
        if self.rule_scope == RuleScope::MatchingFiles && self.confirmed_pattern.is_none() {
            self.edit_pattern();
        }
    }

    fn toggle_enabled(&mut self) {
        if let Some(policy) = &mut self.policy {
            policy.enabled = !policy.enabled;
            self.preview();
        }
    }

    fn load_source(&mut self) {
        let path = self.path.trim();
        if path.is_empty() {
            self.editing_path = true;
            self.error = Some("Enter a file path, then press Enter to load it locally.".into());
            return;
        }
        if let Some(policy) = self.policy.clone() {
            self.actions.push_back(Action::LoadSource {
                path: path.into(),
                policy,
            });
            self.editing_path = false;
            self.editing_range = false;
            self.source_busy = true;
            self.selected = 0;
            self.reset_scroll();
            self.error = None;
        }
    }

    fn save(&mut self) {
        if let Some(policy) = self.policy.clone() {
            self.actions
                .retain(|action| !matches!(action, Action::Preview { .. }));
            self.actions.push_back(Action::Save { policy });
            self.saving = true;
            self.error = None;
            self.notice = None;
        }
    }

    fn hit(&mut self, hit: Hit) {
        match hit {
            Hit::Enabled => self.toggle_enabled(),
            Hit::Page(page) => {
                self.page = page;
                if page == Page::Sources {
                    self.request_target = false;
                }
                self.reset_scroll();
            }
            Hit::Path => {
                self.editing_pattern = false;
                self.editing_range = false;
                self.editing_path = true;
                self.path_cursor = self.path.len();
            }
            Hit::Load if self.editing_range => self.confirm_range(),
            Hit::Load if self.editing_pattern => self.confirm_pattern(),
            Hit::Load => self.load_source(),
            Hit::Observed(index) => {
                if let Some(path) = self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.observed_sources.get(index))
                {
                    self.path = path.clone();
                    self.path_cursor = self.path.len();
                    self.load_source();
                }
            }
            Hit::Node(index) => {
                self.selected_range = None;
                self.editing_range = false;
                self.selected = index;
            }
            Hit::Rule(action) => self.set_rule(action),
            Hit::Scope if self.selected_range.is_some() => {
                self.error = Some("Explicit ranges always use this file version. Select a structural value to choose reuse.".into());
            }
            Hit::Scope if self.rule_scope == RuleScope::Value => {
                self.reuse_selector = !self.reuse_selector
            }
            Hit::Scope => {}
            Hit::Group => self.cycle_scope(),
            Hit::Pattern => self.edit_pattern(),
            Hit::Range => self.edit_range(),
            Hit::Mode(mode) => {
                if mode == PreviewMode::Request {
                    self.request_target = !self.request_target;
                    self.mode = PreviewMode::Sent;
                } else {
                    self.mode = mode;
                }
                self.page = Page::Preview;
                self.reset_scroll();
            }
            Hit::Save => self.save(),
            Hit::Close => self.close(),
        }
    }

    fn insert_path(&mut self, text: &str) {
        let text: String = text
            .chars()
            .filter(|character| !character.is_control())
            .collect();
        if self.path.len().saturating_add(text.len()) > 4096 {
            self.error = Some("Keep the file path under 4,096 bytes.".into());
            return;
        }
        self.path.insert_str(self.path_cursor, &text);
        self.path_cursor += text.len();
    }

    fn insert_pattern(&mut self, text: &str) {
        let text: String = text
            .chars()
            .filter(|character| !character.is_control())
            .collect();
        if self.file_pattern.len().saturating_add(text.len()) > 4096 {
            self.error = Some("Keep the file pattern under 4,096 bytes.".into());
            return;
        }
        self.file_pattern.insert_str(self.pattern_cursor, &text);
        self.pattern_cursor += text.len();
    }

    fn scroll_by(&self, delta: i32) {
        let next = i32::from(self.scroll.get()).saturating_add(delta);
        if next > i32::from(self.max_scroll.get()) && self.window.get() < self.max_window.get() {
            self.window.set(self.window.get() + 1);
            self.scroll.set(0);
        } else if next < 0 && self.window.get() > 0 {
            self.window.set(self.window.get() - 1);
            self.scroll.set(u16::MAX);
        } else {
            self.scroll
                .set(next.clamp(0, i32::from(self.max_scroll.get())) as u16);
        }
    }

    fn reset_scroll(&self) {
        self.scroll.set(0);
        self.window.set(0);
    }

    pub fn handle(&mut self, event: &Event) {
        if self.saving {
            return;
        }
        if self.discard_prompt {
            match event {
                Event::Key(KeyCode::Enter, _) => self.actions.push_back(Action::Close),
                Event::Key(KeyCode::Esc, _) => self.discard_prompt = false,
                _ => {}
            }
            return;
        }
        if matches!(event, Event::Key(KeyCode::Esc, _)) {
            if self.editing_range {
                self.editing_range = false;
            } else if self.editing_pattern {
                self.editing_pattern = false;
            } else if self.editing_path {
                self.editing_path = false;
            } else {
                self.close();
            }
            return;
        }
        if matches!(event, Event::Key(KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL))
        {
            self.close();
            return;
        }
        if self.loading {
            return;
        }
        if let Event::MouseSelection(event) = event {
            if event.phase == MouseSelectionPhase::Start {
                let hit = self
                    .hits
                    .borrow()
                    .iter()
                    .find(|(area, _)| area.contains(Position::new(event.column, event.row)))
                    .map(|(_, hit)| *hit);
                if let Some(hit) = hit {
                    self.hit(hit);
                }
            }
            return;
        }
        if let Event::MouseScroll(event) = event {
            self.scroll_by(if event.direction == ScrollDirection::Up {
                -3
            } else {
                3
            });
            return;
        }
        if let Event::Paste(text) = event {
            if self.editing_range {
                self.insert_range(text);
            } else if self.editing_pattern {
                self.insert_pattern(text);
            } else if self.editing_path {
                self.insert_path(text);
            }
            return;
        }
        let Event::Key(key, modifiers) = event else {
            return;
        };
        let control = modifiers.contains(KeyModifiers::CONTROL);
        if self.editing_range {
            match key {
                KeyCode::Enter => self.confirm_range(),
                KeyCode::Left => {
                    self.range_cursor = previous_grapheme(&self.range_input, self.range_cursor)
                }
                KeyCode::Right => {
                    self.range_cursor = next_grapheme(&self.range_input, self.range_cursor)
                }
                KeyCode::Home => self.range_cursor = 0,
                KeyCode::End => self.range_cursor = self.range_input.len(),
                KeyCode::Backspace => {
                    let previous = previous_grapheme(&self.range_input, self.range_cursor);
                    self.range_input
                        .replace_range(previous..self.range_cursor, "");
                    self.range_cursor = previous;
                }
                KeyCode::Delete => {
                    let next = next_grapheme(&self.range_input, self.range_cursor);
                    self.range_input.replace_range(self.range_cursor..next, "");
                }
                KeyCode::Char(character)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.insert_range(&character.to_string())
                }
                _ => {}
            }
            return;
        }
        if self.editing_pattern {
            match key {
                KeyCode::Enter => self.confirm_pattern(),
                KeyCode::Left => {
                    self.pattern_cursor = previous_grapheme(&self.file_pattern, self.pattern_cursor)
                }
                KeyCode::Right => {
                    self.pattern_cursor = next_grapheme(&self.file_pattern, self.pattern_cursor)
                }
                KeyCode::Home => self.pattern_cursor = 0,
                KeyCode::End => self.pattern_cursor = self.file_pattern.len(),
                KeyCode::Backspace => {
                    let previous = previous_grapheme(&self.file_pattern, self.pattern_cursor);
                    self.file_pattern
                        .replace_range(previous..self.pattern_cursor, "");
                    self.pattern_cursor = previous;
                }
                KeyCode::Delete => {
                    let next = next_grapheme(&self.file_pattern, self.pattern_cursor);
                    self.file_pattern
                        .replace_range(self.pattern_cursor..next, "");
                }
                KeyCode::Char(character)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.insert_pattern(&character.to_string())
                }
                _ => {}
            }
            return;
        }
        if control && *key == KeyCode::Char('s') {
            self.save();
            return;
        }
        if self.editing_path {
            match key {
                KeyCode::Enter => self.load_source(),
                KeyCode::Left => self.path_cursor = previous_grapheme(&self.path, self.path_cursor),
                KeyCode::Right => self.path_cursor = next_grapheme(&self.path, self.path_cursor),
                KeyCode::Home => self.path_cursor = 0,
                KeyCode::End => self.path_cursor = self.path.len(),
                KeyCode::Backspace => {
                    let previous = previous_grapheme(&self.path, self.path_cursor);
                    self.path.replace_range(previous..self.path_cursor, "");
                    self.path_cursor = previous;
                }
                KeyCode::Delete => {
                    let next = next_grapheme(&self.path, self.path_cursor);
                    self.path.replace_range(self.path_cursor..next, "");
                }
                KeyCode::Char(character)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.insert_path(&character.to_string())
                }
                _ => {}
            }
            return;
        }
        match key {
            KeyCode::Char('1') => self.hit(Hit::Page(Page::Sources)),
            KeyCode::Char('2') => self.hit(Hit::Page(Page::Preview)),
            KeyCode::Char('3') => self.hit(Hit::Page(Page::Legacy)),
            KeyCode::Char('p') => self.hit(Hit::Path),
            KeyCode::Char('f') if self.page == Page::Sources => self.hit(Hit::Pattern),
            KeyCode::Char('g') if self.page == Page::Sources => self.hit(Hit::Group),
            KeyCode::Char('v') => self.hit(Hit::Range),
            KeyCode::Char('b') => {
                self.source = None;
                self.selected_range = None;
                self.selected = 0;
                self.page = Page::Sources;
                self.reset_scroll();
            }
            KeyCode::Char('e') | KeyCode::Char(' ') => self.toggle_enabled(),
            KeyCode::Char('a') if self.page == Page::Sources => self.set_rule(RuleAction::Allow),
            KeyCode::Char('m') if self.page == Page::Sources => self.set_rule(RuleAction::Alias),
            KeyCode::Char('h') if self.page == Page::Sources => self.set_rule(RuleAction::Hide),
            KeyCode::Char('u') if self.page == Page::Sources => self.hit(Hit::Scope),
            KeyCode::Char('o') => self.hit(Hit::Mode(PreviewMode::Original)),
            KeyCode::Char('t') => self.hit(Hit::Mode(PreviewMode::Sent)),
            KeyCode::Char('d') => self.hit(Hit::Mode(PreviewMode::Changes)),
            KeyCode::Char('r') => self.hit(Hit::Mode(PreviewMode::Request)),
            KeyCode::Tab | KeyCode::BackTab => {
                self.page = match self.page {
                    Page::Sources => Page::Preview,
                    Page::Preview => Page::Legacy,
                    Page::Legacy => Page::Sources,
                };
                self.reset_scroll();
            }
            KeyCode::Up if self.page == Page::Sources => {
                self.selected_range = None;
                self.selected = self.selected.saturating_sub(1)
            }
            KeyCode::Down if self.page == Page::Sources => {
                self.selected_range = None;
                let count = self.source.as_ref().map_or_else(
                    || {
                        self.snapshot
                            .as_ref()
                            .map_or(0, |snapshot| snapshot.observed_sources.len())
                    },
                    |source| source.projection.nodes.len(),
                );
                self.selected = self.selected.saturating_add(1).min(count.saturating_sub(1));
            }
            KeyCode::Enter if self.page == Page::Sources && self.source.is_none() => {
                self.hit(Hit::Observed(self.selected))
            }
            KeyCode::Up => self.scroll_by(-1),
            KeyCode::Down => self.scroll_by(1),
            KeyCode::PageUp => self.scroll_by(-i32::from(self.visible_rows.get())),
            KeyCode::PageDown => self.scroll_by(i32::from(self.visible_rows.get())),
            KeyCode::Home => self.reset_scroll(),
            KeyCode::End => {
                self.window.set(self.max_window.get());
                self.scroll.set(u16::MAX);
            }
            _ => {}
        }
    }
}

fn previous_grapheme(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_grapheme(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .graphemes(true)
        .next()
        .map_or(text.len(), |grapheme| cursor + grapheme.len())
}

fn action_name(action: RuleAction) -> &'static str {
    match action {
        RuleAction::Allow => "Allow",
        RuleAction::Alias => "Alias",
        RuleAction::Hide => "Hide",
    }
}

fn exact_selector(source: &SourceView, node: &crate::privacy::projection::Node) -> String {
    format!(
        "/@digest/{}/{}/{}",
        source.projection.digest, node.range.start, node.range.end
    )
}

fn source_position(text: &str, offset: usize) -> String {
    let before = text.get(..offset).unwrap_or("");
    let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .trim_end_matches('\r')
        .chars()
        .count()
        + 1;
    format!("{line}:{column}")
}

fn parse_source_range(text: &str, input: &str) -> Result<std::ops::Range<usize>, String> {
    fn position(text: &str, input: &str) -> Option<usize> {
        let (line, column) = input.trim().split_once(':')?;
        let line = line.parse::<usize>().ok()?.checked_sub(1)?;
        let column = column.parse::<usize>().ok()?.checked_sub(1)?;
        let start = if line == 0 {
            0
        } else {
            text.match_indices('\n').nth(line - 1)?.0 + 1
        };
        let end = text[start..]
            .find('\n')
            .map_or(text.len(), |end| start + end);
        let end = if end.checked_sub(1).and_then(|at| text.as_bytes().get(at)) == Some(&b'\r') {
            end - 1
        } else {
            end
        };
        let line = &text[start..end];
        if column == line.chars().count() {
            Some(end)
        } else {
            line.char_indices()
                .nth(column)
                .map(|(offset, _)| start + offset)
        }
    }
    let invalid = || {
        "Use line:column-line:column within Original. Lines and character columns start at 1; the end is excluded.".to_string()
    };
    let (begin, end) = input.split_once('-').ok_or_else(invalid)?;
    let begin = position(text, begin).ok_or_else(invalid)?;
    let end = position(text, end).ok_or_else(invalid)?;
    if begin >= end || !text.is_char_boundary(begin) || !text.is_char_boundary(end) {
        return Err(
            "Select a nonempty range in forward order. The end position is excluded.".into(),
        );
    }
    Ok(begin..end)
}

fn shape_parts(selector: &str) -> (&str, &str) {
    if let Some(rest) = selector.strip_prefix("/@shape/") {
        if let Some((shape, _)) = rest.split_once('/') {
            let length = "/@shape/".len() + shape.len();
            return (&selector[..length], &selector[length..]);
        }
    }
    ("", selector)
}

fn whole_selector(selector: &str) -> String {
    if selector.starts_with("/@region/") {
        if let Some((_, rest)) = selector.split_once("/@shape/") {
            if let Some((shape, _)) = rest.split_once('/') {
                return format!("/**/@shape/{shape}/**");
            }
        }
    }
    let (shape, _) = shape_parts(selector);
    format!("{shape}/**")
}

fn group_selector(selector: &str) -> String {
    let (shape, path) = shape_parts(selector);
    let segments: Vec<_> = path.trim_start_matches('/').split('/').collect();
    if segments.first() == Some(&"markdown") && segments.get(1) == Some(&"section") {
        if let Some(section) = segments.get(2) {
            return format!("{shape}/markdown/section/{section}/**");
        }
    }
    if segments.first() == Some(&"csv")
        && segments.get(1) == Some(&"row")
        && segments.get(3) == Some(&"column")
    {
        if let Some(column) = segments.get(4) {
            return format!("{shape}/csv/row/*/column/{column}");
        }
    }
    let drop = if segments
        .last()
        .is_some_and(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        2
    } else {
        1
    };
    if segments.len() <= drop {
        return whole_selector(selector);
    }
    format!("{shape}/{}/**", segments[..segments.len() - drop].join("/"))
}

fn node_action(
    policy: Option<&Policy>,
    source: &SourceView,
    node: &crate::privacy::projection::Node,
) -> RuleAction {
    if node.kind == crate::privacy::policy::AliasKind::Opaque
        && policy.is_none_or(|policy| policy.default_action != RuleAction::Allow)
    {
        return RuleAction::Hide;
    }
    policy.map_or(RuleAction::Hide, |policy| {
        policy
            .for_path(&source.path)
            .action_for_exact(&node.selector, &exact_selector(source, node))
    })
}

fn button(
    frame: &mut ratatui::Frame,
    editor: &Editor,
    area: Rect,
    text: &str,
    hit: Hit,
    active: bool,
    theme: super::render::ThemePalette,
) {
    let style = if active {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.muted)
    };
    frame.render_widget(Paragraph::new(text).style(style), area);
    editor.hits.borrow_mut().push((area, hit));
}

fn text_panel(
    frame: &mut ratatui::Frame,
    area: Rect,
    title: &str,
    text: &str,
    editor: &Editor,
    theme: super::render::ThemePalette,
) {
    // Wrap only one bounded byte window. Every byte remains reachable without
    // reparsing a multi-megabyte exact request on each terminal frame.
    const WINDOW_BYTES: usize = 32 * 1024;
    let max_window = text.len().saturating_sub(1) / WINDOW_BYTES;
    editor
        .max_window
        .set(editor.max_window.get().max(max_window));
    let mut start = editor
        .window
        .get()
        .min(max_window)
        .saturating_mul(WINDOW_BYTES);
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = start.saturating_add(WINDOW_BYTES).min(text.len());
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }
    let title = if max_window > 0 {
        format!(
            "{title} · bytes {}–{end}/{}",
            if text.is_empty() { 0 } else { start + 1 },
            text.len()
        )
    } else {
        title.to_owned()
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let visible = crate::terminal_text::sanitize_terminal_text(&text[start..end]);
    let paragraph = Paragraph::new(visible)
        .style(Style::default().fg(theme.text))
        .wrap(Wrap { trim: false });
    let max = paragraph
        .line_count(inner.width.max(1))
        .saturating_sub(usize::from(inner.height));
    editor.max_scroll.set(
        editor
            .max_scroll
            .get()
            .max(u16::try_from(max).unwrap_or(u16::MAX)),
    );
    editor.visible_rows.set(inner.height.max(1));
    editor
        .scroll
        .set(editor.scroll.get().min(editor.max_scroll.get()));
    frame.render_widget(
        paragraph.scroll((editor.scroll.get().min(editor.max_scroll.get()), 0)),
        inner,
    );
}

pub(super) fn render_with_version_label(
    frame: &mut ratatui::Frame,
    area: Rect,
    editor: &Editor,
    version: &str,
    theme: super::render::ThemePalette,
    request: RequestView<'_>,
) {
    editor.hits.borrow_mut().clear();
    editor.max_scroll.set(0);
    editor.max_window.set(0);
    frame.render_widget(Clear, area);
    let title = if UnicodeWidthStr::width(version) + 15 <= usize::from(area.width) {
        format!(" {version} · Privacy ")
    } else {
        version.to_owned()
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().fg(theme.text).bg(theme.surface));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 25 || inner.height < 8 {
        frame.render_widget(
            Paragraph::new(
                "Privacy editor\nEnlarge terminal to edit.\nEsc closes; draft preserved.",
            )
            .wrap(Wrap { trim: false }),
            inner,
        );
        return;
    }
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .split(inner);
    let enabled = editor.policy.as_ref().is_some_and(|policy| policy.enabled);
    let toolbar = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(25),
            Constraint::Min(1),
            Constraint::Length(7),
            Constraint::Length(7),
        ])
        .split(areas[0]);
    button(
        frame,
        editor,
        toolbar[0],
        if editor
            .policy
            .as_ref()
            .is_some_and(|policy| policy.default_action == RuleAction::Allow)
        {
            if enabled {
                "[x] Selected values only"
            } else {
                "[ ] Selected values only"
            }
        } else if enabled {
            "[x] Hide unless allowed"
        } else {
            "[ ] Hide unless allowed"
        },
        Hit::Enabled,
        enabled,
        theme,
    );
    let state = if editor.saving {
        "Saving…"
    } else if editor.loading {
        "Loading…"
    } else if editor.dirty() {
        "Unsaved changes"
    } else {
        "Local policy"
    };
    frame.render_widget(
        Paragraph::new(state).style(Style::default().fg(theme.muted)),
        toolbar[1],
    );
    button(frame, editor, toolbar[2], " Save ", Hit::Save, false, theme);
    button(
        frame,
        editor,
        toolbar[3],
        " Close ",
        Hit::Close,
        false,
        theme,
    );
    let tabs = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(13),
            Constraint::Length(13),
            Constraint::Min(1),
        ])
        .split(areas[1]);
    for (index, (label, page)) in [
        ("1 Sources", Page::Sources),
        ("2 Preview", Page::Preview),
        ("3 Legacy filter", Page::Legacy),
    ]
    .into_iter()
    .enumerate()
    {
        button(
            frame,
            editor,
            tabs[index],
            label,
            Hit::Page(page),
            editor.page == page,
            theme,
        );
    }
    let path_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(7)])
        .split(areas[2]);
    let path = if editor.editing_range {
        format!("Range: {}", editor.range_input)
    } else if editor.editing_pattern {
        format!("Glob: {}", editor.file_pattern)
    } else if editor.path.is_empty() {
        "P: enter a file path".to_owned()
    } else {
        format!("File: {}", editor.path)
    };
    button(
        frame,
        editor,
        path_row[0],
        &path,
        if editor.editing_range {
            Hit::Range
        } else {
            Hit::Path
        },
        editor.editing_path || editor.editing_pattern || editor.editing_range,
        theme,
    );
    button(
        frame,
        editor,
        path_row[1],
        if editor.editing_pattern || editor.editing_range {
            " Use "
        } else {
            " Load "
        },
        Hit::Load,
        false,
        theme,
    );
    if editor.editing_path || editor.editing_pattern || editor.editing_range {
        let label_width = if editor.editing_range { 7 } else { 6 };
        let available = usize::from(path_row[0].width.saturating_sub(label_width));
        let prefix = if editor.editing_range {
            &editor.range_input[..editor.range_cursor]
        } else if editor.editing_pattern {
            &editor.file_pattern[..editor.pattern_cursor]
        } else {
            &editor.path[..editor.path_cursor]
        };
        let column = UnicodeWidthStr::width(prefix).min(available);
        frame.set_cursor_position((
            path_row[0]
                .x
                .saturating_add(label_width)
                .saturating_add(column as u16),
            path_row[0].y,
        ));
    }
    if editor.loading {
        frame.render_widget(
            Paragraph::new("Loading the local privacy policy…").wrap(Wrap { trim: false }),
            areas[3],
        );
    } else {
        match editor.page {
            Page::Sources => render_sources(frame, areas[3], editor, theme),
            Page::Preview => render_preview(frame, areas[3], editor, theme, request),
            Page::Legacy => {
                let report = editor
                    .snapshot
                    .as_ref()
                    .map_or("Legacy filter report unavailable.", |snapshot| {
                        snapshot.legacy_report.as_str()
                    });
                text_panel(
                    frame,
                    areas[3],
                    "Legacy filter · unchanged",
                    report,
                    editor,
                    theme,
                );
            }
        }
    }
    let detail = if editor.discard_prompt {
        "Discard unsaved changes? Enter discards · Esc keeps editing".to_owned()
    } else if let Some(error) = &editor.error {
        error.clone()
    } else if let Some(notice) = &editor.notice {
        notice.clone()
    } else if editor.source_busy {
        "Updating local preview… Previous preview is shown until it finishes.".to_owned()
    } else {
        format!(
            "{} · Instructions stay readable; known private values stay masked.",
            editor.workspace
        )
    };
    frame.render_widget(
        Paragraph::new(detail)
            .wrap(Wrap { trim: false })
            .style(
                Style::default().fg(if editor.error.is_some() || editor.discard_prompt {
                    theme.warning
                } else {
                    theme.muted
                }),
            ),
        areas[4],
    );
    frame.render_widget(Paragraph::new(if editor.editing_range { "line:column-line:column · 1-based characters; end excluded\nEnter Review Original · Empty Enter Clear · Esc Cancel" } else if editor.editing_pattern { "Enter Use pattern · Esc Cancel · Then A/M/H to add rule" } else if editor.editing_path { "Enter Load · Esc Finish path · Ctrl-S Save" } else { "G Scope · F Files · V Range · A Allow · M Alias · H Hide\nP Path · B Sources · E Enable · Ctrl-S Save · Esc Close" }).style(Style::default().fg(theme.muted)), areas[5]);
}

fn render_sources(
    frame: &mut ratatui::Frame,
    area: Rect,
    editor: &Editor,
    theme: super::render::ThemePalette,
) {
    let Some(source) = &editor.source else {
        let sources = editor
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.observed_sources.as_slice())
            .unwrap_or_default();
        if sources.is_empty() {
            frame.render_widget(Paragraph::new("Choose a file to review locally.\n\nPress P, enter its path, then Enter.\nNo project scan or provider request is needed.\n\nAllow keeps a selected region readable.\nAlias keeps a stable reference. Hide withholds it.").wrap(Wrap { trim: false }), area);
        } else {
            let rows = usize::from(area.height);
            let start = editor.selected.saturating_add(1).saturating_sub(rows);
            for (index, path) in sources.iter().enumerate().skip(start).take(rows) {
                let row = Rect::new(area.x, area.y + (index - start) as u16, area.width, 1);
                button(
                    frame,
                    editor,
                    row,
                    &format!(
                        "{} {path}",
                        if index == editor.selected { ">" } else { " " }
                    ),
                    Hit::Observed(index),
                    index == editor.selected,
                    theme,
                );
            }
        }
        return;
    };
    let parts = if area.width >= 100 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Percentage(35),
                Constraint::Percentage(35),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(100)])
            .split(area)
    };
    let controls = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(4),
        ])
        .split(parts[0]);
    let rows = usize::from(controls[0].height);
    let start = editor.selected.saturating_add(1).saturating_sub(rows);
    for (index, node) in source
        .projection
        .nodes
        .iter()
        .enumerate()
        .skip(start)
        .take(rows)
    {
        let action = node_action(editor.policy.as_ref(), source, node);
        let row = Rect::new(
            controls[0].x,
            controls[0].y + (index - start) as u16,
            controls[0].width,
            1,
        );
        button(
            frame,
            editor,
            row,
            &format!(
                "{} {} · {}",
                if index == editor.selected { ">" } else { " " },
                node.label,
                action_name(action)
            ),
            Hit::Node(index),
            index == editor.selected,
            theme,
        );
    }
    button(
        frame,
        editor,
        controls[1],
        &editor.selected_range.as_ref().map_or_else(
            || format!("G Scope: {}", editor.rule_scope.label()),
            |range| format!("V Range: {}", range.coordinates),
        ),
        if editor.selected_range.is_some() {
            Hit::Range
        } else {
            Hit::Group
        },
        false,
        theme,
    );
    button(
        frame,
        editor,
        controls[2],
        if editor.selected_range.is_some() {
            "Exact file version · end excluded"
        } else if editor.rule_scope != RuleScope::Value {
            "Reuse while structure matches"
        } else if editor.reuse_selector {
            "U Reuse while structure matches"
        } else {
            "U This file version"
        },
        Hit::Scope,
        editor.reuse_selector,
        theme,
    );
    let pattern = editor
        .confirmed_pattern
        .as_deref()
        .unwrap_or("choose an explicit pattern");
    let selectors = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(controls[3]);
    button(
        frame,
        editor,
        selectors[0],
        &format!("F Files: {pattern}"),
        Hit::Pattern,
        editor.rule_scope == RuleScope::MatchingFiles,
        theme,
    );
    button(
        frame,
        editor,
        selectors[1],
        "V Range",
        Hit::Range,
        editor.selected_range.is_some(),
        theme,
    );
    let buttons = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(controls[4]);
    for (index, (label, action)) in [
        ("A Allow", RuleAction::Allow),
        ("M Alias", RuleAction::Alias),
        ("H Hide", RuleAction::Hide),
    ]
    .into_iter()
    .enumerate()
    {
        button(
            frame,
            editor,
            buttons[index],
            label,
            Hit::Rule(action),
            false,
            theme,
        );
    }
    let reason = if let Some(range) = &editor.selected_range {
        let excerpt: String = source
            .original
            .get(range.bytes.clone())
            .unwrap_or("")
            .chars()
            .take(256)
            .collect();
        format!("Selected locally: {excerpt}\nExact file version · bytes {}:{}\nV + empty Enter clears; choose a node for reuse.", range.bytes.start, range.bytes.end)
    } else {
        source.projection.nodes.get(editor.selected).map_or_else(
            || "No selectable content in this file.".into(),
            |node| {
                let original = source.original.get(node.range.clone()).unwrap_or("");
                let excerpt: String = original.chars().take(256).collect();
                format!(
                    "Selected locally: {excerpt}\n{} · {}{}",
                    node.selector,
                    action_name(node_action(editor.policy.as_ref(), source, node)),
                    if node.kind == crate::privacy::policy::AliasKind::Opaque {
                        " · unsupported or unparsed region stays opaque"
                    } else {
                        " under current policy"
                    },
                )
            },
        )
    };
    frame.render_widget(
        Paragraph::new(reason)
            .style(Style::default().fg(theme.muted))
            .wrap(Wrap { trim: false }),
        controls[5],
    );
    if parts.len() == 3 {
        text_panel(
            frame,
            parts[1],
            "Original · local only",
            &source.original,
            editor,
            theme,
        );
        text_panel(
            frame,
            parts[2],
            "Sent · file projection",
            &source.projection.text,
            editor,
            theme,
        );
    }
}

fn render_preview(
    frame: &mut ratatui::Frame,
    area: Rect,
    editor: &Editor,
    theme: super::render::ThemePalette,
    request: RequestView<'_>,
) {
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(if editor.request_target { 3 } else { 0 }),
            Constraint::Min(1),
        ])
        .split(area);
    let tabs = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(10),
            Constraint::Length(13),
            Constraint::Length(13),
            Constraint::Min(1),
        ])
        .split(parts[0]);
    for (index, (label, mode)) in [
        ("T Full", PreviewMode::Sent),
        ("D Changes", PreviewMode::Changes),
        ("O Original", PreviewMode::Original),
        ("R Request", PreviewMode::Request),
    ]
    .into_iter()
    .enumerate()
    {
        button(
            frame,
            editor,
            tabs[index],
            label,
            Hit::Mode(mode),
            if mode == PreviewMode::Request {
                editor.request_target
            } else {
                editor.mode == mode
            },
            theme,
        );
    }
    if editor.request_target {
        if editor.request_generation.get() != request.generation {
            editor.request_generation.set(request.generation);
            editor.reset_scroll();
        }
        let status = if request.generation == 0 {
            "Waiting for a prepared provider request.".into()
        } else {
            format!(
                "Latest request {}{} · {} bytes · ~{} tokens{}",
                request.generation,
                if request.working { " · pending" } else { "" },
                request.sent.len(),
                estimated_tokens(request.sent),
                if request.opaque_blocks > 0 {
                    " · opaque blocks omitted"
                } else {
                    ""
                },
            )
        };
        frame.render_widget(
            Paragraph::new(status).style(Style::default().fg(theme.muted)),
            parts[1],
        );
        frame.render_widget(
            Paragraph::new(request.latest)
                .style(Style::default().fg(theme.text))
                .wrap(Wrap { trim: false }),
            parts[2],
        );
        let (title, text) = match editor.mode {
            PreviewMode::Original => (
                "Original request · local only",
                request
                    .original
                    .unwrap_or("Original request unavailable for this legacy preview."),
            ),
            PreviewMode::Changes => (
                "Request changes · original → sent",
                request
                    .changes
                    .unwrap_or("Changes unavailable for this legacy preview."),
            ),
            _ => (
                if request.complete && request.opaque_blocks == 0 {
                    "Exact final request · body only"
                } else if request.complete {
                    "Final request · opaque blocks omitted"
                } else {
                    "Latest sent content · bounded view"
                },
                request.sent,
            ),
        };
        text_panel(frame, parts[3], title, text, editor, theme);
        return;
    }
    let Some(source) = &editor.source else {
        frame.render_widget(
            Paragraph::new(
                "Load a file on Sources, or press R to inspect the latest provider request.",
            )
            .wrap(Wrap { trim: false }),
            parts[3],
        );
        return;
    };
    let sent = &source.projection.text;
    let stats = format!(
        "{} → {} bytes · ~{} → ~{} tokens · {} changes{}",
        source.original.len(),
        sent.len(),
        estimated_tokens(&source.original),
        estimated_tokens(sent),
        source.projection.changes.len(),
        if editor.source_busy {
            " · updating"
        } else {
            ""
        }
    );
    frame.render_widget(
        Paragraph::new(stats).style(Style::default().fg(theme.muted)),
        parts[1],
    );
    let (title, text) = match editor.mode {
        PreviewMode::Original => {
            if let Some(range) = &editor.selected_range {
                (
                    "Original range · local only",
                    source
                        .original
                        .get(range.bytes.clone())
                        .unwrap_or("Selected range is unavailable; select it again."),
                )
            } else {
                ("Original · local only", source.original.as_str())
            }
        }
        PreviewMode::Changes => (
            "Changes · original → sent",
            if source.changes.is_empty() {
                "No replacements in this projection."
            } else {
                source.changes.as_ref()
            },
        ),
        PreviewMode::Sent => ("Full · file projection", sent.as_str()),
        PreviewMode::Request => unreachable!(),
    };
    text_panel(frame, parts[3], title, text, editor, theme);
}

fn estimated_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

#[cfg(test)]
pub(super) fn test_editor() -> Editor {
    use crate::privacy::policy::AliasKind;
    use crate::privacy::projection::{Change, Node};
    let original = "Client: delicate-client\n".to_string();
    let replacement = "CA_CLIENT_1".to_string();
    let mut editor = Editor::from_snapshot(
        "/workspace".into(),
        Snapshot {
            policy: Policy {
                enabled: true,
                ..Policy::default()
            },
            legacy_report: "Legacy stop rules remain armed.".into(),
            observed_sources: vec!["/workspace/plan.md".into()],
        },
    );
    editor.source_completed(Ok(SourceView::new(
        "/workspace/plan.md".into(),
        original.clone(),
        Projection {
            text: format!("Client: {replacement}\n"),
            changes: vec![Change {
                selector: "/paragraph/0".into(),
                range: 8..23,
                original: "delicate-client".into(),
                replacement,
                kind: AliasKind::Text,
                action: RuleAction::Hide,
            }],
            nodes: vec![Node {
                selector: "/paragraph/0".into(),
                label: "Text 1".into(),
                range: 8..23,
                kind: AliasKind::Text,
            }],
            digest: crate::privacy::projection::digest(&original),
        },
    )));
    editor
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn key(editor: &mut Editor, key: KeyCode) {
        editor.handle(&Event::Key(key, KeyModifiers::NONE));
    }

    #[test]
    fn privacy_editor_selection_emits_policy_preview_and_preserves_other_rules() {
        let mut editor = test_editor();
        editor.policy.as_mut().unwrap().rules.push(Rule {
            file: "other.rs".into(),
            selector: "/rust/**".into(),
            action: RuleAction::Hide,
        });
        key(&mut editor, KeyCode::Char('u'));
        key(&mut editor, KeyCode::Char('a'));
        assert!(editor.dirty());
        let Some(Action::Preview { path, policy }) = editor.take_action() else {
            panic!("selection prepares a local preview")
        };
        assert_eq!(path, "/workspace/plan.md");
        assert_eq!(policy.action_for(&path, "/paragraph/0"), RuleAction::Allow);
        assert!(policy.rules.iter().any(|rule| rule.file == "other.rs"));
        assert!(editor.source_busy);
        key(&mut editor, KeyCode::Char('m'));
        let Some(Action::Preview { policy, .. }) = editor.take_action() else {
            panic!("alias prepares a local preview")
        };
        assert_eq!(policy.action_for(&path, "/paragraph/0"), RuleAction::Alias);
        assert_eq!(
            policy.rules.iter().filter(|rule| rule.file == path).count(),
            1
        );
    }

    #[test]
    fn privacy_editor_opaque_regions_explain_why_allow_cannot_apply() {
        let mut editor = test_editor();
        editor.source.as_mut().unwrap().projection.nodes[0].kind =
            crate::privacy::policy::AliasKind::Opaque;
        key(&mut editor, KeyCode::Char('a'));
        assert!(editor.error.as_deref().unwrap().contains("stays hidden"));
        assert!(!editor.dirty());
        let source = editor.source.as_ref().unwrap();
        assert_eq!(
            node_action(editor.policy(), source, &source.projection.nodes[0]),
            RuleAction::Hide
        );
    }

    #[test]
    fn privacy_editor_failed_save_keeps_draft_and_never_retries_automatically() {
        let mut editor = test_editor();
        key(&mut editor, KeyCode::Char('a'));
        editor.take_action();
        editor.handle(&Event::Key(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(matches!(editor.take_action(), Some(Action::Save { .. })));
        editor.save_completed(Err("Policy changed on disk; reopen and review.".into()));
        assert!(editor.dirty());
        assert!(editor.error.as_ref().unwrap().contains("changed on disk"));
        editor.handle(&Event::Tick);
        assert!(editor.take_action().is_none());
    }

    #[test]
    fn privacy_editor_escape_keeps_unsaved_choices_until_explicit_discard() {
        let mut editor = test_editor();
        key(&mut editor, KeyCode::Char('h'));
        editor.take_action();
        key(&mut editor, KeyCode::Esc);
        assert!(editor.discard_prompt);
        assert!(editor.take_action().is_none());
        key(&mut editor, KeyCode::Esc);
        assert!(editor.dirty());
        assert!(!editor.discard_prompt);
        key(&mut editor, KeyCode::Esc);
        key(&mut editor, KeyCode::Enter);
        assert!(matches!(editor.take_action(), Some(Action::Close)));
    }

    #[test]
    fn privacy_editor_path_paste_is_local_unicode_safe_and_requires_load_action() {
        let mut editor = test_editor();
        editor.path.clear();
        editor.path_cursor = 0;
        key(&mut editor, KeyCode::Char('p'));
        editor.handle(&Event::Paste("/workspace/界面🙂.html".into()));
        key(&mut editor, KeyCode::Left);
        key(&mut editor, KeyCode::Backspace);
        assert!(editor.path.is_char_boundary(editor.path_cursor));
        assert!(editor.take_action().is_none());
        key(&mut editor, KeyCode::Enter);
        assert!(matches!(
            editor.take_action(),
            Some(Action::LoadSource { .. })
        ));
    }

    #[test]
    fn privacy_editor_mouse_controls_emit_the_same_explicit_action() {
        let mut editor = test_editor();
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_with_version_label(
                    frame,
                    area,
                    &editor,
                    "ca test",
                    super::super::render::standalone_palette(),
                    RequestView {
                        generation: 0,
                        original: None,
                        sent: "",
                        changes: None,
                        latest: "",
                        opaque_blocks: 0,
                        complete: false,
                        working: false,
                    },
                )
            })
            .unwrap();
        let button = editor
            .hits
            .borrow()
            .iter()
            .find(|(_, hit)| matches!(hit, Hit::Rule(RuleAction::Allow)))
            .unwrap()
            .0;
        editor.handle(&Event::MouseSelection(
            crate::tui::event::MouseSelectionEvent {
                phase: MouseSelectionPhase::Start,
                column: button.x,
                row: button.y,
            },
        ));
        assert!(matches!(editor.take_action(), Some(Action::Preview { .. })));
        let source = editor.source.as_ref().unwrap();
        assert_eq!(
            node_action(editor.policy(), source, &source.projection.nodes[0]),
            RuleAction::Allow
        );
    }

    #[test]
    fn privacy_editor_exact_choices_expire_on_drift_and_reuse_is_explicit() {
        let mut editor = test_editor();
        key(&mut editor, KeyCode::Char('a'));
        let Some(Action::Preview { policy, .. }) = editor.take_action() else {
            panic!("preview required")
        };
        let mut source = editor.source.as_ref().unwrap().clone();
        assert_eq!(
            node_action(Some(&policy), &source, &source.projection.nodes[0]),
            RuleAction::Allow
        );
        source.projection.digest = crate::privacy::projection::digest("changed content");
        assert_eq!(
            node_action(Some(&policy), &source, &source.projection.nodes[0]),
            RuleAction::Hide
        );
        key(&mut editor, KeyCode::Char('u'));
        key(&mut editor, KeyCode::Char('a'));
        let Some(Action::Preview { policy, .. }) = editor.take_action() else {
            panic!("preview required")
        };
        assert_eq!(
            node_action(Some(&policy), &source, &source.projection.nodes[0]),
            RuleAction::Allow
        );
    }

    #[test]
    fn privacy_editor_csv_group_explicitly_selects_one_column_with_shape_guard() {
        let mut editor = test_editor();
        let shape = format!("/@shape/{}", "a".repeat(64));
        editor.source.as_mut().unwrap().projection.nodes[0].selector =
            format!("{shape}/csv/row/1/column/2");
        key(&mut editor, KeyCode::Char('g'));
        key(&mut editor, KeyCode::Char('a'));
        let Some(Action::Preview { policy, path }) = editor.take_action() else {
            panic!("column choice must preview")
        };
        assert_eq!(
            policy.action_for(&path, &format!("{shape}/csv/row/0/column/2")),
            RuleAction::Allow
        );
        assert_eq!(
            policy.action_for(&path, &format!("{shape}/csv/row/5/column/2")),
            RuleAction::Allow
        );
        assert_eq!(
            policy.action_for(&path, &format!("{shape}/csv/row/5/column/3")),
            RuleAction::Hide
        );
        assert_eq!(
            policy.action_for(
                &path,
                &format!("/@shape/{}/csv/row/5/column/2", "b".repeat(64))
            ),
            RuleAction::Hide
        );
    }

    #[test]
    fn privacy_editor_markdown_section_file_and_explicit_pattern_are_distinct() {
        let mut editor = test_editor();
        let shape = format!("/@shape/{}", "a".repeat(64));
        editor.source.as_mut().unwrap().projection.nodes[0].selector =
            format!("{shape}/markdown/section/2/paragraph/0/text/0");
        key(&mut editor, KeyCode::Char('g'));
        key(&mut editor, KeyCode::Char('a'));
        let Some(Action::Preview { policy, path }) = editor.take_action() else {
            panic!("section choice must preview")
        };
        assert_eq!(
            policy.action_for(
                &path,
                &format!("{shape}/markdown/section/2/paragraph/7/text/1")
            ),
            RuleAction::Allow
        );
        assert_eq!(
            policy.action_for(
                &path,
                &format!("{shape}/markdown/section/3/paragraph/0/text/0")
            ),
            RuleAction::Hide
        );
        key(&mut editor, KeyCode::Char('g'));
        key(&mut editor, KeyCode::Char('a'));
        let Some(Action::Preview { policy, .. }) = editor.take_action() else {
            panic!("whole file choice must preview")
        };
        assert_eq!(policy.rules.last().unwrap().selector, format!("{shape}/**"));
        key(&mut editor, KeyCode::Char('g'));
        assert!(editor.editing_pattern);
        let before = editor.policy.clone();
        key(&mut editor, KeyCode::Home);
        for _ in 0..editor.file_pattern.len() {
            key(&mut editor, KeyCode::Delete);
        }
        editor.handle(&Event::Paste("notes/*.md".into()));
        key(&mut editor, KeyCode::Enter);
        assert_eq!(
            editor.policy, before,
            "confirming a pattern alone must not broaden live draft rules"
        );
        assert!(editor.take_action().is_none());
        key(&mut editor, KeyCode::Char('a'));
        let Some(Action::Preview { policy, .. }) = editor.take_action() else {
            panic!("explicit matching-files choice must preview")
        };
        assert_eq!(policy.rules.last().unwrap().file, "notes/*.md");
        assert_eq!(
            policy.action_for(
                "notes/other.md",
                &format!("{shape}/markdown/section/3/paragraph/0/text/0")
            ),
            RuleAction::Allow
        );
        assert_eq!(
            policy.action_for(
                "notes/other.txt",
                &format!("{shape}/markdown/section/3/paragraph/0/text/0")
            ),
            RuleAction::Hide
        );
    }

    #[test]
    fn privacy_editor_range_reviews_unicode_and_emits_only_exact_version_rule() {
        let mut editor = test_editor();
        let source = editor.source.as_mut().unwrap();
        source.original = "Aé界 name\nSecond row\n".into();
        source.projection.digest = crate::privacy::projection::digest(&source.original);
        source.projection.nodes[0].range = 0..source.original.len();
        let digest = source.projection.digest.clone();
        key(&mut editor, KeyCode::Char('v'));
        key(&mut editor, KeyCode::Home);
        for _ in 0..editor.range_input.len() {
            key(&mut editor, KeyCode::Delete);
        }
        editor.handle(&Event::Paste("1:2-1:4".into()));
        key(&mut editor, KeyCode::Enter);
        assert_eq!(editor.selected_range.as_ref().unwrap().bytes, 1..6);
        assert_eq!(editor.page, Page::Preview);
        assert_eq!(editor.mode, PreviewMode::Original);
        assert!(!editor.dirty());
        assert!(
            editor.take_action().is_none(),
            "range review is entirely local"
        );
        for width in [60, 120] {
            let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    render_with_version_label(
                        frame,
                        area,
                        &editor,
                        "ca test",
                        super::super::render::standalone_palette(),
                        RequestView {
                            generation: 0,
                            original: None,
                            sent: "",
                            changes: None,
                            latest: "",
                            opaque_blocks: 0,
                            complete: false,
                            working: false,
                        },
                    );
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let rendered = (0..30)
                .map(|row| {
                    (0..width)
                        .map(|col| buffer[(col, row)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(rendered.contains("Original range"), "{rendered}");
            assert!(rendered.contains("é界"), "{rendered}");
            assert!(rendered.contains("end excluded"), "{rendered}");
        }
        key(&mut editor, KeyCode::Char('1'));
        key(&mut editor, KeyCode::Char('u'));
        assert!(!editor.reuse_selector);
        key(&mut editor, KeyCode::Char('g'));
        assert_eq!(editor.rule_scope, RuleScope::Value);
        key(&mut editor, KeyCode::Char('f'));
        assert!(!editor.editing_pattern);
        key(&mut editor, KeyCode::Char('a'));
        let Some(Action::Preview { policy, path }) = editor.take_action() else {
            panic!("exact range must preview");
        };
        assert_eq!(
            policy.rules.last().unwrap().selector,
            format!("/@digest/{digest}/1/6")
        );
        assert_eq!(policy.rules.last().unwrap().file, globset::escape(&path));
        let mut changed = editor.source.as_ref().unwrap().clone();
        changed.original.insert(0, 'X');
        changed.projection.digest = crate::privacy::projection::digest(&changed.original);
        let changed_exact = format!("/@digest/{}/1/6", changed.projection.digest);
        editor.source_completed(Ok(changed));
        assert!(editor.selected_range.is_none());
        assert_eq!(
            policy
                .for_path(&path)
                .action_for_exact("/new/node", &changed_exact),
            RuleAction::Hide
        );
        key(&mut editor, KeyCode::Char('r'));
        assert!(
            editor.request_target,
            "the existing Request shortcut must remain unchanged"
        );
    }

    #[test]
    fn privacy_editor_range_rejects_invalid_positions_and_clears_explicitly() {
        assert_eq!(
            parse_source_range("Aé界\r\nnext\n", "1:2-2:3").unwrap(),
            1..10
        );
        for input in [
            "0:1-1:2", "1:0-1:2", "1:4-1:2", "1:2-1:2", "1:99-2:1", "9:1-9:2", "1:2",
        ] {
            assert!(
                parse_source_range("Aé界\nnext\n", input).is_err(),
                "{input}"
            );
        }
        let mut editor = test_editor();
        key(&mut editor, KeyCode::Char('v'));
        key(&mut editor, KeyCode::Enter);
        assert!(editor.selected_range.is_some());
        key(&mut editor, KeyCode::Char('v'));
        key(&mut editor, KeyCode::Home);
        for _ in 0..editor.range_input.len() {
            key(&mut editor, KeyCode::Delete);
        }
        key(&mut editor, KeyCode::Enter);
        assert!(editor.selected_range.is_none());
        key(&mut editor, KeyCode::Char('1'));
        key(&mut editor, KeyCode::Char('u'));
        assert!(
            editor.reuse_selector,
            "an explicit structural choice can opt into reuse after clearing"
        );
        assert!(editor.take_action().is_none());
    }

    #[test]
    fn privacy_editor_exact_request_keeps_newest_visible_and_every_window_reachable() {
        fn draw(editor: &Editor, generation: u64, sent: &str, latest: &str) -> String {
            let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    render_with_version_label(
                        frame,
                        area,
                        editor,
                        "ca test",
                        super::super::render::standalone_palette(),
                        RequestView {
                            generation,
                            original: Some(sent),
                            sent,
                            changes: Some("No changes."),
                            latest,
                            opaque_blocks: 0,
                            complete: true,
                            working: true,
                        },
                    );
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            (0..30)
                .map(|row| {
                    (0..100)
                        .map(|column| buffer[(column, row)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        let mut editor = test_editor();
        key(&mut editor, KeyCode::Char('r'));
        let sent = format!("{}\nTAIL-OF-EXACT-BODY", "old history row\n".repeat(20_000));
        let first = draw(&editor, 7, &sent, "FIRST-EXACT-PENDING");
        assert!(first.contains("FIRST-EXACT-PENDING"));
        assert!(first.contains("Exact final request"));
        key(&mut editor, KeyCode::End);
        let tail = draw(&editor, 7, &sent, "FIRST-EXACT-PENDING");
        assert!(tail.contains("TAIL-OF-EXACT-BODY"), "{tail}");
        assert!(tail.contains("bytes"));
        let second = draw(&editor, 8, &sent, "SECOND-EXACT-PENDING");
        assert!(second.contains("SECOND-EXACT-PENDING"));
        assert!(!second.contains("FIRST-EXACT-PENDING"));
        assert_eq!(editor.window.get(), 0);
        assert_eq!(editor.scroll.get(), 0);
    }
}
