//! Local privacy policy editor. Parsing, persistence and provider work belong
//! to its owner; this module emits actions and renders already prepared views.
//! Drag on Original or use V to select an exact-version range. R inspects requests.

use crate::privacy::policy::{Action as RuleAction, Policy, Rule};
use crate::privacy::projection::Projection;
use crate::tui::event::{MouseSelectionEvent, MouseSelectionPhase, ScrollDirection};
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

mod guide;
mod source_selection;
pub(crate) mod suggestions;

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub policy: Policy,
    pub effective: Policy,
    pub filename_only: bool,
    pub override_note: Option<String>,
    pub active_now: String,
    pub profile: crate::provider_filter::ProviderFilterConfig,
    pub legacy_report: String,
    pub observed_sources: Vec<String>,
    pub recent_sources: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SourceView {
    pub path: String,
    pub original: String,
    pub projection: Projection,
    changes: Arc<str>,
    node_lines: Vec<usize>,
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
        let mut position = 0;
        let mut line = 1;
        let node_lines = projection
            .nodes
            .iter()
            .map(|node| {
                line += original
                    .as_bytes()
                    .get(position..node.range.start)
                    .unwrap_or_default()
                    .iter()
                    .filter(|&&byte| byte == b'\n')
                    .count();
                position = node.range.start;
                line
            })
            .collect();
        Self {
            path,
            original,
            projection,
            changes: Arc::from(changes),
            node_lines,
        }
    }

    fn treatment(&self, node: &crate::privacy::projection::Node) -> RuleAction {
        self.projection
            .changes
            .binary_search_by_key(&node.range.start, |change| change.range.start)
            .map_or(RuleAction::Allow, |index| {
                self.projection.changes[index].action
            })
    }

    fn excerpt(&self, range: std::ops::Range<usize>, limit: usize) -> String {
        let text = self.original.get(range).unwrap_or("");
        let bounded: String = text.chars().take(limit).collect();
        let excerpt = bounded.split_whitespace().collect::<Vec<_>>().join(" ");
        crate::terminal_text::sanitize_terminal_text(&excerpt)
    }
}

pub(super) struct RequestView<'a> {
    pub generation: u64,
    pub original: Option<&'a str>,
    pub sent: &'a str,
    pub changes: Option<&'a str>,
    pub latest: &'a str,
    pub sources: &'a [crate::privacy::SourceLink],
    pub opaque_blocks: usize,
    pub complete: bool,
    pub working: bool,
}

#[derive(Debug, Clone)]
pub enum Action {
    PreviewTask {
        policy: crate::task_filter::Policy,
        generation: u64,
    },
    SendTask {
        policy: crate::task_filter::Policy,
        remember: bool,
        generation: u64,
    },
    BreakdownTask,
    ContinueTask {
        words: Vec<String>,
        identifiers: Vec<String>,
        hide_urls: bool,
    },
    AnalyzeGuide(suggestions::GuideInput),
    LoadSnapshot,
    Close,
    /// Turn the pre-send review off for this process and restore a held draft.
    DisablePreview,
    LoadSource {
        path: String,
        policy: Policy,
    },
    Preview {
        path: String,
        policy: Policy,
    },
    Save {
        policy: Policy,
    },
    SaveProfile {
        expected: Box<crate::provider_filter::ProviderFilterConfig>,
        draft: Box<crate::provider_filter::ProviderFilterConfig>,
    },
    IndexFiles {
        generation: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Guide,
    Overview,
    Files,
    Sources,
    Rules,
    Preview,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewMode {
    Sent,
    Original,
    Changes,
    Request,
    FileDraft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleScope {
    Value,
    Group,
    File,
    MatchingFiles,
}

const SCOPE_CHOICES: [(RuleScope, bool, &str); 5] = [
    (RuleScope::Value, false, "Value in this version"),
    (RuleScope::Value, true, "Value when structure matches"),
    (RuleScope::Group, false, "Containing group"),
    (RuleScope::File, false, "Whole file"),
    (RuleScope::MatchingFiles, false, "Files matching a pattern"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingId {
    Enabled,
    Structural,
    Filenames,
    AllUrls,
    Images,
    HideWords,
    HidePairs,
    HideUrls,
    StopPhrases,
    StopUrls,
    StopRules,
    TripFile,
    Pages,
    PageSymbols,
    LegacyHeader,
    LegacyPhrases,
    LegacyUrls,
    LegacyRules,
    LegacyAdjustments,
    Obfuscate,
    ObfuscationPatterns,
    PageStandIn,
    PageStandIns,
}

#[derive(Debug, Clone, Copy)]
enum Hit {
    TaskPreset,
    TaskView(u8),
    TaskRemember,
    TaskAdvanced,
    GuideRow(usize),
    GuideApply,
    GuideUnchanged,
    GuideBreakdown,
    Page(Page),
    Path,
    Load,
    Observed(usize),
    Node(usize),
    Rule(RuleAction),
    Scope,
    Group,
    ScopeChoice(usize),
    Pattern,
    Range,
    Mode(PreviewMode),
    Save,
    SaveProfile,
    Close,
    Overview(Page),
    FileRow(usize),
    FileSearch,
    RuleRow(usize),
    RuleAdd,
    RuleRemove,
    RuleOpenSource,
    RuleField(RuleField),
    RuleTreatment(RuleAction),
    RuleCancel,
    RuleEarlier,
    RuleLater,
    RuleSearch,
    ProjectEnabled,
    DefaultAction,
    Setting(SettingId),
    RequestSource(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleField {
    File,
    Action,
    Advanced,
    Selector,
    Apply,
}

#[derive(Debug)]
struct RuleForm {
    index: Option<usize>,
    draft: Rule,
    field: RuleField,
    cursor: usize,
    advanced: bool,
}

impl RuleForm {
    fn fields(&self) -> &[RuleField] {
        if self.advanced {
            &[
                RuleField::File,
                RuleField::Action,
                RuleField::Advanced,
                RuleField::Selector,
                RuleField::Apply,
            ]
        } else {
            &[
                RuleField::File,
                RuleField::Action,
                RuleField::Advanced,
                RuleField::Apply,
            ]
        }
    }

    fn focus(&mut self, field: RuleField) {
        self.field = field;
        self.cursor = match field {
            RuleField::File => self.draft.file.len(),
            RuleField::Selector => self.draft.selector.len(),
            _ => 0,
        };
    }

    fn text_mut(&mut self) -> Option<&mut String> {
        match self.field {
            RuleField::File => Some(&mut self.draft.file),
            RuleField::Selector => Some(&mut self.draft.selector),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct SelectedRange {
    path: String,
    digest: String,
    bytes: std::ops::Range<usize>,
    typed_coordinates: Option<String>,
}

#[derive(Debug)]
pub struct Editor {
    guide: Option<guide::Guide>,
    task_preview: Option<guide::TaskPreview>,
    workspace: String,
    snapshot: Option<Snapshot>,
    policy: Option<Policy>,
    profile: crate::provider_filter::ProviderFilterConfig,
    source: Option<SourceView>,
    page: Page,
    mode: PreviewMode,
    request_target: bool,
    reuse_selector: bool,
    rule_scope: RuleScope,
    scope_choice: Option<usize>,
    file_pattern: String,
    pattern_cursor: usize,
    editing_pattern: bool,
    confirmed_pattern: Option<String>,
    selected_range: Option<SelectedRange>,
    source_drag: Option<source_selection::Drag>,
    original_hits: RefCell<source_selection::HitMap>,
    range_input: String,
    range_cursor: usize,
    editing_range: bool,
    selected: usize,
    path: String,
    path_cursor: usize,
    editing_path: bool,
    overview_selected: usize,
    files_query: String,
    files_cursor: usize,
    editing_files: bool,
    files_selected: usize,
    indexed_files: Vec<String>,
    index_generation: u64,
    index_note: Option<String>,
    rules_query: String,
    rules_cursor: usize,
    editing_rules_query: bool,
    rules_selected: usize,
    rule_form: Option<RuleForm>,
    settings_selected: usize,
    editing_setting: bool,
    setting_input: String,
    setting_cursor: usize,
    legacy_open: bool,
    expected_digest: Option<String>,
    opened_current: bool,
    preview_token: u64,
    replace_project: bool,
    replace_profile: bool,
    conflict_project: bool,
    conflict_profile: bool,
    link_selected: usize,
    request_sources: RefCell<Vec<(String, String)>>,
    loading: bool,
    source_busy: bool,
    source_stale: bool,
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
    /// Session review gate. Render copies `AppState.task_filter_preview_enabled`.
    review_gate: Cell<bool>,
}

impl Editor {
    pub fn loading(workspace: String) -> Self {
        Self {
            guide: None,
            task_preview: None,
            workspace,
            snapshot: None,
            policy: None,
            profile: crate::provider_filter::ProviderFilterConfig::default(),
            source: None,
            page: Page::Overview,
            mode: PreviewMode::Sent,
            request_target: false,
            reuse_selector: false,
            rule_scope: RuleScope::Value,
            scope_choice: None,
            file_pattern: String::new(),
            pattern_cursor: 0,
            editing_pattern: false,
            confirmed_pattern: None,
            selected_range: None,
            source_drag: None,
            original_hits: RefCell::default(),
            range_input: String::new(),
            range_cursor: 0,
            editing_range: false,
            selected: 0,
            path: String::new(),
            path_cursor: 0,
            editing_path: false,
            overview_selected: 0,
            files_query: String::new(),
            files_cursor: 0,
            editing_files: false,
            files_selected: 0,
            indexed_files: Vec::new(),
            index_generation: 0,
            index_note: None,
            rules_query: String::new(),
            rules_cursor: 0,
            editing_rules_query: false,
            rules_selected: 0,
            rule_form: None,
            settings_selected: 0,
            editing_setting: false,
            setting_input: String::new(),
            setting_cursor: 0,
            legacy_open: false,
            expected_digest: None,
            opened_current: false,
            preview_token: 0,
            replace_project: false,
            replace_profile: false,
            conflict_project: false,
            conflict_profile: false,
            link_selected: 0,
            request_sources: RefCell::new(Vec::new()),
            loading: true,
            source_busy: false,
            source_stale: false,
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
            review_gate: Cell::new(true),
        }
    }

    pub(super) fn set_review_gate(&self, enabled: bool) {
        self.review_gate.set(enabled);
    }

    #[cfg(test)]
    pub fn from_snapshot(workspace: String, snapshot: Snapshot) -> Self {
        let mut editor = Self::loading(workspace);
        editor.policy = Some(snapshot.policy.clone());
        editor.profile = snapshot.profile.clone();
        editor.snapshot = Some(snapshot);
        editor.loading = false;
        editor.actions.clear();
        editor
    }

    pub fn load_completed(&mut self, result: Result<Snapshot, String>) {
        self.loading = false;
        match result {
            Ok(snapshot) => {
                if !self.dirty_project() {
                    self.policy = Some(snapshot.policy.clone());
                } else if let Some(policy) = &mut self.policy {
                    policy.revision = snapshot.policy.revision;
                }
                if !self.dirty_profile() {
                    self.profile = snapshot.profile.clone();
                }
                self.snapshot = Some(snapshot);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn source_completed(&mut self, result: Result<SourceView, String>) {
        self.apply_source(self.preview_token, result);
    }

    pub fn apply_source(&mut self, token: u64, result: Result<SourceView, String>) {
        if token != self.preview_token {
            return;
        }
        self.source_busy = false;
        match result {
            Ok(source) => {
                let same_source = self.source.as_ref().is_some_and(|previous| {
                    previous.path == source.path
                        && previous.projection.digest == source.projection.digest
                });
                let digest_changed = self.selected_range.as_ref().is_some_and(|range| {
                    range.path != source.path || range.digest != source.projection.digest
                });
                self.opened_current = self
                    .expected_digest
                    .as_ref()
                    .is_some_and(|digest| digest != &source.projection.digest);
                if digest_changed {
                    self.selected_range = None;
                    self.editing_range = false;
                    self.notice = Some("The source changed. Select and review a new range; the previous rule applies only to its original file version.".into());
                } else if self.opened_current {
                    self.notice = Some(
                        "Opened copy is current. This file has changed since that request.".into(),
                    );
                }
                self.path = source.path.clone();
                self.path_cursor = self.path.len();
                self.selected = self
                    .selected
                    .min(source.projection.nodes.len().saturating_sub(1));
                self.source = Some(source);
                self.source_stale = false;
                if !same_source {
                    self.source_drag = None;
                    self.reset_scroll();
                }
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn apply_index(
        &mut self,
        generation: u64,
        paths: Vec<String>,
        truncated: bool,
        error: Option<String>,
    ) {
        if generation != self.index_generation {
            return;
        }
        self.indexed_files = paths;
        self.index_note = match error {
            Some(error) => Some(error),
            None if truncated => Some("File list is capped. Use Path for other files.".into()),
            None => None,
        };
    }

    pub fn save_completed(&mut self, result: Result<Snapshot, String>) {
        self.saving = false;
        match result {
            Ok(snapshot) => {
                self.conflict_project = false;
                self.replace_project = false;
                if let Some(current) = &mut self.snapshot {
                    current.policy = snapshot.policy.clone();
                    current.effective = snapshot.effective.clone();
                    current.filename_only = snapshot.filename_only;
                    current.override_note = snapshot.override_note.clone();
                    current.active_now = snapshot.active_now.clone();
                    current.legacy_report = snapshot.legacy_report.clone();
                    current.observed_sources = snapshot.observed_sources.clone();
                    current.recent_sources = snapshot.recent_sources.clone();
                } else {
                    self.snapshot = Some(snapshot.clone());
                }
                self.policy = Some(snapshot.policy);
                self.error = None;
                if self.source_stale {
                    self.preview();
                }
                self.notice = Some("Saved project rules. The next request uses them.".into());
            }
            Err(error) => {
                self.source_busy = false;
                if error.contains("changed") || error.contains("reload") {
                    self.conflict_project = true;
                }
                self.error = Some(error);
                self.notice = None;
            }
        }
    }

    pub fn profile_save_completed(
        &mut self,
        result: Result<crate::provider_filter::ProviderFilterConfig, String>,
    ) {
        self.saving = false;
        match result {
            Ok(config) => {
                self.conflict_profile = false;
                self.replace_profile = false;
                if let Some(snapshot) = &mut self.snapshot {
                    snapshot.profile = config.clone();
                }
                self.profile = config;
                self.error = None;
                self.notice = Some(
                    "Saved for the next launch. Restart ca to use these profile settings.".into(),
                );
            }
            Err(error) => {
                if error.contains("changed") || error.contains("reload") {
                    self.conflict_profile = true;
                }
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

    fn dirty_project(&self) -> bool {
        match (&self.policy, &self.snapshot) {
            (Some(policy), Some(snapshot)) => policy != &snapshot.policy,
            _ => false,
        }
    }

    fn dirty_profile(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|snapshot| self.profile != snapshot.profile)
    }

    pub fn dirty(&self) -> bool {
        self.dirty_project() || self.dirty_profile()
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
        self.preview_token = self.preview_token.wrapping_add(1);
        self.actions
            .retain(|action| !matches!(action, Action::Preview { .. }));
        self.actions.push_back(Action::Preview {
            path: source.path.clone(),
            policy: policy.clone(),
        });
        self.source_busy = true;
        self.source_stale = true;
        self.error = None;
        self.notice = None;
    }

    fn set_rule(&mut self, action: RuleAction) {
        if self.source_drag.is_some() {
            return;
        }
        let (Some(source), Some(policy)) = (&self.source, &mut self.policy) else {
            return;
        };
        let node = source.projection.nodes.get(self.selected);
        let effective_default = self
            .snapshot
            .as_ref()
            .map_or(policy.default_action, |snapshot| {
                if snapshot.effective.default_action != snapshot.policy.default_action {
                    snapshot.effective.default_action
                } else {
                    policy.default_action
                }
            });
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
        if opaque && action == RuleAction::Allow && effective_default != RuleAction::Allow {
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
            if !source.projection.nodes.iter().any(|node| {
                node.range.start < range.bytes.end && range.bytes.start < node.range.end
            }) {
                self.error = Some("Select a value or text that the filter can change.".into());
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

    fn apply_rule_form(&mut self) {
        let (Some(form), Some(policy)) = (&self.rule_form, &mut self.policy) else {
            return;
        };
        let mut next = policy.clone();
        if let Some(index) = form.index {
            next.rules[index] = form.draft.clone();
        } else {
            next.rules.push(form.draft.clone());
        }
        if let Err(error) = next.validate() {
            self.error = Some(if form.draft.file.trim().is_empty() {
                "Enter a file path or a pattern such as docs/*.md.".into()
            } else {
                error
            });
            return;
        }
        self.rules_selected = form.index.unwrap_or(next.rules.len() - 1);
        *policy = next;
        self.rule_form = None;
        self.rules_query.clear();
        self.rules_cursor = 0;
        self.error = None;
        self.preview();
    }

    fn rule_field(&mut self, field: RuleField) {
        let Some(form) = &mut self.rule_form else {
            return;
        };
        match field {
            RuleField::Apply => self.apply_rule_form(),
            RuleField::Advanced => {
                form.advanced = !form.advanced;
                form.focus(if form.advanced {
                    RuleField::Selector
                } else {
                    RuleField::Advanced
                });
            }
            _ => form.focus(field),
        }
    }

    fn edit_rule_key(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        let Some(form) = &mut self.rule_form else {
            return;
        };
        match key {
            KeyCode::Esc => {
                self.rule_form = None;
                self.error = None;
            }
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                let fields = form.fields();
                let current = fields
                    .iter()
                    .position(|&field| field == form.field)
                    .unwrap_or(0);
                let next = if matches!(key, KeyCode::BackTab | KeyCode::Up) {
                    (current + fields.len() - 1) % fields.len()
                } else {
                    (current + 1) % fields.len()
                };
                form.focus(fields[next]);
            }
            KeyCode::Enter => match form.field {
                RuleField::File => form.focus(RuleField::Action),
                RuleField::Action | RuleField::Selector => form.focus(RuleField::Apply),
                field => self.rule_field(field),
            },
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                if form.field == RuleField::Action =>
            {
                let actions = [RuleAction::Hide, RuleAction::Alias, RuleAction::Allow];
                let current = actions
                    .iter()
                    .position(|&action| action == form.draft.action)
                    .unwrap_or(0);
                let step = if key == KeyCode::Left { 2 } else { 1 };
                form.draft.action = actions[(current + step) % actions.len()];
            }
            key if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                let mut cursor = form.cursor;
                if let Some(text) = form.text_mut() {
                    match key {
                        KeyCode::Left => cursor = previous_grapheme(text, cursor),
                        KeyCode::Right => cursor = next_grapheme(text, cursor),
                        KeyCode::Home => cursor = 0,
                        KeyCode::End => cursor = text.len(),
                        KeyCode::Backspace => {
                            let previous = previous_grapheme(text, cursor);
                            text.replace_range(previous..cursor, "");
                            cursor = previous;
                        }
                        KeyCode::Delete => {
                            let next = next_grapheme(text, cursor);
                            text.replace_range(cursor..next, "");
                        }
                        KeyCode::Char(character) => {
                            Self::insert_limited(text, &mut cursor, &character.to_string(), 4096)
                        }
                        _ => {}
                    }
                    form.cursor = cursor;
                    self.error = None;
                }
            }
            _ => {}
        }
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
        let coordinates = self.range_input.trim().to_owned();
        self.select_bytes(bytes, Some(coordinates));
        self.editing_range = false;
        self.page = Page::Preview;
        self.mode = PreviewMode::Original;
        self.request_target = false;
        self.reset_scroll();
        self.error = None;
        self.notice = Some(format!(
            "Review {} (end excluded). A Allow · M Alias · H Hide. This file version only.",
            self.range_input.trim()
        ));
    }

    fn select_bytes(&mut self, bytes: std::ops::Range<usize>, typed_coordinates: Option<String>) {
        let Some(source) = &self.source else { return };
        self.selected_range = Some(SelectedRange {
            path: source.path.clone(),
            digest: source.projection.digest.clone(),
            bytes,
            typed_coordinates,
        });
        self.rule_scope = RuleScope::Value;
        self.reuse_selector = false;
        self.error = None;
        self.notice = None;
    }

    fn file_original(&self) -> bool {
        self.page == Page::Preview && !self.request_target && self.mode == PreviewMode::Original
    }

    fn can_apply_rule(&self) -> bool {
        self.page == Page::Sources || (self.file_original() && self.selected_range.is_some())
    }

    fn select_source_text(&mut self, event: &MouseSelectionEvent) -> bool {
        if self.editing_path || self.editing_range || self.editing_pattern {
            return false;
        }
        if event.phase == MouseSelectionPhase::Start {
            if !self
                .original_hits
                .get_mut()
                .contains(event.column, event.row)
            {
                self.source_drag = None;
                return false;
            }
            self.selected_range = None;
            self.error = None;
            self.notice = None;
            self.source_drag = self
                .original_hits
                .get_mut()
                .bytes_at(event.column, event.row, false)
                .map(|anchor| source_selection::Drag {
                    focus: anchor.clone(),
                    anchor,
                    moved: false,
                });
            return true;
        }
        if self.original_hits.get_mut().is_empty() {
            self.source_drag = None;
            return false;
        }
        let Some(drag) = &mut self.source_drag else {
            return false;
        };
        if let Some(focus) = self
            .original_hits
            .get_mut()
            .bytes_at(event.column, event.row, true)
        {
            drag.moved |= focus != drag.anchor || event.phase == MouseSelectionPhase::Drag;
            drag.focus = focus;
        }
        if event.phase == MouseSelectionPhase::End {
            let drag = self.source_drag.take().unwrap();
            if drag.moved {
                self.select_bytes(drag.bytes(), None);
            }
        }
        true
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

    fn choose_scope(&mut self) {
        if self.selected_range.is_some() {
            self.error = Some(
                "A selected range uses this file version. Choose a value to change scope.".into(),
            );
            return;
        }
        self.scope_choice = SCOPE_CHOICES
            .iter()
            .position(|&(scope, reuse, _)| scope == self.rule_scope && reuse == self.reuse_selector)
            .or(Some(0));
    }

    fn apply_scope(&mut self, index: usize) {
        let (scope, reuse, _) = SCOPE_CHOICES[index];
        self.rule_scope = scope;
        self.reuse_selector = reuse;
        self.scope_choice = None;
        if scope == RuleScope::MatchingFiles && self.confirmed_pattern.is_none() {
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
            self.preview_token = self.preview_token.wrapping_add(1);
            self.actions.retain(|action| {
                !matches!(action, Action::Preview { .. } | Action::LoadSource { .. })
            });
            self.source = None;
            self.selected_range = None;
            self.source_drag = None;
            self.original_hits.get_mut().clear();
            self.actions.push_back(Action::LoadSource {
                path: path.into(),
                policy,
            });
            self.editing_path = false;
            self.editing_range = false;
            self.source_busy = true;
            self.page = Page::Sources;
            self.request_target = false;
            self.rule_scope = RuleScope::Value;
            self.reuse_selector = false;
            self.selected = 0;
            self.reset_scroll();
            self.error = None;
        }
    }

    fn open_indexed(&mut self, path: String, digest: Option<String>) {
        self.expected_digest = digest;
        self.opened_current = false;
        self.path = path;
        self.path_cursor = self.path.len();
        self.editing_files = false;
        self.load_source();
    }

    fn open_files(&mut self) {
        self.page = Page::Files;
        self.editing_files = true;
        self.editing_path = false;
        self.editing_pattern = false;
        self.editing_range = false;
        self.rule_form = None;
        self.editing_setting = false;
        self.editing_rules_query = false;
        self.files_cursor = self.files_query.len();
        static NEXT_FILE_INDEX: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.index_generation = NEXT_FILE_INDEX.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.index_note = Some("Searching project files…".into());
        self.actions.push_back(Action::IndexFiles {
            generation: self.index_generation,
        });
        self.reset_scroll();
        self.error = None;
    }

    fn save(&mut self) {
        if self.conflict_project {
            self.replace_project = true;
            self.notice = Some(
                "Replace the saved project rules with this draft? Enter replaces them. Esc keeps editing."
                    .into(),
            );
            return;
        }
        if let Some(policy) = self.policy.clone() {
            self.actions
                .retain(|action| !matches!(action, Action::Preview { .. }));
            if self.source.is_none() && self.source_busy {
                self.error = Some("Wait for the file to open before saving.".into());
                return;
            }
            self.actions.push_back(Action::Save { policy });
            self.saving = true;
            self.error = None;
            self.notice = None;
        }
    }

    fn save_profile(&mut self) {
        if let Some((field, message)) = self.profile_field_error(None) {
            self.error = Some(format!("{field}: {message}"));
            return;
        }
        if self.conflict_profile {
            self.replace_profile = true;
            self.notice = Some(
                "Replace the saved profile settings with this draft? Enter replaces them. Esc keeps editing."
                    .into(),
            );
            return;
        }
        let Some(snapshot) = &self.snapshot else {
            self.error = Some("Profile settings are still loading.".into());
            return;
        };
        self.actions.push_back(Action::SaveProfile {
            expected: Box::new(snapshot.profile.clone()),
            draft: Box::new(self.profile.clone()),
        });
        self.saving = true;
        self.error = None;
        self.notice = None;
    }

    fn hit(&mut self, hit: Hit) {
        match hit {
            Hit::TaskPreset => self.cycle_task_preset(),
            Hit::TaskView(view) => self.select_task_view(view),
            Hit::TaskRemember => self.toggle_task_remember(),
            Hit::TaskAdvanced => self.open_task_advanced(),
            Hit::GuideRow(index) => self.toggle_guide_row(index),
            Hit::GuideApply => self.continue_guide(true),
            Hit::GuideUnchanged => self.continue_guide(false),
            Hit::GuideBreakdown => self.breakdown_guide(),
            Hit::Page(page) => self.show_page(page),
            Hit::Overview(page) => self.show_page(page),
            Hit::Path => {
                self.editing_files = false;
                self.editing_pattern = false;
                self.editing_range = false;
                self.editing_path = true;
                self.path_cursor = self.path.len();
            }
            Hit::Load if self.editing_range => self.confirm_range(),
            Hit::Load if self.editing_pattern => self.confirm_pattern(),
            Hit::Load if self.page == Page::Files && !self.editing_path => {
                self.hit(Hit::FileRow(self.files_selected));
            }
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
            Hit::Group => self.choose_scope(),
            Hit::ScopeChoice(index) => self.apply_scope(index),
            Hit::Pattern => self.edit_pattern(),
            Hit::Range => self.edit_range(),
            Hit::Mode(mode) => {
                if mode == PreviewMode::Request {
                    self.request_target = !self.request_target;
                    self.mode = PreviewMode::Sent;
                } else if mode == PreviewMode::FileDraft {
                    self.request_target = false;
                    self.mode = if self.selected_range.is_some() {
                        PreviewMode::Original
                    } else {
                        PreviewMode::Sent
                    };
                } else {
                    self.mode = mode;
                }
                self.page = Page::Preview;
                self.reset_scroll();
            }
            Hit::Save => self.save(),
            Hit::SaveProfile => self.save_profile(),
            Hit::Close if self.is_task_preview() => {
                self.actions.clear();
                self.actions.push_back(Action::Close);
            }
            Hit::Close => self.close(),
            Hit::FileSearch => {
                self.editing_path = false;
                self.editing_files = true;
                self.files_cursor = self.files_query.len();
            }
            Hit::FileRow(index) => {
                if let Some(path) = self.file_rows().get(index).cloned() {
                    self.files_selected = index;
                    self.open_indexed(path, None);
                }
            }
            Hit::RuleSearch => {
                self.editing_rules_query = true;
                self.rules_cursor = self.rules_query.len();
            }
            Hit::RuleRow(index) => self.activate_rule(index),
            Hit::RuleAdd => self.add_rule(),
            Hit::RuleRemove => self.remove_rule(),
            Hit::RuleOpenSource => self.open_rule_source(),
            Hit::RuleField(field) => self.rule_field(field),
            Hit::RuleTreatment(action) => {
                if let Some(form) = &mut self.rule_form {
                    form.draft.action = action;
                    form.focus(RuleField::Action);
                }
            }
            Hit::RuleCancel => {
                self.rule_form = None;
                self.error = None;
            }
            Hit::RuleEarlier => self.move_rule(false),
            Hit::RuleLater => self.move_rule(true),
            Hit::ProjectEnabled => self.toggle_enabled(),
            Hit::DefaultAction => self.cycle_default_action(),
            Hit::Setting(id) => self.activate_setting(id),
            Hit::RequestSource(index) => self.open_request_source(index),
        }
    }

    fn show_page(&mut self, page: Page) {
        self.page = page;
        self.scope_choice = None;
        self.editing_files = false;
        self.editing_path = false;
        self.editing_pattern = false;
        self.editing_range = false;
        self.editing_rules_query = false;
        self.rule_form = None;
        self.editing_setting = false;
        match page {
            Page::Files => self.open_files(),
            Page::Preview => {
                self.request_target = true;
                self.mode = PreviewMode::Sent;
            }
            Page::Sources => self.request_target = false,
            Page::Rules | Page::Settings | Page::Overview | Page::Guide => {}
        }
        self.reset_scroll();
    }

    fn overview_page(&self) -> Page {
        match self.overview_selected {
            0 => Page::Files,
            1 => Page::Rules,
            2 => Page::Preview,
            _ => Page::Settings,
        }
    }

    fn file_rows(&self) -> Vec<String> {
        let query = self.files_query.to_lowercase();
        let mut rows = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let recent = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.recent_sources.clone())
            .unwrap_or_default();
        for path in recent {
            if (query.is_empty() || path_matches(&path, &query)) && seen.insert(path.clone()) {
                rows.push(path);
            }
        }
        for path in &self.indexed_files {
            if path_matches(path, &query) && seen.insert(path.clone()) {
                rows.push(path.clone());
            }
            if rows.len() >= 200 {
                break;
            }
        }
        rows
    }

    fn visible_rules(&self) -> Vec<usize> {
        let Some(policy) = &self.policy else {
            return Vec::new();
        };
        let query = self.rules_query.to_lowercase();
        policy
            .rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| {
                query.is_empty()
                    || rule.file.to_lowercase().contains(&query)
                    || rule.selector.to_lowercase().contains(&query)
                    || action_name(rule.action).to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn activate_rule(&mut self, index: usize) {
        let Some(rule) = self
            .policy
            .as_ref()
            .and_then(|policy| policy.rules.get(index))
            .cloned()
        else {
            return;
        };
        self.rules_selected = index;
        self.begin_rule_form(Some(index), rule);
    }

    fn begin_rule_form(&mut self, index: Option<usize>, draft: Rule) {
        self.editing_rules_query = false;
        self.error = None;
        self.rule_form = Some(RuleForm {
            index,
            cursor: draft.file.len(),
            draft,
            field: RuleField::File,
            advanced: false,
        });
    }

    fn add_rule(&mut self) {
        if self.policy.is_none() {
            return;
        }
        self.begin_rule_form(
            None,
            Rule {
                file: self
                    .source
                    .as_ref()
                    .map_or_else(String::new, |source| globset::escape(&source.path)),
                selector: "/**".into(),
                action: RuleAction::Hide,
            },
        );
    }

    fn open_rule_source(&mut self) {
        if !self.visible_rules().contains(&self.rules_selected) {
            return;
        }
        let Some(rule) = self
            .policy
            .as_ref()
            .and_then(|policy| policy.rules.get(self.rules_selected))
        else {
            return;
        };
        if let Some(path) = concrete_rule_file(&rule.file) {
            self.open_indexed(path.to_string(), None);
        } else {
            self.error =
                Some("This rule matches a pattern. Open a matching file from Files.".into());
        }
    }

    fn move_rule_selection(&mut self, later: bool) {
        let visible = self.visible_rules();
        let position = visible
            .iter()
            .position(|&index| index == self.rules_selected);
        let next = match position {
            Some(position) if later => position.saturating_add(1).min(visible.len() - 1),
            Some(position) => position.saturating_sub(1),
            None => 0,
        };
        if let Some(&index) = visible.get(next) {
            self.rules_selected = index;
        }
    }

    fn remove_rule(&mut self) {
        if !self.visible_rules().contains(&self.rules_selected) {
            return;
        }
        let Some(policy) = &mut self.policy else {
            return;
        };
        if policy.rules.is_empty() {
            return;
        }
        let index = self.rules_selected.min(policy.rules.len() - 1);
        policy.rules.remove(index);
        self.rules_selected = index.min(policy.rules.len().saturating_sub(1));
        self.rule_form = None;
        self.preview();
    }

    fn move_rule(&mut self, later: bool) {
        if !self.visible_rules().contains(&self.rules_selected) {
            return;
        }
        let Some(policy) = &mut self.policy else {
            return;
        };
        if policy.rules.is_empty() {
            return;
        }
        let index = self.rules_selected.min(policy.rules.len() - 1);
        let next = if later {
            index.saturating_add(1)
        } else {
            index.saturating_sub(1)
        };
        if next >= policy.rules.len() || next == index {
            return;
        }
        policy.rules.swap(index, next);
        self.rules_selected = next;
        self.preview();
    }

    fn cycle_default_action(&mut self) {
        if let Some(policy) = &mut self.policy {
            policy.default_action = match policy.default_action {
                RuleAction::Hide => RuleAction::Alias,
                RuleAction::Alias => RuleAction::Allow,
                RuleAction::Allow => RuleAction::Hide,
            };
            self.preview();
        }
    }

    fn open_request_source(&mut self, index: usize) {
        let Some((path, digest)) = self.request_sources.borrow().get(index).cloned() else {
            return;
        };
        self.link_selected = index;
        self.open_indexed(path, Some(digest));
    }

    fn setting_ids(&self) -> Vec<SettingId> {
        let mut ids = vec![
            SettingId::Enabled,
            SettingId::Structural,
            SettingId::Filenames,
            SettingId::AllUrls,
            SettingId::Images,
            SettingId::HideWords,
            SettingId::HidePairs,
            SettingId::HideUrls,
            SettingId::StopPhrases,
            SettingId::StopUrls,
            SettingId::StopRules,
            SettingId::TripFile,
            SettingId::Pages,
            SettingId::PageSymbols,
            SettingId::LegacyHeader,
        ];
        if self.legacy_open {
            ids.extend([
                SettingId::LegacyPhrases,
                SettingId::LegacyUrls,
                SettingId::LegacyRules,
                SettingId::LegacyAdjustments,
                SettingId::Obfuscate,
                SettingId::ObfuscationPatterns,
                SettingId::PageStandIn,
                SettingId::PageStandIns,
            ]);
        }
        ids
    }

    fn profile_field_error(&self, field: Option<&str>) -> Option<(String, String)> {
        let errors = self.profile.configuration_errors();
        if let Some(field) = field {
            errors.into_iter().find(|(id, _)| id == field)
        } else {
            errors.into_iter().next()
        }
    }

    fn activate_setting(&mut self, id: SettingId) {
        if let Some(position) = self.setting_ids().iter().position(|item| *item == id) {
            self.settings_selected = position;
        }
        use crate::provider_filter::{PageStandIn, PagesSetting};
        match id {
            SettingId::Enabled => self.profile.enabled = !self.profile.enabled,
            SettingId::Structural => self.profile.structural = !self.profile.structural,
            SettingId::Filenames => self.profile.hide.filenames = !self.profile.hide.filenames,
            SettingId::AllUrls => self.profile.hide.all_urls = !self.profile.hide.all_urls,
            SettingId::Images => self.profile.placeholder_images = !self.profile.placeholder_images,
            SettingId::Obfuscate => self.profile.obfuscate = !self.profile.obfuscate,
            SettingId::LegacyHeader => self.legacy_open = !self.legacy_open,
            SettingId::Pages => {
                self.profile.pages = match &self.profile.pages {
                    PagesSetting::Absent | PagesSetting::Off => PagesSetting::Latin,
                    PagesSetting::Latin => PagesSetting::Table {
                        stand_in: PageStandIn::Characters,
                        symbols: Vec::new(),
                        images: None,
                    },
                    PagesSetting::Table { stand_in, .. } => match stand_in {
                        PageStandIn::Characters => PagesSetting::Words(Vec::new()),
                        PageStandIn::Words | PageStandIn::Off | PageStandIn::Latin => {
                            PagesSetting::Table {
                                stand_in: PageStandIn::Phrases,
                                symbols: Vec::new(),
                                images: None,
                            }
                        }
                        PageStandIn::Phrases => PagesSetting::Off,
                    },
                    PagesSetting::Words(_) => PagesSetting::Table {
                        stand_in: PageStandIn::Phrases,
                        symbols: Vec::new(),
                        images: None,
                    },
                };
            }
            SettingId::PageStandIn => {
                self.profile.page_stand_in = match self.profile.page_stand_in {
                    PageStandIn::Off => PageStandIn::Latin,
                    PageStandIn::Latin => PageStandIn::Characters,
                    PageStandIn::Characters => PageStandIn::Words,
                    PageStandIn::Words => PageStandIn::Phrases,
                    PageStandIn::Phrases => PageStandIn::Off,
                };
            }
            SettingId::HideWords => self.begin_setting(lines(&self.profile.hide.words)),
            SettingId::HidePairs => self.begin_setting(pairs_text(&self.profile.hide.pairs)),
            SettingId::HideUrls => self.begin_setting(hide_urls_text(&self.profile.hide.urls)),
            SettingId::StopPhrases => self.begin_setting(stops_text(&self.profile.stop.phrases)),
            SettingId::StopUrls => self.begin_setting(stops_text(&self.profile.stop.urls)),
            SettingId::StopRules => self.begin_setting(filter_rules_text(&self.profile.stop.rules)),
            SettingId::TripFile => self.begin_setting(
                self.profile
                    .trip_file
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            ),
            SettingId::PageSymbols => self.begin_setting(page_symbols_text(&self.profile.pages)),
            SettingId::LegacyPhrases => self.begin_setting(lines(&self.profile.phrases)),
            SettingId::LegacyUrls => self.begin_setting(lines(&self.profile.urls)),
            SettingId::LegacyRules => self.begin_setting(filter_rules_text(&self.profile.rules)),
            SettingId::LegacyAdjustments => {
                self.begin_setting(pairs_text(&self.profile.adjustments))
            }
            SettingId::ObfuscationPatterns => {
                self.begin_setting(lines(&self.profile.obfuscation_patterns))
            }
            SettingId::PageStandIns => self.begin_setting(lines(&self.profile.page_stand_ins)),
        }
        self.error = self
            .profile_field_error(Some(setting_field(id)))
            .map(|(_, message)| message);
    }

    fn begin_setting(&mut self, text: String) {
        self.setting_cursor = text.len();
        self.setting_input = text;
        self.editing_setting = true;
    }

    fn confirm_setting(&mut self) {
        let Some(id) = self.setting_ids().get(self.settings_selected).copied() else {
            return;
        };
        let lines = non_empty_lines(&self.setting_input);
        match id {
            SettingId::HideWords => self.profile.hide.words = lines,
            SettingId::HidePairs => match parse_pairs(&self.setting_input) {
                Ok(pairs) => self.profile.hide.pairs = pairs,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            },
            SettingId::HideUrls => match parse_hide_urls(&self.setting_input) {
                Ok(urls) => self.profile.hide.urls = urls,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            },
            SettingId::StopPhrases => self.profile.stop.phrases = parse_stops(&self.setting_input),
            SettingId::StopUrls => self.profile.stop.urls = parse_stops(&self.setting_input),
            SettingId::StopRules => match parse_filter_rules(&self.setting_input) {
                Ok(rules) => self.profile.stop.rules = rules,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            },
            SettingId::TripFile => {
                self.profile.trip_file = if self.setting_input.trim().is_empty() {
                    None
                } else {
                    Some(std::path::PathBuf::from(self.setting_input.trim()))
                };
            }
            SettingId::PageSymbols => apply_page_symbols(&mut self.profile.pages, &lines),
            SettingId::LegacyPhrases => self.profile.phrases = lines,
            SettingId::LegacyUrls => self.profile.urls = lines,
            SettingId::LegacyRules => match parse_filter_rules(&self.setting_input) {
                Ok(rules) => self.profile.rules = rules,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            },
            SettingId::LegacyAdjustments => match parse_pairs(&self.setting_input) {
                Ok(pairs) => self.profile.adjustments = pairs,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            },
            SettingId::ObfuscationPatterns => self.profile.obfuscation_patterns = lines,
            SettingId::PageStandIns => self.profile.page_stand_ins = lines,
            _ => {}
        }
        self.editing_setting = false;
        self.error = self
            .profile_field_error(Some(setting_field(id)))
            .map(|(_, message)| message);
    }

    fn edit_files_key(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        match key {
            KeyCode::Enter => self.hit(Hit::FileRow(self.files_selected)),
            KeyCode::Tab | KeyCode::BackTab => self.editing_files = false,
            KeyCode::Up => self.files_selected = self.files_selected.saturating_sub(1),
            KeyCode::Down => {
                let count = self.file_rows().len();
                self.files_selected = self
                    .files_selected
                    .saturating_add(1)
                    .min(count.saturating_sub(1));
            }
            KeyCode::Left => {
                self.files_cursor = previous_grapheme(&self.files_query, self.files_cursor)
            }
            KeyCode::Right => {
                self.files_cursor = next_grapheme(&self.files_query, self.files_cursor)
            }
            KeyCode::Home => self.files_cursor = 0,
            KeyCode::End => self.files_cursor = self.files_query.len(),
            KeyCode::Backspace => {
                let previous = previous_grapheme(&self.files_query, self.files_cursor);
                self.files_query
                    .replace_range(previous..self.files_cursor, "");
                self.files_cursor = previous;
                self.files_selected = 0;
            }
            KeyCode::Delete => {
                let next = next_grapheme(&self.files_query, self.files_cursor);
                self.files_query.replace_range(self.files_cursor..next, "");
                self.files_selected = 0;
            }
            KeyCode::Char(character)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert_files(&character.to_string());
                self.files_selected = 0;
            }
            _ => {}
        }
    }

    fn edit_rules_key(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        match key {
            KeyCode::Enter => {
                self.editing_rules_query = false;
                let visible = self.visible_rules();
                if !visible.contains(&self.rules_selected) {
                    self.rules_selected = visible.first().copied().unwrap_or(0);
                }
            }
            KeyCode::Down | KeyCode::Up => self.move_rule_selection(key == KeyCode::Down),
            KeyCode::Left => {
                self.rules_cursor = previous_grapheme(&self.rules_query, self.rules_cursor)
            }
            KeyCode::Right => {
                self.rules_cursor = next_grapheme(&self.rules_query, self.rules_cursor)
            }
            KeyCode::Home => self.rules_cursor = 0,
            KeyCode::End => self.rules_cursor = self.rules_query.len(),
            KeyCode::Backspace => {
                let previous = previous_grapheme(&self.rules_query, self.rules_cursor);
                self.rules_query
                    .replace_range(previous..self.rules_cursor, "");
                self.rules_cursor = previous;
            }
            KeyCode::Delete => {
                let next = next_grapheme(&self.rules_query, self.rules_cursor);
                self.rules_query.replace_range(self.rules_cursor..next, "");
            }
            KeyCode::Char(character)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert_rules_query(&character.to_string());
            }
            _ => {}
        }
    }

    fn edit_setting_key(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        match key {
            KeyCode::Enter => self.confirm_setting(),
            KeyCode::Left => {
                self.setting_cursor = previous_grapheme(&self.setting_input, self.setting_cursor)
            }
            KeyCode::Right => {
                self.setting_cursor = next_grapheme(&self.setting_input, self.setting_cursor)
            }
            KeyCode::Home => self.setting_cursor = 0,
            KeyCode::End => self.setting_cursor = self.setting_input.len(),
            KeyCode::Backspace => {
                let previous = previous_grapheme(&self.setting_input, self.setting_cursor);
                self.setting_input
                    .replace_range(previous..self.setting_cursor, "");
                self.setting_cursor = previous;
            }
            KeyCode::Delete => {
                let next = next_grapheme(&self.setting_input, self.setting_cursor);
                self.setting_input
                    .replace_range(self.setting_cursor..next, "");
            }
            KeyCode::Char(character)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert_setting(&character.to_string());
            }
            _ => {}
        }
    }

    fn insert_limited(text: &mut String, cursor: &mut usize, extra: &str, limit: usize) {
        let extra: String = extra
            .chars()
            .filter(|character| !character.is_control() || *character == '\n')
            .collect();
        if text.len().saturating_add(extra.len()) > limit {
            return;
        }
        text.insert_str(*cursor, &extra);
        *cursor += extra.len();
    }

    fn insert_files(&mut self, text: &str) {
        Self::insert_limited(&mut self.files_query, &mut self.files_cursor, text, 512);
    }

    fn insert_rules_query(&mut self, text: &str) {
        Self::insert_limited(&mut self.rules_query, &mut self.rules_cursor, text, 512);
    }

    fn insert_setting(&mut self, text: &str) {
        Self::insert_limited(
            &mut self.setting_input,
            &mut self.setting_cursor,
            text,
            8192,
        );
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
        self.original_hits.borrow_mut().clear();
        self.scroll.set(0);
        self.window.set(0);
    }

    fn hit_at(&self, column: u16, row: u16) -> Option<Hit> {
        self.hits
            .borrow()
            .iter()
            .find(|(area, _)| area.contains(Position::new(column, row)))
            .map(|(_, hit)| *hit)
    }

    pub fn handle(&mut self, event: &Event) {
        if self.page == Page::Guide {
            self.handle_guide(event);
            return;
        }
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
        if let Some(selected) = self.scope_choice {
            match event {
                Event::Key(KeyCode::Esc, _) => self.scope_choice = None,
                Event::Key(KeyCode::Up, _) => self.scope_choice = Some(selected.saturating_sub(1)),
                Event::Key(KeyCode::Down, _) => {
                    self.scope_choice = Some((selected + 1).min(SCOPE_CHOICES.len() - 1))
                }
                Event::Key(KeyCode::Enter, _) => self.apply_scope(selected),
                Event::Key(KeyCode::Char(number @ '1'..='5'), _) => {
                    self.apply_scope((*number as u8 - b'1') as usize);
                }
                Event::MouseSelection(event) if event.phase == MouseSelectionPhase::Start => {
                    if let Some(hit @ Hit::ScopeChoice(_)) = self.hit_at(event.column, event.row) {
                        self.hit(hit);
                    }
                }
                _ => {}
            }
            return;
        }
        if self.rule_form.is_some() {
            match event {
                Event::Key(key, modifiers) => self.edit_rule_key(*key, *modifiers),
                Event::Paste(text) => {
                    let form = self.rule_form.as_mut().unwrap();
                    let mut cursor = form.cursor;
                    if let Some(field) = form.text_mut() {
                        Self::insert_limited(field, &mut cursor, text, 4096);
                        form.cursor = cursor;
                        self.error = None;
                    }
                }
                Event::MouseSelection(event) if event.phase == MouseSelectionPhase::Start => {
                    if let Some(
                        hit @ (Hit::RuleField(_) | Hit::RuleTreatment(_) | Hit::RuleCancel),
                    ) = self.hit_at(event.column, event.row)
                    {
                        self.hit(hit);
                    }
                }
                _ => {}
            }
            return;
        }
        if self.replace_project || self.replace_profile {
            match event {
                Event::Key(KeyCode::Enter, _) => {
                    let project = self.replace_project;
                    let profile = self.replace_profile;
                    self.replace_project = false;
                    self.replace_profile = false;
                    self.conflict_project = false;
                    self.conflict_profile = false;
                    if project {
                        self.save();
                    }
                    if profile {
                        self.save_profile();
                    }
                }
                Event::Key(KeyCode::Esc, _) => {
                    self.replace_project = false;
                    self.replace_profile = false;
                }
                _ => {}
            }
            return;
        }
        if matches!(event, Event::Key(KeyCode::Esc, _)) {
            if self.source_drag.take().is_some() {
                return;
            }
            if self.editing_range {
                self.editing_range = false;
            } else if self.editing_pattern {
                self.editing_pattern = false;
            } else if self.editing_path {
                self.editing_path = false;
                self.editing_files = self.page == Page::Files;
            } else if self.editing_files {
                self.editing_files = false;
            } else if self.editing_rules_query {
                self.editing_rules_query = false;
            } else if self.editing_setting {
                self.editing_setting = false;
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
            if self.select_source_text(event) {
                return;
            }
            if event.phase == MouseSelectionPhase::Start {
                if let Some(hit) = self.hit_at(event.column, event.row) {
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
            } else if self.editing_files {
                self.insert_files(text);
            } else if self.editing_rules_query {
                self.insert_rules_query(text);
            } else if self.editing_setting {
                self.insert_setting(text);
            }
            return;
        }
        let Event::Key(key, modifiers) = event else {
            return;
        };
        let control = modifiers.contains(KeyModifiers::CONTROL);
        if control && *key == KeyCode::Char('p') && self.page == Page::Files {
            self.hit(Hit::Path);
            return;
        }
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
            if self.page == Page::Settings {
                self.save_profile();
            } else {
                self.save();
            }
            return;
        }
        if self.editing_files {
            self.edit_files_key(*key, *modifiers);
            return;
        }
        if self.editing_rules_query {
            self.edit_rules_key(*key, *modifiers);
            return;
        }
        if self.editing_setting {
            self.edit_setting_key(*key, *modifiers);
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
            KeyCode::Char('0') => self.show_page(Page::Overview),
            KeyCode::Char('1') => self.hit(Hit::Page(Page::Sources)),
            KeyCode::Char('2') => self.hit(Hit::Page(Page::Preview)),
            KeyCode::Char('3') => self.hit(Hit::Page(Page::Rules)),
            KeyCode::Char('4') => self.hit(Hit::Page(Page::Settings)),
            KeyCode::Char('5') => self.open_files(),
            KeyCode::Char('p') => self.hit(Hit::Path),
            KeyCode::Char('f') if self.page == Page::Sources => self.hit(Hit::Pattern),
            KeyCode::Char('f') if self.page == Page::Preview => {
                self.hit(Hit::Mode(PreviewMode::FileDraft))
            }
            KeyCode::Char('g') if self.page == Page::Sources => self.hit(Hit::Group),
            KeyCode::Char('v') => self.hit(Hit::Range),
            KeyCode::Char('b') if self.page == Page::Sources => self.open_files(),
            KeyCode::Char('e') => self.toggle_enabled(),
            KeyCode::Char(' ')
                if matches!(self.page, Page::Sources | Page::Rules | Page::Overview) =>
            {
                self.toggle_enabled()
            }
            KeyCode::Char('a') if self.can_apply_rule() => self.set_rule(RuleAction::Allow),
            KeyCode::Char('m') if self.can_apply_rule() => self.set_rule(RuleAction::Alias),
            KeyCode::Char('h') if self.can_apply_rule() => self.set_rule(RuleAction::Hide),
            KeyCode::Char('u') if self.page == Page::Sources => self.hit(Hit::Scope),
            KeyCode::Char('o' | 'O') if self.page == Page::Overview => {
                self.actions.push_back(Action::DisablePreview);
            }
            KeyCode::Char('o') if self.page == Page::Rules => self.open_rule_source(),
            KeyCode::Char('o') => self.hit(Hit::Mode(PreviewMode::Original)),
            KeyCode::Char('t') => self.hit(Hit::Mode(PreviewMode::Sent)),
            KeyCode::Char('d') if self.page == Page::Rules => self.cycle_default_action(),
            KeyCode::Char('d') => self.hit(Hit::Mode(PreviewMode::Changes)),
            KeyCode::Char('r') => self.hit(Hit::Mode(PreviewMode::Request)),
            KeyCode::Tab | KeyCode::BackTab => {
                let next = match self.page {
                    Page::Guide => Page::Overview,
                    Page::Overview => Page::Files,
                    Page::Files => Page::Sources,
                    Page::Sources => Page::Rules,
                    Page::Rules => Page::Preview,
                    Page::Preview => Page::Settings,
                    Page::Settings => Page::Overview,
                };
                self.show_page(next);
            }
            KeyCode::Up if self.page == Page::Overview => {
                self.overview_selected = self.overview_selected.saturating_sub(1);
            }
            KeyCode::Down if self.page == Page::Overview => {
                self.overview_selected = self.overview_selected.saturating_add(1).min(3);
            }
            KeyCode::Enter if self.page == Page::Overview => {
                self.show_page(self.overview_page());
            }
            KeyCode::Up if self.page == Page::Files => {
                self.files_selected = self.files_selected.saturating_sub(1);
            }
            KeyCode::Down if self.page == Page::Files => {
                let count = self.file_rows().len();
                self.files_selected = self
                    .files_selected
                    .saturating_add(1)
                    .min(count.saturating_sub(1));
            }
            KeyCode::Enter if self.page == Page::Files => {
                self.hit(Hit::FileRow(self.files_selected))
            }
            KeyCode::Up if self.page == Page::Rules => self.move_rule_selection(false),
            KeyCode::Down if self.page == Page::Rules => self.move_rule_selection(true),
            KeyCode::Enter if self.page == Page::Rules => {
                if self.visible_rules().contains(&self.rules_selected) {
                    self.activate_rule(self.rules_selected);
                }
            }
            KeyCode::Char(',') if self.page == Page::Rules => self.move_rule(false),
            KeyCode::Char('.') if self.page == Page::Rules => self.move_rule(true),
            KeyCode::Char('n') if self.page == Page::Rules => self.add_rule(),
            KeyCode::Delete if self.page == Page::Rules => self.remove_rule(),
            KeyCode::Char('/') if self.page == Page::Rules => self.hit(Hit::RuleSearch),
            KeyCode::Up if self.page == Page::Settings => {
                self.settings_selected = self.settings_selected.saturating_sub(1);
            }
            KeyCode::Down if self.page == Page::Settings => {
                let count = self.setting_ids().len();
                self.settings_selected = self
                    .settings_selected
                    .saturating_add(1)
                    .min(count.saturating_sub(1));
            }
            KeyCode::Enter if self.page == Page::Settings => {
                if let Some(id) = self.setting_ids().get(self.settings_selected).copied() {
                    self.activate_setting(id);
                }
            }
            KeyCode::Left if self.page == Page::Preview => {
                self.link_selected = self.link_selected.saturating_sub(1);
            }
            KeyCode::Right if self.page == Page::Preview => {
                self.link_selected = self.link_selected.saturating_add(1);
            }
            KeyCode::Enter if self.page == Page::Preview && self.request_target => {
                self.open_request_source(self.link_selected);
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

fn path_matches(path: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let path = path.to_lowercase();
    let name = path.rsplit('/').next().unwrap_or(path.as_str());
    path.contains(query) || name.contains(query)
}

fn concrete_rule_file(file: &str) -> Option<&str> {
    if file.is_empty()
        || file
            .chars()
            .any(|character| matches!(character, '*' | '?' | '[' | '{' | '\\'))
    {
        None
    } else {
        Some(file)
    }
}

fn rule_scope_label(rule: &Rule) -> &'static str {
    if rule.selector.starts_with("/@digest/") {
        "This file version"
    } else if concrete_rule_file(&rule.file).is_none() {
        "Files that match a pattern"
    } else if rule.selector.ends_with("/**")
        && !rule.selector.contains("/markdown/")
        && !rule.selector.contains("/csv/")
    {
        "This whole file"
    } else if rule.selector.contains("/**") || rule.selector.contains("/*/") {
        "The group that contains it"
    } else {
        "This value when it appears again"
    }
}

fn non_empty_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn lines(values: &[String]) -> String {
    values.join("\n")
}

fn pairs_text(pairs: &[crate::provider_filter::ContentAdjustment]) -> String {
    pairs
        .iter()
        .map(|pair| format!("{} -> {}", pair.from, pair.to))
        .collect::<Vec<_>>()
        .join("\n")
}

fn hide_urls_text(urls: &[crate::provider_filter::HideUrl]) -> String {
    urls.iter()
        .map(|url| {
            format!(
                "{} -> {}{}",
                url.from,
                url.to,
                if url.prefix { " prefix" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn stops_text(values: &[crate::provider_filter::StopText]) -> String {
    values
        .iter()
        .map(|value| match value {
            crate::provider_filter::StopText::Plain(text) => text.clone(),
            crate::provider_filter::StopText::Detailed(detail) if detail.mask => {
                format!("mask {}", detail.text)
            }
            crate::provider_filter::StopText::Detailed(detail) => detail.text.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn filter_rules_text(rules: &[crate::provider_filter::ProviderFilterRule]) -> String {
    rules
        .iter()
        .map(|rule| {
            format!(
                "{} {} {} {}",
                rule.id,
                match rule.kind {
                    crate::provider_filter::ProviderFilterKind::Literal => "literal",
                    crate::provider_filter::ProviderFilterKind::Regex => "regex",
                },
                match rule.target {
                    crate::provider_filter::ProviderFilterTarget::Request => "request",
                    crate::provider_filter::ProviderFilterTarget::Response => "response",
                    crate::provider_filter::ProviderFilterTarget::Both => "both",
                },
                rule.pattern
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_pairs(text: &str) -> Result<Vec<crate::provider_filter::ContentAdjustment>, String> {
    non_empty_lines(text)
        .into_iter()
        .map(|line| {
            let (from, to) = line
                .split_once(" -> ")
                .ok_or_else(|| "Write each replacement as from -> to.".to_string())?;
            Ok(crate::provider_filter::ContentAdjustment {
                from: from.trim().to_string(),
                to: to.trim().to_string(),
            })
        })
        .collect()
}

fn parse_hide_urls(text: &str) -> Result<Vec<crate::provider_filter::HideUrl>, String> {
    non_empty_lines(text)
        .into_iter()
        .map(|line| {
            let prefix = line.ends_with(" prefix");
            let line = line.trim_end_matches(" prefix");
            let (from, to) = line
                .split_once(" -> ")
                .ok_or_else(|| "Write each URL replacement as from -> to.".to_string())?;
            Ok(crate::provider_filter::HideUrl {
                from: from.trim().to_string(),
                to: to.trim().to_string(),
                prefix,
            })
        })
        .collect()
}

fn parse_stops(text: &str) -> Vec<crate::provider_filter::StopText> {
    non_empty_lines(text)
        .into_iter()
        .map(|line| {
            if let Some(text) = line.strip_prefix("mask ") {
                crate::provider_filter::StopText::Detailed(crate::provider_filter::StopTextDetail {
                    text: text.to_string(),
                    mask: true,
                })
            } else {
                crate::provider_filter::StopText::Plain(line)
            }
        })
        .collect()
}

fn parse_filter_rules(
    text: &str,
) -> Result<Vec<crate::provider_filter::ProviderFilterRule>, String> {
    non_empty_lines(text)
        .into_iter()
        .map(|line| {
            let mut parts = line.splitn(4, ' ');
            let id = parts.next().ok_or_else(|| {
                "Write id, literal or regex, request response or both, then the pattern."
                    .to_string()
            })?;
            let kind = match parts.next().unwrap_or("literal") {
                "regex" => crate::provider_filter::ProviderFilterKind::Regex,
                _ => crate::provider_filter::ProviderFilterKind::Literal,
            };
            let target = match parts.next().unwrap_or("both") {
                "request" => crate::provider_filter::ProviderFilterTarget::Request,
                "response" => crate::provider_filter::ProviderFilterTarget::Response,
                _ => crate::provider_filter::ProviderFilterTarget::Both,
            };
            let pattern = parts.next().unwrap_or("").to_string();
            if id.is_empty() || pattern.is_empty() {
                return Err(
                    "Write id, literal or regex, request response or both, then the pattern."
                        .into(),
                );
            }
            Ok(crate::provider_filter::ProviderFilterRule {
                id: id.to_string(),
                target,
                kind,
                pattern,
            })
        })
        .collect()
}

fn page_symbols_text(pages: &crate::provider_filter::PagesSetting) -> String {
    match pages {
        crate::provider_filter::PagesSetting::Words(words) => lines(words),
        crate::provider_filter::PagesSetting::Table { symbols, .. } => lines(symbols),
        _ => String::new(),
    }
}

fn apply_page_symbols(pages: &mut crate::provider_filter::PagesSetting, symbols: &[String]) {
    use crate::provider_filter::{PageStandIn, PagesSetting};
    match pages {
        PagesSetting::Table {
            stand_in,
            symbols: current,
            ..
        } => {
            *current = symbols.to_vec();
            let _ = stand_in;
        }
        PagesSetting::Words(_) => *pages = PagesSetting::Words(symbols.to_vec()),
        PagesSetting::Latin | PagesSetting::Off | PagesSetting::Absent if !symbols.is_empty() => {
            *pages = PagesSetting::Words(symbols.to_vec());
        }
        _ => {}
    }
    let _ = PageStandIn::Off;
}

fn setting_field(id: SettingId) -> &'static str {
    match id {
        SettingId::TripFile => "trip_file",
        SettingId::HideWords | SettingId::HidePairs | SettingId::HideUrls => "hide.words",
        SettingId::StopPhrases => "stop.phrases",
        SettingId::StopUrls => "stop.urls",
        SettingId::StopRules => "stop.rules",
        SettingId::Pages | SettingId::PageSymbols => "pages",
        SettingId::LegacyPhrases => "phrases",
        SettingId::LegacyUrls => "urls",
        SettingId::LegacyRules => "rules",
        SettingId::LegacyAdjustments => "adjustments",
        SettingId::ObfuscationPatterns => "obfuscation_patterns",
        SettingId::PageStandIn => "page_stand_in",
        SettingId::PageStandIns => "page_stand_ins",
        SettingId::Images => "placeholder_images",
        _ => "form",
    }
}

fn setting_label(id: SettingId, profile: &crate::provider_filter::ProviderFilterConfig) -> String {
    use crate::provider_filter::{PageStandIn, PagesSetting};
    let mark = |on: bool| if on { "[x]" } else { "[ ]" };
    match id {
        SettingId::Enabled => format!("{} Stop filter on next launch", mark(profile.enabled)),
        SettingId::Structural => format!(
            "{} Selected-value mode at startup",
            mark(profile.structural)
        ),
        SettingId::Filenames => format!("{} Mask filenames", mark(profile.hide.filenames)),
        SettingId::AllUrls => format!("{} Mask every URL", mark(profile.hide.all_urls)),
        SettingId::Images => format!("{} Image placeholders", mark(profile.placeholder_images)),
        SettingId::HideWords => format!("Hide words ({})", profile.hide.words.len()),
        SettingId::HidePairs => format!("Text replacements ({})", profile.hide.pairs.len()),
        SettingId::HideUrls => format!("URL replacements ({})", profile.hide.urls.len()),
        SettingId::StopPhrases => format!("Phrase stops ({})", profile.stop.phrases.len()),
        SettingId::StopUrls => format!("URL stops ({})", profile.stop.urls.len()),
        SettingId::StopRules => format!("Literal and regex stops ({})", profile.stop.rules.len()),
        SettingId::TripFile => format!(
            "Emergency trip file: {}",
            profile
                .trip_file
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".into())
        ),
        SettingId::Pages => format!(
            "Page substitution: {}",
            match &profile.pages {
                PagesSetting::Absent | PagesSetting::Off => "off",
                PagesSetting::Latin => "latin",
                PagesSetting::Words(_) => "words",
                PagesSetting::Table { stand_in, .. } => match stand_in {
                    PageStandIn::Characters => "characters",
                    PageStandIn::Words => "words",
                    PageStandIn::Phrases => "phrases",
                    PageStandIn::Latin => "latin",
                    PageStandIn::Off => "off",
                },
            }
        ),
        SettingId::PageSymbols => "Custom page symbols".into(),
        SettingId::LegacyHeader => "Legacy settings".into(),
        SettingId::LegacyPhrases => format!("Legacy phrases ({})", profile.phrases.len()),
        SettingId::LegacyUrls => format!("Legacy URLs ({})", profile.urls.len()),
        SettingId::LegacyRules => format!("Legacy rules ({})", profile.rules.len()),
        SettingId::LegacyAdjustments => {
            format!("Legacy replacements ({})", profile.adjustments.len())
        }
        SettingId::Obfuscate => format!("{} Legacy automatic masking", mark(profile.obfuscate)),
        SettingId::ObfuscationPatterns => {
            format!("Legacy hide words ({})", profile.obfuscation_patterns.len())
        }
        SettingId::PageStandIn => format!("Legacy page mode: {:?}", profile.page_stand_in),
        SettingId::PageStandIns => {
            format!("Legacy page symbols ({})", profile.page_stand_ins.len())
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

fn treatment_label(action: RuleAction) -> &'static str {
    match action {
        RuleAction::Allow => "Readable",
        RuleAction::Alias => "Aliased",
        RuleAction::Hide => "Hidden",
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

#[cfg(test)]
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
    selectable_original: bool,
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
    if selectable_original {
        let layout = source_selection::OriginalLayout::new(&text[start..end], start, inner.width);
        let scroll = panel_scroll(editor, inner, layout.row_count());
        let selection = editor
            .source_drag
            .as_ref()
            .filter(|drag| drag.moved)
            .map(source_selection::Drag::bytes)
            .or_else(|| {
                editor
                    .selected_range
                    .as_ref()
                    .map(|range| range.bytes.clone())
            });
        let (lines, hits) = layout.visible(inner, scroll, selection.as_ref());
        *editor.original_hits.borrow_mut() = hits;
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().fg(theme.text)),
            inner,
        );
    } else {
        let visible = crate::terminal_text::sanitize_terminal_text(&text[start..end]);
        let paragraph = Paragraph::new(visible)
            .style(Style::default().fg(theme.text))
            .wrap(Wrap { trim: false });
        let scroll = panel_scroll(editor, inner, paragraph.line_count(inner.width.max(1)));
        frame.render_widget(paragraph.scroll((scroll, 0)), inner);
    }
}

fn panel_scroll(editor: &Editor, area: Rect, rows: usize) -> u16 {
    let max = rows.saturating_sub(usize::from(area.height));
    editor.max_scroll.set(
        editor
            .max_scroll
            .get()
            .max(u16::try_from(max).unwrap_or(u16::MAX)),
    );
    editor.visible_rows.set(area.height.max(1));
    let scroll = editor.scroll.get().min(editor.max_scroll.get());
    editor.scroll.set(scroll);
    scroll
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
    editor.original_hits.borrow_mut().clear();
    editor.max_scroll.set(0);
    editor.max_window.set(0);
    frame.render_widget(Clear, area);
    let title = if UnicodeWidthStr::width(version) + 14 <= usize::from(area.width) {
        format!(" {version} · Filter ")
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
                "Filter\nEnlarge the terminal to edit.\nEsc keeps the draft. /filter off skips the review.",
            )
            .wrap(Wrap { trim: false }),
            inner,
        );
        return;
    }
    if editor.page == Page::Guide {
        guide::render(frame, inner, editor, theme);
        return;
    }
    let show_field = editor.editing_range
        || editor.editing_pattern
        || editor.editing_path
        || matches!(editor.page, Page::Files | Page::Sources | Page::Rules)
            && editor.rule_form.is_none();
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(u16::from(show_field)),
            Constraint::Min(1),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .split(inner);
    let toolbar = Layout::horizontal([Constraint::Min(1), Constraint::Length(7)]).split(areas[0]);
    let gate = if editor.review_gate.get() {
        "on"
    } else {
        "off"
    };
    let mode = editor.snapshot.as_ref().map_or("loading", |snapshot| {
        if snapshot.filename_only {
            "Filename masking"
        } else if !snapshot.effective.enabled {
            "Project filtering off"
        } else {
            match snapshot.effective.default_action {
                RuleAction::Allow => "Selected values only",
                RuleAction::Alias => "Alias by default",
                RuleAction::Hide => "Hide unless allowed",
            }
        }
    });
    let active = format!("Review before send: {gate} · {mode}");
    frame.render_widget(
        Paragraph::new(active).style(Style::default().fg(theme.text)),
        toolbar[0],
    );
    button(
        frame,
        editor,
        toolbar[1],
        " Close ",
        Hit::Close,
        false,
        theme,
    );
    let tabs = Layout::horizontal([
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(10),
        Constraint::Min(1),
    ])
    .split(areas[1]);
    for (index, (label, page)) in [
        ("Files", Page::Files),
        ("Rules", Page::Rules),
        ("Preview", Page::Preview),
        ("Settings", Page::Settings),
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
            editor.page == page || (page == Page::Files && editor.page == Page::Sources),
            theme,
        );
    }
    if show_field {
        render_navigation_field(frame, areas[2], editor, theme);
    }
    if editor.loading {
        frame.render_widget(
            Paragraph::new("Loading the local privacy policy…").wrap(Wrap { trim: false }),
            areas[3],
        );
    } else {
        match editor.page {
            Page::Guide => unreachable!("task guide renders above"),
            Page::Overview => render_overview(frame, areas[3], editor, theme),
            Page::Files => render_files(frame, areas[3], editor, theme),
            Page::Sources => render_sources(frame, areas[3], editor, theme),
            Page::Rules => render_rules(frame, areas[3], editor, theme),
            Page::Preview => render_preview(frame, areas[3], editor, theme, request),
            Page::Settings => render_settings(frame, areas[3], editor, theme),
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
        match editor.page {
            Page::Guide => unreachable!("task guide renders above"),
            Page::Files if editor.editing_path => {
                "Enter opens the path · Esc returns to search".into()
            }
            Page::Files => {
                "↑↓ Choose file · Enter Open · Ctrl-P Enter path · Tab Leave search".into()
            }
            Page::Sources if editor.scope_choice.is_some() => {
                "↑↓ Choose scope · Enter Use · Esc Cancel".into()
            }
            Page::Sources if !editor.original_hits.borrow().is_empty() => {
                "Drag text in Original · A Allow · M Alias · H Hide · Ctrl-S Save".into()
            }
            Page::Sources => "O Original to drag text · ↑↓ Choose a value · B Back to files".into(),
            Page::Rules if editor.rule_form.is_some() => {
                "Tab Move · ←→ Choose action · Enter Confirm field · Esc Cancel".into()
            }
            Page::Rules => "Enter Edit · O Open source · / Search · Ctrl-S Save".into(),
            Page::Preview if editor.file_original() => {
                "Drag text · A Allow · M Alias · H Hide · Ctrl-S Save · 1 File review".into()
            }
            Page::Preview => "R Switch request/file · ↑↓ Scroll · 0 Overview".into(),
            Page::Settings => "↑↓ Choose setting · Enter Edit · Ctrl-S Save for next launch".into(),
            Page::Overview => format!("{} · ↑↓ Choose a section · Enter Open", editor.workspace),
        }
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
    let saves = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(areas[5]);
    button(
        frame,
        editor,
        saves[0],
        "Save project rules",
        Hit::Save,
        editor.dirty_project(),
        theme,
    );
    button(
        frame,
        editor,
        saves[1],
        "Save for next launch",
        Hit::SaveProfile,
        editor.dirty_profile(),
        theme,
    );
}

fn render_field(
    frame: &mut ratatui::Frame,
    editor: &Editor,
    area: Rect,
    field: (&str, &str, Option<usize>),
    hit: Hit,
    theme: super::render::ThemePalette,
) {
    let (label, text, cursor) = field;
    let text = crate::terminal_text::sanitize_terminal_text(text);
    let label_width = UnicodeWidthStr::width(label);
    let column = cursor.map_or(0, |cursor| {
        UnicodeWidthStr::width(&text[..text.floor_char_boundary(cursor.min(text.len()))])
    });
    let available = usize::from(area.width).saturating_sub(label_width);
    let scroll = column.saturating_sub(available.saturating_sub(1));
    let row = Layout::horizontal([Constraint::Length(label_width as u16), Constraint::Min(1)])
        .split(area);
    frame.render_widget(
        Paragraph::new(label).style(Style::default().fg(theme.muted)),
        row[0],
    );
    frame.render_widget(
        Paragraph::new(text)
            .scroll((0, scroll as u16))
            .style(Style::default().fg(theme.text)),
        row[1],
    );
    editor.hits.borrow_mut().push((area, hit));
    if cursor.is_some() && row[1].width > 0 {
        frame.set_cursor_position((row[1].x + (column - scroll) as u16, area.y));
    }
}

fn render_navigation_field(
    frame: &mut ratatui::Frame,
    area: Rect,
    editor: &Editor,
    theme: super::render::ThemePalette,
) {
    if editor.page == Page::Rules && !editor.editing_path {
        render_field(
            frame,
            editor,
            area,
            (
                "/ Find rules: ",
                &editor.rules_query,
                editor.editing_rules_query.then_some(editor.rules_cursor),
            ),
            Hit::RuleSearch,
            theme,
        );
        return;
    }
    let row = Layout::horizontal([
        Constraint::Min(1),
        Constraint::Length(7),
        Constraint::Length(7),
    ])
    .split(area);
    let (label, value, cursor, hit, submit) = if editor.editing_range {
        (
            "Range: ",
            &editor.range_input,
            Some(editor.range_cursor),
            Hit::Range,
            " Use",
        )
    } else if editor.editing_pattern {
        (
            "File pattern: ",
            &editor.file_pattern,
            Some(editor.pattern_cursor),
            Hit::Pattern,
            " Use",
        )
    } else if editor.page == Page::Files && !editor.editing_path {
        (
            "Search: ",
            &editor.files_query,
            editor.editing_files.then_some(editor.files_cursor),
            Hit::FileSearch,
            " Open",
        )
    } else {
        (
            "File: ",
            &editor.path,
            editor.editing_path.then_some(editor.path_cursor),
            Hit::Path,
            " Open",
        )
    };
    render_field(frame, editor, row[0], (label, value, cursor), hit, theme);
    button(frame, editor, row[1], submit, Hit::Load, false, theme);
    button(
        frame,
        editor,
        row[2],
        " Path",
        Hit::Path,
        editor.editing_path,
        theme,
    );
}

fn render_overview(
    frame: &mut ratatui::Frame,
    area: Rect,
    editor: &Editor,
    theme: super::render::ThemePalette,
) {
    let saved = editor.snapshot.as_ref().map(|snapshot| {
        format!(
            "Next launch: {} rules, filter {}",
            snapshot.policy.rules.len(),
            if snapshot.profile.enabled {
                "on"
            } else {
                "off"
            }
        )
    });
    let unsaved = if editor.dirty_project() && editor.dirty_profile() {
        "Unsaved changes: project and profile"
    } else if editor.dirty_project() {
        "Unsaved changes: project rules"
    } else if editor.dirty_profile() {
        "Unsaved changes: profile settings"
    } else {
        "Unsaved changes: none"
    };
    let active = editor
        .snapshot
        .as_ref()
        .map_or("Active now: loading the filter status.", |snapshot| {
            snapshot.active_now.as_str()
        });
    let mut lines = vec![
        active.to_string(),
        saved.unwrap_or_else(|| "Next launch: loading".into()),
        unsaved.to_string(),
    ];
    if let Some(note) = editor
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.override_note.as_deref())
    {
        lines.push(note.to_string());
    }
    lines.push(String::new());
    lines.push(
        "New tasks prepare their filters automatically. Review the provider copy before sending."
            .into(),
    );
    lines.push("O or /filter off skips the review for this ca.".into());
    let rows = [
        "Files — search this project",
        "Rules — saved and draft rules, last match wins",
        "Preview — latest provider request",
        "Settings — advanced profile for the next launch",
    ];
    for (index, label) in rows.iter().enumerate() {
        let y = area.y.saturating_add(index as u16);
        if y >= area.y.saturating_add(area.height) {
            break;
        }
        button(
            frame,
            editor,
            Rect::new(area.x, y, area.width, 1),
            &format!(
                "{} {label}",
                if index == editor.overview_selected {
                    ">"
                } else {
                    " "
                }
            ),
            Hit::Overview(match index {
                0 => Page::Files,
                1 => Page::Rules,
                2 => Page::Preview,
                _ => Page::Settings,
            }),
            index == editor.overview_selected,
            theme,
        );
    }
    let facts = Rect {
        x: area.x,
        y: area.y.saturating_add(rows.len() as u16),
        width: area.width,
        height: area.height.saturating_sub(rows.len() as u16),
    };
    if facts.height > 0 {
        frame.render_widget(
            Paragraph::new(lines.join("\n")).wrap(Wrap { trim: false }),
            facts,
        );
    }
}

fn render_files(
    frame: &mut ratatui::Frame,
    area: Rect,
    editor: &Editor,
    theme: super::render::ThemePalette,
) {
    let rows = editor.file_rows();
    if rows.is_empty() {
        let message = if let Some(note) = &editor.index_note {
            note.as_str()
        } else if editor.indexed_files.is_empty() {
            "No files listed yet. Search by name or use Path to open a file directly."
        } else {
            "No files match. Change the search or use Path to open a file directly."
        };
        frame.render_widget(Paragraph::new(message).wrap(Wrap { trim: false }), area);
        return;
    }
    let visible = usize::from(area.height);
    let start = editor
        .files_selected
        .saturating_add(1)
        .saturating_sub(visible);
    for (offset, path) in rows.iter().enumerate().skip(start).take(visible) {
        let row = Rect::new(area.x, area.y + (offset - start) as u16, area.width, 1);
        button(
            frame,
            editor,
            row,
            &format!(
                "{} {path}",
                if offset == editor.files_selected {
                    ">"
                } else {
                    " "
                }
            ),
            Hit::FileRow(offset),
            offset == editor.files_selected,
            theme,
        );
    }
}

fn render_rules(
    frame: &mut ratatui::Frame,
    area: Rect,
    editor: &Editor,
    theme: super::render::ThemePalette,
) {
    if let Some(form) = &editor.rule_form {
        render_rule_form(frame, area, editor, form, theme);
        return;
    }
    let header = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(1),
    ])
    .split(area);
    let actions = Layout::horizontal([
        Constraint::Length(7),
        Constraint::Length(11),
        Constraint::Length(10),
        Constraint::Length(9),
        Constraint::Min(0),
    ])
    .split(header[0]);
    for (index, (label, hit)) in [
        ("N Add", Hit::RuleAdd),
        ("Del Remove", Hit::RuleRemove),
        (", Earlier", Hit::RuleEarlier),
        (". Later", Hit::RuleLater),
    ]
    .into_iter()
    .enumerate()
    {
        button(frame, editor, actions[index], label, hit, false, theme);
    }
    if actions[4].width >= 13 {
        button(
            frame,
            editor,
            actions[4],
            "O Open source",
            Hit::RuleOpenSource,
            false,
            theme,
        );
    }
    let enabled = editor.policy.as_ref().is_some_and(|policy| policy.enabled);
    button(
        frame,
        editor,
        header[1],
        if enabled {
            "E  Project policy draft: on"
        } else {
            "E  Project policy draft: off"
        },
        Hit::ProjectEnabled,
        enabled,
        theme,
    );
    let default = editor
        .policy
        .as_ref()
        .map(|policy| action_name(policy.default_action))
        .unwrap_or("Hide");
    button(
        frame,
        editor,
        header[2],
        &format!("D  Project default: {default}"),
        Hit::DefaultAction,
        false,
        theme,
    );
    let note = editor
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.override_note.clone())
        .unwrap_or_else(|| "Last matching rule wins.".into());
    frame.render_widget(
        Paragraph::new(note)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(theme.muted)),
        header[3],
    );
    let visible = editor.visible_rules();
    if visible.is_empty() {
        frame.render_widget(
            Paragraph::new(if editor.rules_query.is_empty() {
                "No project rules yet. Add a rule or choose a value in Files."
            } else {
                "No rules match this search."
            })
            .wrap(Wrap { trim: false }),
            header[4],
        );
        return;
    }
    let rows = usize::from(header[4].height);
    let position = visible
        .iter()
        .position(|index| *index == editor.rules_selected)
        .unwrap_or(0);
    let start = position.saturating_add(1).saturating_sub(rows);
    let policy = editor.policy.as_ref();
    for (offset, index) in visible.iter().copied().enumerate().skip(start).take(rows) {
        let Some(rule) = policy.and_then(|policy| policy.rules.get(index)) else {
            continue;
        };
        let row = Rect::new(
            header[4].x,
            header[4].y + (offset - start) as u16,
            header[4].width,
            1,
        );
        button(
            frame,
            editor,
            row,
            &format!(
                "{} {} · {} · {}",
                if index == editor.rules_selected {
                    ">"
                } else {
                    " "
                },
                action_name(rule.action),
                rule.file,
                rule_scope_label(rule)
            ),
            Hit::RuleRow(index),
            index == editor.rules_selected,
            theme,
        );
    }
}

fn render_rule_form(
    frame: &mut ratatui::Frame,
    area: Rect,
    editor: &Editor,
    form: &RuleForm,
    theme: super::render::ThemePalette,
) {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(u16::from(form.advanced)),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(if form.index.is_some() {
            "Edit project rule"
        } else {
            "Add project rule"
        })
        .style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new("Use a file path or a pattern like docs/*.md.")
            .style(Style::default().fg(theme.muted)),
        rows[1],
    );
    render_field(
        frame,
        editor,
        rows[2],
        (
            "Match: ",
            &form.draft.file,
            (form.field == RuleField::File).then_some(form.cursor),
        ),
        Hit::RuleField(RuleField::File),
        theme,
    );
    let treatments = Layout::horizontal([
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(9),
        Constraint::Min(8),
    ])
    .split(rows[3]);
    frame.render_widget(
        Paragraph::new("Action").style(Style::default().fg(theme.muted)),
        treatments[0],
    );
    for (index, action) in [RuleAction::Hide, RuleAction::Alias, RuleAction::Allow]
        .into_iter()
        .enumerate()
    {
        let selected = action == form.draft.action;
        let label = format!(
            "{}{}",
            if selected { "● " } else { "○ " },
            action_name(action)
        );
        button(
            frame,
            editor,
            treatments[index + 1],
            &label,
            Hit::RuleTreatment(action),
            selected || form.field == RuleField::Action,
            theme,
        );
    }
    let scope = if form.draft.selector == "/**" {
        "Whole file"
    } else {
        "Selected content"
    };
    button(
        frame,
        editor,
        rows[4],
        &format!("{scope} · Advanced selector"),
        Hit::RuleField(RuleField::Advanced),
        form.field == RuleField::Advanced,
        theme,
    );
    if form.advanced {
        render_field(
            frame,
            editor,
            rows[5],
            (
                "Selector: ",
                &form.draft.selector,
                (form.field == RuleField::Selector).then_some(form.cursor),
            ),
            Hit::RuleField(RuleField::Selector),
            theme,
        );
    }
    let actions = Layout::horizontal([Constraint::Length(18), Constraint::Min(1)]).split(rows[6]);
    button(
        frame,
        editor,
        actions[0],
        "Apply to draft",
        Hit::RuleField(RuleField::Apply),
        form.field == RuleField::Apply,
        theme,
    );
    button(
        frame,
        editor,
        actions[1],
        "Esc Cancel",
        Hit::RuleCancel,
        false,
        theme,
    );
}

fn render_settings(
    frame: &mut ratatui::Frame,
    area: Rect,
    editor: &Editor,
    theme: super::render::ThemePalette,
) {
    let ids = editor.setting_ids();
    let errors = editor.profile.configuration_errors();
    let rows = usize::from(area.height);
    let start = editor
        .settings_selected
        .saturating_add(1)
        .saturating_sub(rows);
    for (offset, id) in ids.iter().copied().enumerate().skip(start).take(rows) {
        let mut label = setting_label(id, &editor.profile);
        let field = setting_field(id);
        if let Some((_, message)) = errors.iter().find(|(name, _)| name == field) {
            label = format!("{label} · {message}");
        }
        let row = Rect::new(area.x, area.y + (offset - start) as u16, area.width, 1);
        button(
            frame,
            editor,
            row,
            &format!(
                "{} {label}",
                if offset == editor.settings_selected {
                    ">"
                } else {
                    " "
                }
            ),
            Hit::Setting(id),
            offset == editor.settings_selected,
            theme,
        );
    }
    if editor.editing_setting {
        frame.render_widget(
            Paragraph::new(format!("Edit: {}", editor.setting_input)),
            Rect {
                y: area.y.saturating_add(area.height.saturating_sub(1)),
                height: 1,
                ..area
            },
        );
    }
}

fn render_sources(
    frame: &mut ratatui::Frame,
    area: Rect,
    editor: &Editor,
    theme: super::render::ThemePalette,
) {
    if let Some(selected) = editor.scope_choice {
        frame.render_widget(
            Paragraph::new("Choose what the rule applies to")
                .style(Style::default().fg(theme.text)),
            Rect::new(area.x, area.y, area.width, 1),
        );
        for (index, &(_, _, label)) in SCOPE_CHOICES
            .iter()
            .enumerate()
            .take(usize::from(area.height.saturating_sub(1)))
        {
            button(
                frame,
                editor,
                Rect::new(area.x, area.y + 1 + index as u16, area.width, 1),
                &format!("{} {label}", index + 1),
                Hit::ScopeChoice(index),
                index == selected,
                theme,
            );
        }
        return;
    }
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
            Constraint::Length(1),
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
        let treatment = if editor.source_stale {
            "…"
        } else {
            treatment_label(source.treatment(node))
        };
        let excerpt = source.excerpt(node.range.clone(), 76);
        let label = if excerpt.is_empty() {
            &node.label
        } else {
            &excerpt
        };
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
                "{} L{} · {treatment} · {label}",
                if index == editor.selected { ">" } else { " " },
                source.node_lines.get(index).copied().unwrap_or(1)
            ),
            Hit::Node(index),
            index == editor.selected,
            theme,
        );
    }
    let scope = SCOPE_CHOICES
        .iter()
        .find(|&&(scope, reuse, _)| scope == editor.rule_scope && reuse == editor.reuse_selector)
        .map_or(SCOPE_CHOICES[0].2, |&(_, _, label)| label);
    let (label, hit) = editor.selected_range.as_ref().map_or_else(
        || (format!("G Scope · {scope}"), Hit::Group),
        |range| {
            (
                range.typed_coordinates.as_ref().map_or_else(
                    || "Selected text · This file version".into(),
                    |coordinates| format!("V Range · {coordinates}"),
                ),
                Hit::Range,
            )
        },
    );
    button(frame, editor, controls[1], &label, hit, false, theme);
    let options =
        Layout::horizontal([Constraint::Length(11), Constraint::Min(1)]).split(controls[2]);
    button(
        frame,
        editor,
        options[0],
        "V Range",
        Hit::Range,
        editor.selected_range.is_some(),
        theme,
    );
    button(
        frame,
        editor,
        options[1],
        "F File pattern",
        Hit::Pattern,
        editor.rule_scope == RuleScope::MatchingFiles,
        theme,
    );
    let label = if editor.rule_scope == RuleScope::MatchingFiles {
        editor
            .confirmed_pattern
            .as_deref()
            .unwrap_or("Choose a file pattern")
    } else if parts.len() == 1 {
        "O Original · select text"
    } else {
        "File preview"
    };
    button(
        frame,
        editor,
        controls[3],
        label,
        if editor.rule_scope == RuleScope::MatchingFiles {
            Hit::Pattern
        } else if parts.len() == 1 {
            Hit::Mode(PreviewMode::Original)
        } else {
            Hit::Mode(PreviewMode::FileDraft)
        },
        false,
        theme,
    );
    render_rule_actions(frame, controls[4], editor, theme);
    let reason = if let Some(range) = &editor.selected_range {
        let excerpt = source.excerpt(range.bytes.clone(), 180);
        format!("Selected locally: {excerpt}\nExact file version · A Allow · M Alias · H Hide")
    } else {
        source.projection.nodes.get(editor.selected).map_or_else(
            || "No selectable content in this file.".into(),
            |node| {
                let excerpt = source.excerpt(node.range.clone(), 180);
                let treatment = if editor.source_stale {
                    "Previous preview"
                } else {
                    treatment_label(source.treatment(node))
                };
                format!("Preview: {treatment}\nLocal selection: {excerpt}")
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
            true,
        );
        text_panel(
            frame,
            parts[2],
            if editor.source_stale {
                "Previous file preview"
            } else if editor.dirty_project() {
                "File preview · draft rules"
            } else {
                "File preview · active rules"
            },
            &source.projection.text,
            editor,
            theme,
            false,
        );
    }
}

fn render_rule_actions(
    frame: &mut ratatui::Frame,
    area: Rect,
    editor: &Editor,
    theme: super::render::ThemePalette,
) {
    let buttons = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(area);
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
            Constraint::Length(if editor.request_target {
                3
            } else if editor.file_original() {
                1
            } else {
                0
            }),
            Constraint::Min(1),
        ])
        .split(area);
    let tabs = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Min(1),
        ])
        .split(parts[0]);
    *editor.request_sources.borrow_mut() = request
        .sources
        .iter()
        .map(|source| (source.path.to_string(), source.digest.to_string()))
        .collect();
    let (first, last) = if editor.request_target {
        (
            ("Sent", PreviewMode::Sent),
            ("File Draft", PreviewMode::FileDraft),
        )
    } else {
        (
            ("Projected", PreviewMode::Sent),
            ("Request", PreviewMode::Request),
        )
    };
    for (index, (label, mode)) in [
        first,
        ("Changes", PreviewMode::Changes),
        ("Original", PreviewMode::Original),
        last,
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
            mode == editor.mode,
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
        let status = if request.sources.is_empty() {
            status
        } else {
            let index = editor
                .link_selected
                .min(request.sources.len().saturating_sub(1));
            editor
                .hits
                .borrow_mut()
                .push((parts[1], Hit::RequestSource(index)));
            format!("{status} · {}", request.sources[index].path)
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
            PreviewMode::FileDraft | PreviewMode::Request | PreviewMode::Sent => (
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
        text_panel(frame, parts[3], title, text, editor, theme, false);
        return;
    }
    let Some(source) = &editor.source else {
        frame.render_widget(
            Paragraph::new("Open a file from Files, or press R to view the latest request.")
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
    let typed_range = editor
        .selected_range
        .as_ref()
        .filter(|range| range.typed_coordinates.is_some());
    if editor.file_original() && editor.selected_range.is_some() {
        render_rule_actions(frame, parts[2], editor, theme);
    }
    let (title, text) = match editor.mode {
        PreviewMode::Original => {
            if let Some(range) = typed_range {
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
        PreviewMode::Sent | PreviewMode::FileDraft | PreviewMode::Request => {
            ("Full · file projection", sent.as_str())
        }
    };
    text_panel(
        frame,
        parts[3],
        title,
        text,
        editor,
        theme,
        editor.file_original() && typed_range.is_none(),
    );
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
            effective: Policy {
                enabled: true,
                ..Policy::default()
            },
            filename_only: false,
            override_note: None,
            active_now: "Active now: filter on · nothing armed".into(),
            profile: crate::provider_filter::ProviderFilterConfig::default(),
            legacy_report: "Legacy stop rules remain armed.".into(),
            observed_sources: vec!["/workspace/plan.md".into()],
            recent_sources: Vec::new(),
        },
    );
    editor.page = Page::Sources;
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
                        sources: &[],
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
        key(&mut editor, KeyCode::Char('3'));
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
        key(&mut editor, KeyCode::Char('3'));
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
        key(&mut editor, KeyCode::Char('4'));
        key(&mut editor, KeyCode::Char('a'));
        let Some(Action::Preview { policy, .. }) = editor.take_action() else {
            panic!("whole file choice must preview")
        };
        assert_eq!(policy.rules.last().unwrap().selector, format!("{shape}/**"));
        key(&mut editor, KeyCode::Char('g'));
        key(&mut editor, KeyCode::Char('5'));
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
    fn privacy_editor_drag_selects_exact_text_across_wrapped_rows() {
        let text = "Visible lead visible lead x migration 記録
Visible tail";
        let begin = text.find("migration").unwrap();
        let end = text.find('録').unwrap();
        let bytes = begin..end + '録'.len_utf8();
        let policy = Policy {
            enabled: true,
            default_action: RuleAction::Allow,
            ..Policy::default()
        };
        let path = "notes.txt";
        let project = |policy: &Policy| {
            SourceView::new(
                path.into(),
                text.into(),
                crate::privacy::projection::project(path, text, policy, &mut |_, _| {
                    Ok("PRIVATE_MARKER".into())
                })
                .unwrap(),
            )
        };
        for width in [60, 120] {
            let mut editor = test_editor();
            editor.policy = Some(policy.clone());
            editor.source_completed(Ok(project(&policy)));
            if width == 60 {
                key(&mut editor, KeyCode::Char('o'));
                assert!(editor.file_original());
            }
            draw(&editor, width, 24);
            let cell = |offset| {
                let map = editor.original_hits.borrow();
                (0..24)
                    .flat_map(|row| (0..width).map(move |col| (col, row)))
                    .find(|&(col, row)| {
                        map.bytes_at(col, row, false)
                            .is_some_and(|range| range.start == offset)
                    })
                    .unwrap()
            };
            let first = cell(begin);
            let last = cell(end);
            if width == 120 {
                assert_ne!(first.1, last.1, "the fixture must cross a soft wrap");
            }
            // Reverse drag from the second terminal cell of a wide grapheme.
            for (phase, (column, row)) in [
                (MouseSelectionPhase::Start, (last.0 + 1, last.1)),
                (MouseSelectionPhase::Drag, first),
            ] {
                editor.handle(&Event::MouseSelection(MouseSelectionEvent {
                    phase,
                    column,
                    row,
                }));
            }
            assert_eq!(editor.source_drag.as_ref().unwrap().bytes(), bytes);
            assert!(editor.take_action().is_none());
            editor.handle(&Event::MouseSelection(MouseSelectionEvent {
                phase: MouseSelectionPhase::End,
                column: first.0,
                row: first.1,
            }));
            assert_eq!(editor.selected_range.as_ref().unwrap().bytes, bytes);
            assert!(editor.take_action().is_none());
            key(&mut editor, KeyCode::Char('h'));
            let Some(Action::Preview { policy, .. }) = editor.take_action() else {
                panic!("Hide must use the drag range");
            };
            assert_eq!(
                policy.rules[0].selector,
                format!(
                    "/@digest/{}/{}/{}",
                    crate::privacy::projection::digest(text),
                    bytes.start,
                    bytes.end
                )
            );
            assert_eq!(
                project(&policy).projection.text,
                text.replace("migration 記録", "PRIVATE_MARKER")
            );
            assert_eq!(editor.selected_range.as_ref().unwrap().bytes, bytes);
        }
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
                            sources: &[],
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
                            sources: &[],
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

    fn draw(editor: &Editor, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                render_with_version_label(
                    frame,
                    frame.area(),
                    editor,
                    "ca test",
                    super::super::render::standalone_palette(),
                    RequestView {
                        generation: 4,
                        original: Some("local original"),
                        sent: "sent body with unattributed note",
                        changes: Some("one change"),
                        latest: "LATEST-REQUEST",
                        sources: &[crate::privacy::SourceLink {
                            id: "receipt-1".into(),
                            path: "notes/my notes.txt".into(),
                            digest: "abc".into(),
                        }],
                        opaque_blocks: 0,
                        complete: true,
                        working: true,
                    },
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|row| {
                (0..width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn fresh_editor() -> Editor {
        Editor::from_snapshot(
            "/workspace".into(),
            Snapshot {
                policy: Policy::default(),
                effective: Policy::default(),
                filename_only: false,
                override_note: Some(
                    "A startup override is active for this process. Saving stores the project policy only."
                        .into(),
                ),
                active_now: "Active now: filter off".into(),
                profile: crate::provider_filter::ProviderFilterConfig::default(),
                legacy_report: String::new(),
                observed_sources: Vec::new(),
                recent_sources: Vec::new(),
            },
        )
    }

    #[test]
    fn filter_overview_is_usable_with_no_rules_and_no_read_files() {
        let editor = fresh_editor();
        let wide = draw(&editor, 100, 30);
        for label in [
            "Active now",
            "Next launch",
            "Unsaved changes",
            "Files",
            "Rules",
            "Preview",
            "Settings",
            "Save project rules",
            "Save for next launch",
            "project policy only",
        ] {
            assert!(wide.contains(label), "{label} missing from {wide}");
        }
        let narrow = draw(&editor, 40, 16);
        for label in [
            "Files",
            "Rules",
            "Preview",
            "Settings",
            "Save project rules",
            "Active now",
        ] {
            assert!(narrow.contains(label), "{label} missing from {narrow}");
        }
        let mut editor = fresh_editor();
        key(&mut editor, KeyCode::Char('e'));
        let narrow = draw(&editor, 20, 6);
        assert!(narrow.contains("Enlarge"), "{narrow}");
        assert!(editor.dirty());
    }

    #[test]
    fn selected_startup_preview_reports_actual_treatment_before_and_after_draft() {
        let mut editor = fresh_editor();
        let snapshot = editor.snapshot.as_mut().unwrap();
        snapshot.effective = crate::privacy::overlay_policy(snapshot.policy.clone(), true, false);
        let active = snapshot.effective.clone();
        let path = "notes.txt";
        let original = "PrivateMarker";
        let project = |policy: &Policy| {
            SourceView::new(
                path.into(),
                original.into(),
                crate::privacy::projection::project(path, original, policy, &mut |_, _| {
                    Ok("PLACEHOLDER".into())
                })
                .unwrap(),
            )
        };
        editor.page = Page::Sources;
        editor.source_completed(Ok(project(&active)));
        for width in [60, 120] {
            let text = draw(&editor, width, 24);
            assert!(
                text.contains("Review before send: on · Selected values only"),
                "{text}"
            );
            assert!(text.contains("L1 · Readable · PrivateMarker"), "{text}");
        }
        key(&mut editor, KeyCode::Char('h'));
        assert!(draw(&editor, 120, 24).contains("Previous file preview"));
        let Some(Action::Preview { policy, .. }) = editor.take_action() else {
            panic!("the rule needs a new projection");
        };
        editor.source_completed(Ok(project(&crate::privacy::overlay_policy(
            policy, true, false,
        ))));
        let text = draw(&editor, 120, 24);
        assert!(text.contains("L1 · Hidden · PrivateMarker"), "{text}");
        assert!(text.contains("PLACEHOLDER"), "{text}");
        assert!(!editor.policy().unwrap().enabled);
    }

    #[test]
    fn rule_form_requires_apply_and_preserves_scope_when_editing() {
        let mut editor = fresh_editor();
        key(&mut editor, KeyCode::Char('3'));
        key(&mut editor, KeyCode::Char('n'));
        assert!(editor.policy().unwrap().rules.is_empty());
        let form = draw(&editor, 60, 24);
        assert!(
            form.contains("Apply to draft") && !form.contains("Selector: "),
            "{form}"
        );
        key(&mut editor, KeyCode::Esc);
        assert!(!editor.dirty());
        key(&mut editor, KeyCode::Char('n'));
        editor.handle(&Event::Paste("docs/*.md".into()));
        key(&mut editor, KeyCode::Enter);
        key(&mut editor, KeyCode::Right);
        key(&mut editor, KeyCode::Enter);
        assert!(editor.policy().unwrap().rules.is_empty());
        key(&mut editor, KeyCode::Enter);
        assert_eq!(
            editor.policy().unwrap().rules[0],
            Rule {
                file: "docs/*.md".into(),
                selector: "/**".into(),
                action: RuleAction::Alias,
            }
        );
        key(&mut editor, KeyCode::Enter);
        key(&mut editor, KeyCode::Enter);
        key(&mut editor, KeyCode::Right);
        key(&mut editor, KeyCode::Enter);
        key(&mut editor, KeyCode::Enter);
        let rule = &editor.policy().unwrap().rules[0];
        assert_eq!(rule.action, RuleAction::Allow);
        assert_eq!(rule.selector, "/**");
        assert_eq!(editor.policy().unwrap().rules.len(), 1);
    }

    #[test]
    fn filter_files_search_opens_spaced_unicode_names_and_drops_stale_results() {
        let mut editor = fresh_editor();
        key(&mut editor, KeyCode::Enter);
        let Some(Action::IndexFiles { generation: stale }) = editor.take_action() else {
            panic!("opening Files starts an index");
        };
        key(&mut editor, KeyCode::Esc);
        key(&mut editor, KeyCode::Char('5'));
        let Some(Action::IndexFiles { generation: latest }) = editor.take_action() else {
            panic!("reopening Files refreshes the index");
        };
        editor.apply_index(stale, vec!["stale spaced.txt".into()], false, None);
        editor.apply_index(
            latest,
            vec!["my notes.txt".into(), "界面.md".into()],
            false,
            None,
        );
        key(&mut editor, KeyCode::Char('界'));
        key(&mut editor, KeyCode::Enter);
        match editor.take_action() {
            Some(Action::LoadSource { path, .. }) => assert_eq!(path, "界面.md"),
            other => panic!("search opens the indexed file, got {other:?}"),
        }
    }

    #[test]
    fn moving_a_later_rule_changes_which_match_wins() {
        let mut editor = test_editor();
        editor.policy.as_mut().unwrap().rules = vec![
            Rule {
                file: "plan.md".into(),
                selector: "/paragraph/0".into(),
                action: RuleAction::Allow,
            },
            Rule {
                file: "plan.md".into(),
                selector: "/paragraph/0".into(),
                action: RuleAction::Hide,
            },
        ];
        assert_eq!(
            editor
                .policy()
                .unwrap()
                .action_for("plan.md", "/paragraph/0"),
            RuleAction::Hide
        );
        key(&mut editor, KeyCode::Char('3'));
        key(&mut editor, KeyCode::Char('.'));
        let policy = editor.policy().unwrap();
        assert_eq!(policy.rules[1].action, RuleAction::Allow);
        assert_eq!(
            policy.action_for("plan.md", "/paragraph/0"),
            RuleAction::Allow
        );
    }

    #[test]
    fn invalid_selector_and_profile_trip_file_stay_beside_the_field() {
        let mut editor = test_editor();
        key(&mut editor, KeyCode::Char('3'));
        key(&mut editor, KeyCode::Char('n'));
        key(&mut editor, KeyCode::Tab);
        key(&mut editor, KeyCode::Tab);
        key(&mut editor, KeyCode::Enter);
        key(&mut editor, KeyCode::Home);
        for _ in 0..3 {
            key(&mut editor, KeyCode::Delete);
        }
        key(&mut editor, KeyCode::Enter);
        key(&mut editor, KeyCode::Enter);
        assert!(editor.error.is_some());
        assert!(!editor.dirty());
        assert!(editor.take_action().is_none());
        key(&mut editor, KeyCode::Esc);
        key(&mut editor, KeyCode::Char('4'));
        for _ in 0..11 {
            key(&mut editor, KeyCode::Down);
        }
        key(&mut editor, KeyCode::Enter);
        for character in ['r', 'e', 'l'] {
            key(&mut editor, KeyCode::Char(character));
        }
        key(&mut editor, KeyCode::Enter);
        editor.handle(&Event::Key(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(editor.take_action().is_none());
        assert!(editor.error.as_deref().unwrap().contains("trip_file"));
    }

    #[test]
    fn startup_override_is_visible_and_omitted_from_the_saved_project_policy() {
        let mut editor = fresh_editor();
        let rendered = draw(&editor, 100, 30);
        assert!(rendered.contains("project policy only"), "{rendered}");
        editor.handle(&Event::Key(KeyCode::Char('s'), KeyModifiers::CONTROL));
        match editor.take_action() {
            Some(Action::Save { policy }) => {
                assert!(!policy.enabled);
                assert!(policy.rules.is_empty());
            }
            other => panic!("save stores the project draft, got {other:?}"),
        }
    }

    #[test]
    fn stale_source_preview_does_not_replace_the_open_file() {
        let mut editor = test_editor();
        editor.apply_source(
            1,
            Ok(SourceView::new(
                "other.md".into(),
                "x".into(),
                crate::privacy::projection::Projection {
                    text: "x".into(),
                    changes: Vec::new(),
                    nodes: Vec::new(),
                    digest: crate::privacy::projection::digest("x"),
                },
            )),
        );
        key(&mut editor, KeyCode::Char('a'));
        match editor.take_action() {
            Some(Action::Preview { path, .. }) => assert_eq!(path, "/workspace/plan.md"),
            other => panic!("stale preview must leave the open file, got {other:?}"),
        }
    }

    #[test]
    fn mouse_and_keyboard_open_the_same_overview_row() {
        let mut editor = fresh_editor();
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| {
                render_with_version_label(
                    frame,
                    frame.area(),
                    &editor,
                    "ca test",
                    super::super::render::standalone_palette(),
                    RequestView {
                        generation: 0,
                        original: None,
                        sent: "",
                        changes: None,
                        latest: "",
                        sources: &[],
                        opaque_blocks: 0,
                        complete: false,
                        working: false,
                    },
                );
            })
            .unwrap();
        let row = editor
            .hits
            .borrow()
            .iter()
            .find(|(_, hit)| matches!(hit, Hit::Overview(Page::Rules)))
            .unwrap()
            .0;
        editor.handle(&Event::MouseSelection(
            crate::tui::event::MouseSelectionEvent {
                phase: MouseSelectionPhase::Start,
                column: row.x,
                row: row.y,
            },
        ));
        assert!(matches!(editor.page, Page::Rules));
        let mut keyboard = fresh_editor();
        key(&mut keyboard, KeyCode::Down);
        key(&mut keyboard, KeyCode::Enter);
        assert!(matches!(keyboard.page, Page::Rules));
    }

    #[test]
    fn preview_keeps_unattributed_text_and_replaces_source_links_with_the_new_request() {
        let mut editor = test_editor();
        key(&mut editor, KeyCode::Char('r'));
        let first = draw(&editor, 100, 30);
        assert!(first.contains("unattributed note"), "{first}");
        assert!(first.contains("notes/my notes.txt"), "{first}");
        assert!(first.contains("LATEST-REQUEST"), "{first}");
        assert!(
            first.contains("Original") && first.contains("local only")
                || first.contains("Original")
        );
    }
}
