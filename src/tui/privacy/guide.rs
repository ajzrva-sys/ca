//! Transient task review. Its owner applies the selected process-only filter.

use super::suggestions::{GuideAnalysis, GuideInput, MatchKind};
use super::*;

const MAX_TERMS: usize = 32;
const MAX_TERM_BYTES: usize = 128;

#[derive(Debug)]
struct Term {
    text: String,
    kind: MatchKind,
    spellings: Vec<String>,
    occurrences: usize,
    historical: usize,
    checked: bool,
}

#[derive(Debug)]
struct EditTerm {
    index: Option<usize>,
    text: String,
    cursor: usize,
}

#[derive(Debug)]
pub(super) struct TaskPreview {
    preset: crate::task_filter::Preset,
    generation: u64,
    ready_generation: Option<u64>,
    preview: Option<crate::task_filter::Preview>,
    remember: bool,
    view: u8,
    inspect_only: bool,
}

#[derive(Debug)]
pub(super) struct Guide {
    terms: Vec<Term>,
    has_urls: bool,
    hide_urls: bool,
    selected: usize,
    edit: Option<EditTerm>,
    truncated: bool,
    scanned_bytes: usize,
    scanned_parts: usize,
}

impl Guide {
    fn rows(&self) -> usize {
        self.terms.len() + usize::from(self.has_urls)
    }

    fn selected_term(&self) -> Option<usize> {
        self.selected.checked_sub(usize::from(self.has_urls))
    }
}

impl Editor {
    /// The owner has already resolved preset defaults and scanned task inputs.
    /// Selected additions are exact: unchecked suggestions are never re-added.
    pub fn task_preview(
        workspace: String,
        policy: crate::task_filter::Policy,
        analysis: GuideAnalysis,
    ) -> Self {
        let mut editor = Self::preparing(workspace, None);
        editor.apply_guide_analysis(Ok(analysis));
        editor.actions.clear();
        editor.saving = false;
        let guide = editor.guide.as_mut().expect("prepared guide");
        guide.has_urls = false;
        guide.hide_urls = false;
        for term in &mut guide.terms {
            term.checked = match term.kind {
                MatchKind::Phrase => policy
                    .words
                    .iter()
                    .any(|word| word.eq_ignore_ascii_case(&term.text)),
                MatchKind::Identifier => term
                    .spellings
                    .iter()
                    .any(|text| policy.identifiers.contains(text)),
            };
            if term.checked && term.kind == MatchKind::Identifier {
                term.spellings
                    .retain(|text| policy.identifiers.contains(text));
            }
        }
        for (kind, entries) in [
            (MatchKind::Phrase, &policy.words),
            (MatchKind::Identifier, &policy.identifiers),
        ] {
            for text in entries {
                if !guide
                    .terms
                    .iter()
                    .any(|term| term.kind == kind && term.spellings.contains(text))
                {
                    guide.terms.push(Term {
                        text: text.clone(),
                        kind,
                        spellings: vec![text.clone()],
                        occurrences: 0,
                        historical: 0,
                        checked: true,
                    });
                }
            }
        }
        editor.task_preview = Some(TaskPreview {
            preset: policy.preset,
            generation: 0,
            ready_generation: None,
            preview: None,
            remember: false,
            view: 2,
            inspect_only: false,
        });
        editor.queue_task_preview();
        editor
    }

    #[allow(dead_code)]
    pub fn task_inspection(
        workspace: String,
        policy: crate::task_filter::Policy,
        preview: crate::task_filter::Preview,
    ) -> Self {
        let analysis = super::suggestions::analyze("", "", std::iter::empty());
        let mut editor = Self::task_preview(workspace, policy, analysis);
        editor.actions.clear();
        let task = editor.task_preview.as_mut().expect("task preview");
        task.inspect_only = true;
        task.ready_generation = Some(task.generation);
        task.preview = Some(preview);
        editor
    }

    pub fn is_task_preview(&self) -> bool {
        self.task_preview.is_some()
    }

    pub fn take_task_cancel(&mut self) -> bool {
        let Some(index) = self
            .actions
            .iter()
            .position(|action| matches!(action, Action::Close))
        else {
            return false;
        };
        self.actions.remove(index);
        true
    }

    pub fn take_disable_preview(&mut self) -> bool {
        let Some(index) = self
            .actions
            .iter()
            .position(|action| matches!(action, Action::DisablePreview))
        else {
            return false;
        };
        self.actions.remove(index);
        true
    }

    pub fn refresh_task_inspection(
        &mut self,
        generation: u64,
        preview: crate::task_filter::Preview,
    ) {
        let Some(task) = &mut self.task_preview else {
            return;
        };
        if !task.inspect_only || task.generation == generation {
            return;
        }
        task.generation = generation;
        task.ready_generation = Some(generation);
        task.preview = Some(preview);
        // Keep the selected pane, but reveal the new request from its start.
        self.reset_scroll();
    }

    pub fn task_preview_generation(&self) -> Option<u64> {
        self.task_preview.as_ref().map(|task| task.generation)
    }

    pub fn refresh_task_preview(&mut self) {
        self.saving = false;
        self.queue_task_preview();
    }

    pub fn task_preview_policy(&self) -> Option<crate::task_filter::Policy> {
        let task = self.task_preview.as_ref()?;
        let guide = self.guide.as_ref()?;
        Some(crate::task_filter::Policy {
            preset: task.preset,
            words: guide
                .terms
                .iter()
                .filter(|term| term.checked && term.kind == MatchKind::Phrase)
                .map(|term| term.text.clone())
                .collect(),
            identifiers: guide
                .terms
                .iter()
                .filter(|term| term.checked && term.kind == MatchKind::Identifier)
                .flat_map(|term| term.spellings.clone())
                .collect(),
        })
    }

    pub fn apply_task_preview(
        &mut self,
        generation: u64,
        result: Result<crate::task_filter::Preview, String>,
    ) {
        let Some(task) = self.task_preview.as_mut() else {
            return;
        };
        if task.generation != generation || self.saving {
            return;
        }
        match result {
            Ok(preview) => {
                task.preview = Some(preview);
                task.ready_generation = Some(generation);
                self.error = None;
            }
            Err(error) => {
                task.ready_generation = None;
                self.error = Some(error);
            }
        }
    }

    fn queue_task_preview(&mut self) {
        let Some(task) = self.task_preview.as_mut() else {
            return;
        };
        if task.inspect_only {
            return;
        }
        task.generation = task.generation.wrapping_add(1);
        task.ready_generation = None;
        let generation = task.generation;
        self.error = None;
        self.actions
            .retain(|action| !matches!(action, Action::PreviewTask { .. }));
        if let Some(policy) = self.task_preview_policy() {
            self.actions
                .push_back(Action::PreviewTask { policy, generation });
        }
    }

    pub(super) fn cycle_task_preset(&mut self) {
        let Some(task) = self.task_preview.as_mut() else {
            return;
        };
        if task.inspect_only || self.saving {
            return;
        }
        task.preset = match task.preset {
            crate::task_filter::Preset::PrivateValues => {
                crate::task_filter::Preset::PrivateValuesAndSuggestions
            }
            crate::task_filter::Preset::PrivateValuesAndSuggestions => {
                crate::task_filter::Preset::PrivateValues
            }
        };
        if let Some(guide) = &mut self.guide {
            for term in &mut guide.terms {
                // Explicit user additions survive switching the suggested preset.
                if term.occurrences > 0 {
                    term.checked =
                        task.preset == crate::task_filter::Preset::PrivateValuesAndSuggestions;
                }
            }
        }
        self.queue_task_preview();
    }

    pub(super) fn select_task_view(&mut self, view: u8) {
        if let Some(task) = &mut self.task_preview {
            task.view = view.min(3);
            self.scroll.set(0);
            self.window.set(0);
        }
    }

    pub(super) fn toggle_task_remember(&mut self) {
        if let Some(task) = &mut self.task_preview {
            if !task.inspect_only && !self.saving {
                task.remember = !task.remember;
            }
        }
    }

    pub(super) fn open_task_advanced(&mut self) {
        if self
            .task_preview
            .as_ref()
            .is_some_and(|task| task.inspect_only)
        {
            *self = Self::loading(self.workspace.clone());
        }
    }

    fn send_task_preview(&mut self) {
        let Some(task) = &self.task_preview else {
            return;
        };
        if task.inspect_only {
            self.open_task_advanced();
            return;
        }
        if task.ready_generation != Some(task.generation) {
            if self.error.is_some() {
                self.queue_task_preview();
            }
            return;
        }
        if task
            .preview
            .as_ref()
            .is_some_and(|preview| preview.blocked.is_some())
        {
            return;
        }
        let remember = task.remember;
        let generation = task.generation;
        if let Some(policy) = self.task_preview_policy() {
            self.actions.push_back(Action::SendTask {
                policy,
                remember,
                generation,
            });
            self.saving = true;
        }
    }

    /// Inspect only the task and attachment bytes already read for this draft.
    #[allow(dead_code)]
    pub fn guided(workspace: String, task: &str, expanded: &str) -> Option<Self> {
        let analysis = super::suggestions::analyze(task, expanded, std::iter::empty());
        if !analysis.needs_review() {
            return None;
        }
        let mut editor = Self::preparing(workspace, None);
        editor.apply_guide_analysis(Ok(analysis));
        Some(editor)
    }

    pub fn preparing(workspace: String, input: Option<GuideInput>) -> Self {
        let mut editor = Self::loading(workspace);
        editor.actions.clear();
        editor.loading = input.is_some();
        editor.page = Page::Guide;
        editor.guide = Some(Guide {
            terms: Vec::new(),
            has_urls: false,
            hide_urls: false,
            selected: 0,
            edit: None,
            truncated: false,
            scanned_bytes: 0,
            scanned_parts: 0,
        });
        if let Some(input) = input {
            editor.actions.push_back(Action::AnalyzeGuide(input));
        }
        editor
    }

    pub fn take_guide_analysis(&mut self) -> Option<GuideInput> {
        if !matches!(self.actions.front(), Some(Action::AnalyzeGuide(_))) {
            return None;
        }
        match self.actions.pop_front() {
            Some(Action::AnalyzeGuide(input)) => Some(input),
            _ => None,
        }
    }

    pub fn guide_analyzing(&self) -> bool {
        self.is_guided() && self.loading
    }

    pub fn apply_guide_analysis(&mut self, result: Result<GuideAnalysis, String>) {
        self.loading = false;
        let analysis = match result {
            Ok(analysis) => analysis,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let needs_review = analysis.needs_review();
        self.guide = Some(Guide {
            terms: analysis
                .terms
                .into_iter()
                .map(|term| Term {
                    text: term.text,
                    kind: term.kind,
                    spellings: term.spellings,
                    occurrences: term.occurrences,
                    historical: term.historical,
                    checked: true,
                })
                .collect(),
            has_urls: analysis.has_urls,
            hide_urls: analysis.has_urls,
            selected: 0,
            edit: None,
            truncated: !analysis.complete,
            scanned_bytes: analysis.scanned_bytes,
            scanned_parts: analysis.scanned_parts,
        });
        if !needs_review {
            self.continue_guide(false);
        }
    }

    pub fn is_guided(&self) -> bool {
        self.page == Page::Guide
    }

    pub fn apply_guide_failed(&mut self, error: String) {
        self.saving = false;
        self.error = Some(error);
    }

    pub(super) fn toggle_guide_row(&mut self, index: usize) {
        if self
            .task_preview
            .as_ref()
            .is_some_and(|task| task.inspect_only)
        {
            return;
        }
        let Some(guide) = self.guide.as_mut() else {
            return;
        };
        guide.selected = index.min(guide.rows().saturating_sub(1));
        if guide.has_urls && index == 0 {
            guide.hide_urls = !guide.hide_urls;
        } else if let Some(term) = guide
            .selected_term()
            .and_then(|index| guide.terms.get_mut(index))
        {
            term.checked = !term.checked;
        }
        self.error = None;
        self.queue_task_preview();
    }

    pub(super) fn continue_guide(&mut self, apply: bool) {
        if self.saving {
            return;
        }
        if self.task_preview.is_some() {
            self.send_task_preview();
            return;
        }
        let Some(guide) = self.guide.as_ref() else {
            return;
        };
        self.actions.push_back(Action::ContinueTask {
            words: if apply {
                guide
                    .terms
                    .iter()
                    .filter(|term| term.checked && term.kind == MatchKind::Phrase)
                    .map(|term| term.text.clone())
                    .collect()
            } else {
                Vec::new()
            },
            identifiers: if apply {
                guide
                    .terms
                    .iter()
                    .filter(|term| term.checked && term.kind == MatchKind::Identifier)
                    .flat_map(|term| term.spellings.clone())
                    .collect()
            } else {
                Vec::new()
            },
            hide_urls: apply && guide.hide_urls,
        });
        self.saving = true;
        self.error = None;
    }

    pub(super) fn breakdown_guide(&mut self) {
        if !self.loading && !self.saving {
            self.actions.push_back(Action::BreakdownTask);
            self.saving = true;
        }
    }

    fn finish_guide_term(&mut self) {
        let guide = self.guide.as_mut().unwrap();
        let Some(edit) = guide.edit.as_ref() else {
            return;
        };
        let text = edit.text.trim();
        if text.is_empty() {
            self.error = Some("Enter a term, or Esc to cancel the edit.".into());
            return;
        }
        if guide
            .terms
            .iter()
            .enumerate()
            .any(|(index, term)| Some(index) != edit.index && term.text.eq_ignore_ascii_case(text))
        {
            self.error = Some("That term is already listed. Esc returns to the list.".into());
            return;
        }
        let term = Term {
            text: text.into(),
            kind: edit
                .index
                .map_or(MatchKind::Phrase, |index| guide.terms[index].kind),
            spellings: vec![text.into()],
            occurrences: 0,
            historical: 0,
            checked: true,
        };
        let index = if let Some(index) = edit.index {
            guide.terms[index] = term;
            index
        } else {
            guide.terms.push(term);
            guide.terms.len() - 1
        };
        guide.selected = index + usize::from(guide.has_urls);
        guide.edit = None;
        self.error = None;
        self.queue_task_preview();
    }

    pub(super) fn handle_guide(&mut self, event: &Event) {
        if self.loading {
            match event {
                Event::Key(KeyCode::Esc, _) => {
                    self.actions.clear();
                    self.loading = false;
                    self.actions.push_back(Action::Close);
                }
                Event::Key(KeyCode::Char('o' | 'O'), _) => {
                    self.actions.clear();
                    self.loading = false;
                    self.actions.push_back(Action::DisablePreview);
                }
                Event::Key(KeyCode::Char('u' | 'U'), _) => {
                    self.actions.clear();
                    self.loading = false;
                    self.continue_guide(false);
                }
                _ => {}
            }
            return;
        }
        if self.saving {
            return;
        }
        let Some(guide) = self.guide.as_mut() else {
            return;
        };
        if let Some(edit) = guide.edit.as_mut() {
            match event {
                Event::Key(KeyCode::Esc, _) => {
                    guide.edit = None;
                    self.error = None;
                }
                Event::Key(KeyCode::Enter, _) => self.finish_guide_term(),
                Event::Paste(text) => {
                    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
                    Self::insert_limited(&mut edit.text, &mut edit.cursor, &clean, MAX_TERM_BYTES);
                }
                Event::Key(key, modifiers)
                    if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    match key {
                        KeyCode::Left => edit.cursor = previous_grapheme(&edit.text, edit.cursor),
                        KeyCode::Right => edit.cursor = next_grapheme(&edit.text, edit.cursor),
                        KeyCode::Home => edit.cursor = 0,
                        KeyCode::End => edit.cursor = edit.text.len(),
                        KeyCode::Backspace => {
                            let previous = previous_grapheme(&edit.text, edit.cursor);
                            edit.text.replace_range(previous..edit.cursor, "");
                            edit.cursor = previous;
                        }
                        KeyCode::Delete => {
                            let next = next_grapheme(&edit.text, edit.cursor);
                            edit.text.replace_range(edit.cursor..next, "");
                        }
                        KeyCode::Char(character) if !character.is_control() => {
                            Self::insert_limited(
                                &mut edit.text,
                                &mut edit.cursor,
                                &character.to_string(),
                                MAX_TERM_BYTES,
                            );
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            return;
        }
        if let Some(task) = self.task_preview.as_ref() {
            let view = task.view;
            let inspect_only = task.inspect_only;
            match event {
                Event::Key(KeyCode::Esc, _)
                | Event::Key(KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                    self.actions.clear();
                    self.actions.push_back(Action::Close);
                    return;
                }
                Event::Key(KeyCode::Char('o' | 'O'), _) => {
                    self.actions.clear();
                    self.actions.push_back(Action::DisablePreview);
                    return;
                }
                Event::Key(KeyCode::Enter, _) => {
                    self.send_task_preview();
                    return;
                }
                Event::Key(KeyCode::Char('p' | 'P'), _) => {
                    self.cycle_task_preset();
                    return;
                }
                Event::Key(KeyCode::Char('r' | 'R'), _) => {
                    self.toggle_task_remember();
                    return;
                }
                Event::Key(KeyCode::Char('g' | 'G'), _) if inspect_only => {
                    self.open_task_advanced();
                    return;
                }
                Event::Key(KeyCode::Char(number @ '1'..='4'), _) => {
                    self.select_task_view(*number as u8 - b'1');
                    return;
                }
                Event::Key(KeyCode::Char('u' | 'U' | 'b' | 'B'), _) => return,
                Event::Key(KeyCode::Char('a' | 'A' | 'e' | 'E' | ' '), _) if inspect_only => return,
                Event::Key(KeyCode::Char('a' | 'A' | 'e' | 'E'), _) => self.select_task_view(0),
                Event::Key(KeyCode::Up, _) if view != 0 => {
                    self.scroll_by(-1);
                    return;
                }
                Event::Key(KeyCode::Down, _) if view != 0 => {
                    self.scroll_by(1);
                    return;
                }
                Event::Key(KeyCode::PageUp, _) if view != 0 => {
                    self.scroll_by(-i32::from(self.visible_rows.get()));
                    return;
                }
                Event::Key(KeyCode::PageDown, _) if view != 0 => {
                    self.scroll_by(i32::from(self.visible_rows.get()));
                    return;
                }
                Event::Key(KeyCode::Home, _) if view != 0 => {
                    self.reset_scroll();
                    return;
                }
                Event::Key(KeyCode::End, _) if view != 0 => {
                    self.window.set(self.max_window.get());
                    self.scroll.set(u16::MAX);
                    return;
                }
                Event::Key(KeyCode::Char(' '), _) if view != 0 => return,
                _ => {}
            }
        }
        let guide = self.guide.as_mut().expect("guide remains open");
        let selected = guide.selected;
        match event {
            Event::Key(KeyCode::Esc, _) | Event::Key(KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.actions.push_back(Action::Close);
            }
            Event::Key(KeyCode::Enter, _) => self.continue_guide(true),
            Event::Key(KeyCode::Char('u' | 'U'), _) => self.continue_guide(false),
            Event::Key(KeyCode::Char('b' | 'B'), _) => self.breakdown_guide(),
            Event::Key(KeyCode::Up, _) => guide.selected = guide.selected.saturating_sub(1),
            Event::Key(KeyCode::Down, _) => {
                guide.selected = (guide.selected + 1).min(guide.rows().saturating_sub(1));
            }
            Event::Key(KeyCode::Char(' '), _) => self.toggle_guide_row(selected),
            Event::Key(KeyCode::Char('a' | 'A'), _) => {
                if guide.terms.len() >= MAX_TERMS {
                    self.error = Some(
                        "This review holds up to 32 terms. Edit an existing term with E.".into(),
                    );
                } else {
                    guide.edit = Some(EditTerm {
                        index: None,
                        text: String::new(),
                        cursor: 0,
                    });
                    self.error = None;
                }
            }
            Event::Key(KeyCode::Char('e' | 'E'), _) => {
                if let Some((index, term)) = guide
                    .selected_term()
                    .and_then(|index| guide.terms.get(index).map(|term| (index, term)))
                {
                    guide.edit = Some(EditTerm {
                        index: Some(index),
                        cursor: term.text.len(),
                        text: term.text.clone(),
                    });
                    self.error = None;
                }
            }
            Event::MouseSelection(event) if event.phase == MouseSelectionPhase::Start => {
                if let Some(hit) = self.hit_at(event.column, event.row) {
                    self.hit(hit);
                }
            }
            _ => {}
        }
    }
}

pub(super) fn render(
    frame: &mut ratatui::Frame,
    area: Rect,
    editor: &Editor,
    theme: super::super::render::ThemePalette,
) {
    if editor.task_preview.is_some() {
        render_task_preview(frame, area, editor, theme);
        return;
    }
    let Some(guide) = editor.guide.as_ref() else {
        return;
    };
    let areas = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(4),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new("Review task and filters before sending")
            .style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        areas[0],
    );
    frame.render_widget(
        Paragraph::new(concat!(
            "Enter adds selected filters for this CA session.\n",
            "B breaks a mixed request into focused tasks for review.\n",
            "Breakdown keeps existing filters and shared purpose.\n",
            "Suggestions are not known provider-banned terms."
        ))
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(theme.muted)),
        areas[1],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{} choices · {} sections · {} bytes checked{}",
            guide.terms.len(),
            guide.scanned_parts,
            guide.scanned_bytes,
            if guide.truncated { " · PARTIAL" } else { "" }
        )),
        areas[2],
    );
    if let Some(edit) = &guide.edit {
        render_field(
            frame,
            editor,
            areas[3],
            ("Term", &edit.text, Some(edit.cursor)),
            Hit::GuideRow(guide.selected),
            theme,
        );
    } else if guide.rows() == 0 {
        frame.render_widget(
            Paragraph::new(if editor.loading {
                "Checking the task and visible conversation history…"
            } else {
                "No new suggestions. Existing filters stay active. A adds a term."
            })
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(theme.muted)),
            areas[3],
        );
    } else {
        let height = usize::from(areas[3].height);
        let start = guide.selected.saturating_sub(height.saturating_sub(1));
        for index in start..guide.rows().min(start + height) {
            let (text, checked) = if guide.has_urls && index == 0 {
                ("Hide URL hosts (keep paths)".to_string(), guide.hide_urls)
            } else {
                let term = &guide.terms[index - usize::from(guide.has_urls)];
                (
                    format!(
                        "{} · {} · {} hits{}",
                        term.text,
                        if term.kind == MatchKind::Identifier {
                            "alias"
                        } else {
                            "hide"
                        },
                        term.occurrences,
                        if term.historical > 0 {
                            format!(" ({} in history)", term.historical)
                        } else {
                            String::new()
                        }
                    ),
                    term.checked,
                )
            };
            button(
                frame,
                editor,
                Rect::new(
                    areas[3].x,
                    areas[3].y + (index - start) as u16,
                    areas[3].width,
                    1,
                ),
                &format!(
                    "{} [{}] {}",
                    if guide.selected == index { ">" } else { " " },
                    if checked { "x" } else { " " },
                    text
                ),
                Hit::GuideRow(index),
                guide.selected == index,
                theme,
            );
        }
    }
    let detail = editor.error.as_deref().unwrap_or(if editor.loading {
        "Checking all supplied text locally · U Continue unchanged · Esc Edit task"
    } else if editor.saving {
        "Preparing your task…"
    } else if guide.edit.is_some() {
        "Enter keeps this term · Esc cancels the edit"
    } else {
        "↑↓ Choose · Space Toggle · A Add term · E Edit term"
    });
    frame.render_widget(
        Paragraph::new(detail)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(if editor.error.is_some() {
                theme.warning
            } else {
                theme.muted
            })),
        areas[4],
    );
    for (index, label, hit, active) in [
        (5, "Enter  Apply selected & continue", Hit::GuideApply, true),
        (6, "U      Continue unchanged", Hit::GuideUnchanged, false),
        (
            7,
            "B      Break into focused tasks",
            Hit::GuideBreakdown,
            false,
        ),
        (8, "Esc    Edit task", Hit::Close, false),
    ] {
        button(frame, editor, areas[index], label, hit, active, theme);
    }
}

fn render_task_preview(
    frame: &mut ratatui::Frame,
    area: Rect,
    editor: &Editor,
    theme: super::super::render::ThemePalette,
) {
    let task = editor.task_preview.as_ref().expect("task preview");
    let guide = editor.guide.as_ref().expect("task guide");
    let fresh = task.ready_generation == Some(task.generation);
    let blocked = task
        .preview
        .as_ref()
        .and_then(|preview| preview.blocked.as_deref());
    let coverage = if fresh && blocked.is_some() {
        "Filter stopped this request. Change the selected rules or edit your task."
    } else if fresh {
        task.preview
            .as_ref()
            .map_or("", |preview| preview.coverage.as_str())
    } else if editor.error.is_some() {
        "Preview unavailable. Enter retries; nothing has been sent."
    } else {
        "Updating preview… Send is available when the latest copy is ready."
    };
    let mut coverage = crate::terminal_text::sanitize_terminal_text(coverage);
    if fresh {
        if let Some(preview) = task.preview.as_ref() {
            for (name, alias) in &preview.filename_samples {
                coverage.push('\n');
                coverage.push_str(name);
                coverage.push_str(" → ");
                coverage.push_str(alias);
            }
        }
    }
    let coverage_rows = Paragraph::new(coverage.as_str())
        .wrap(Wrap { trim: false })
        .line_count(area.width.max(1))
        .min(usize::from(area.height.saturating_sub(11).max(1))) as u16;
    let areas = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(coverage_rows),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .split(area);
    let heading = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(areas[0]);
    frame.render_widget(
        Paragraph::new(if task.inspect_only {
            "Filter on · this task"
        } else {
            "Filter on · review before send"
        })
        .style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        heading[0],
    );
    frame.render_widget(
        Paragraph::new(if task.inspect_only {
            "O turns this off for this ca. Esc closes."
        } else {
            "O turns this off for this ca. Esc keeps the draft."
        })
        .style(Style::default().fg(theme.muted)),
        heading[1],
    );
    let preset = match task.preset {
        crate::task_filter::Preset::PrivateValues => "Private values",
        crate::task_filter::Preset::PrivateValuesAndSuggestions => "Private values + suggestions",
    };
    button(
        frame,
        editor,
        areas[1],
        &format!(
            "{}Preset: {preset}",
            if task.inspect_only { "" } else { "P  " }
        ),
        Hit::TaskPreset,
        false,
        theme,
    );
    let tabs = Layout::horizontal([
        Constraint::Length(9),
        Constraint::Length(12),
        Constraint::Length(17),
        Constraint::Min(1),
    ])
    .split(areas[2]);
    for (index, label) in ["1 Rules", "2 Original", "3 Provider copy", "4 Changes"]
        .into_iter()
        .enumerate()
    {
        button(
            frame,
            editor,
            tabs[index],
            label,
            Hit::TaskView(index as u8),
            task.view == index as u8,
            theme,
        );
    }
    if let Some(edit) = &guide.edit {
        render_field(
            frame,
            editor,
            areas[3],
            ("Term", &edit.text, Some(edit.cursor)),
            Hit::GuideRow(guide.selected),
            theme,
        );
    } else if task.view == 0 {
        if guide.terms.is_empty() {
            frame.render_widget(Paragraph::new("Private-value filtering is configured automatically.\nNo extra terms selected. A adds a term.").wrap(Wrap { trim: false }), areas[3]);
        } else {
            let height = usize::from(areas[3].height);
            let start = guide.selected.saturating_sub(height.saturating_sub(1));
            for (index, term) in guide.terms.iter().enumerate().skip(start).take(height) {
                let text = crate::terminal_text::sanitize_terminal_text(&term.text);
                button(
                    frame,
                    editor,
                    Rect::new(
                        areas[3].x,
                        areas[3].y + (index - start) as u16,
                        areas[3].width,
                        1,
                    ),
                    &format!(
                        "{} [{}] {} · {}",
                        if guide.selected == index { ">" } else { " " },
                        if term.checked { "x" } else { " " },
                        text,
                        if term.kind == MatchKind::Identifier {
                            "alias"
                        } else {
                            "hide"
                        }
                    ),
                    Hit::GuideRow(index),
                    guide.selected == index,
                    theme,
                );
            }
        }
    } else if let Some(preview) = &task.preview {
        let changes = format!("{}\n\nCoverage\n{}", preview.changes, preview.coverage);
        let (title, text) = match task.view {
            1 => ("Original", preview.original.as_str()),
            2 => ("Provider copy", preview.provider_copy.as_str()),
            _ => ("Changes", changes.as_str()),
        };
        text_panel(frame, areas[3], title, text, editor, theme, false);
    } else {
        frame.render_widget(
            Paragraph::new("Preparing the provider copy locally…").wrap(Wrap { trim: false }),
            areas[3],
        );
    }
    frame.render_widget(
        Paragraph::new(coverage)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(theme.muted)),
        areas[4],
    );
    if !task.inspect_only {
        button(
            frame,
            editor,
            areas[5],
            &format!(
                "R  [{}] Remember for this project",
                if task.remember { "x" } else { " " }
            ),
            Hit::TaskRemember,
            false,
            theme,
        );
    }
    let detail = editor
        .error
        .as_deref()
        .or(blocked)
        .unwrap_or(if editor.saving {
            "Saving filters and preparing your task…"
        } else if guide.edit.is_some() {
            "Enter keeps this term · Esc cancels the edit"
        } else if task.inspect_only {
            "1–4 View · ↑↓ Scroll · G Advanced rules"
        } else if task.view == 0 {
            "↑↓ Choose · Space Toggle · A Add · E Edit"
        } else {
            "1–4 View · ↑↓ Scroll · A Add a term · P Change preset"
        });
    frame.render_widget(
        Paragraph::new(crate::terminal_text::sanitize_terminal_text(detail))
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(if editor.error.is_some() {
                theme.warning
            } else {
                theme.muted
            })),
        areas[6],
    );
    let actions = Layout::horizontal([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(areas[7]);
    button(
        frame,
        editor,
        actions[0],
        if task.inspect_only {
            "Enter  Advanced rules"
        } else if !fresh || editor.saving {
            "Preparing…"
        } else {
            "Enter  Send and run"
        },
        if task.inspect_only {
            Hit::TaskAdvanced
        } else {
            Hit::GuideApply
        },
        fresh && !editor.saving && (task.inspect_only || blocked.is_none()),
        theme,
    );
    button(
        frame,
        editor,
        actions[1],
        if task.inspect_only {
            "Esc  Close"
        } else {
            "Esc  Edit task"
        },
        Hit::Close,
        false,
        theme,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn key(editor: &mut Editor, key: KeyCode) {
        editor.handle(&Event::Key(key, KeyModifiers::NONE));
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
                    super::super::super::render::standalone_palette(),
                    RequestView {
                        generation: 0,
                        original: None,
                        sent: "",
                        changes: None,
                        latest: "",
                        sources: &[],
                        opaque_blocks: 0,
                        complete: true,
                        working: false,
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

    fn preview(text: &str) -> crate::task_filter::Preview {
        crate::task_filter::Preview {
            original: "Original private task".into(),
            provider_copy: text.into(),
            changes: "Private values replaced locally".into(),
            coverage: "Prompt and attachments checked locally".into(),
            blocked: None,
            filename_samples: Vec::new(),
        }
    }

    fn task_editor(text: &str) -> Editor {
        Editor::task_preview(
            "/w".into(),
            crate::task_filter::Policy::default(),
            super::super::suggestions::analyze(text, text, std::iter::empty()),
        )
    }

    #[test]
    fn task_filter_preview_offers_off_and_hides_send_until_ready() {
        let mut editor = task_editor("Review private task");
        let _ = editor.take_action();
        let text = draw(&editor, 90, 30);
        assert!(text.contains("Filter on · review before send"), "{text}");
        assert!(text.contains("O turns this off for this ca"), "{text}");
        assert!(text.contains("Preparing"), "{text}");
        assert!(!text.contains("Send and run"), "{text}");
        key(&mut editor, KeyCode::Char('O'));
        assert!(matches!(editor.take_action(), Some(Action::DisablePreview)));
    }

    #[test]
    fn task_filter_preview_plain_task_waits_for_fresh_copy_and_sends_once() {
        let mut editor = task_editor("Fix the menu");
        let Some(Action::PreviewTask { generation, policy }) = editor.take_action() else {
            panic!("plain task still prepares a preview")
        };
        assert!(policy.words.is_empty());
        key(&mut editor, KeyCode::Enter);
        assert!(
            editor.take_action().is_none(),
            "cannot send an unfinished preview"
        );
        editor.apply_task_preview(generation, Ok(preview("Prepared copy")));
        key(&mut editor, KeyCode::Char('r'));
        key(&mut editor, KeyCode::Enter);
        assert!(
            matches!(editor.take_action(), Some(Action::SendTask { remember: true, generation: actual, .. }) if actual == generation)
        );
        key(&mut editor, KeyCode::Enter);
        assert!(editor.take_action().is_none());
    }

    #[test]
    fn task_filter_preview_edits_invalidate_results_and_preserve_exact_selection() {
        let mut editor = task_editor("Review malware and EPT");
        let old = editor.task_preview_generation().unwrap();
        editor.take_action();
        key(&mut editor, KeyCode::Char('p'));
        let Some(Action::PreviewTask { policy, generation }) = editor.take_action() else {
            panic!("preset refresh")
        };
        assert_eq!(policy.words, ["malware"]);
        assert_eq!(policy.identifiers, ["EPT"]);
        editor.apply_task_preview(old, Ok(preview("STALE COPY")));
        key(&mut editor, KeyCode::Enter);
        assert!(editor.take_action().is_none());
        editor.apply_task_preview(generation, Ok(preview("LATEST COPY")));
        assert!(draw(&editor, 80, 24).contains("LATEST COPY"));
        assert!(!draw(&editor, 80, 24).contains("STALE COPY"));
        key(&mut editor, KeyCode::Char('1'));
        key(&mut editor, KeyCode::Char(' '));
        let Some(Action::PreviewTask { policy, .. }) = editor.take_action() else {
            panic!("toggle refresh")
        };
        assert!(
            policy.words.is_empty(),
            "deselected suggestion remains excluded"
        );
        assert_eq!(policy.identifiers, ["EPT"]);
    }

    #[test]
    fn task_filter_preview_unicode_addition_refreshes_and_cancel_never_sends() {
        let mut editor = task_editor("Fix menu");
        editor.take_action();
        key(&mut editor, KeyCode::Char('a'));
        editor.handle(&Event::Paste("私有 café".into()));
        key(&mut editor, KeyCode::Enter);
        let Some(Action::PreviewTask { policy, .. }) = editor.take_action() else {
            panic!("addition refresh")
        };
        assert_eq!(policy.words, ["私有 café"]);
        key(&mut editor, KeyCode::Char('e'));
        key(&mut editor, KeyCode::Backspace);
        key(&mut editor, KeyCode::Char('e'));
        key(&mut editor, KeyCode::Enter);
        assert_eq!(editor.task_preview_policy().unwrap().words, ["私有 cafe"]);
        key(&mut editor, KeyCode::Esc);
        assert!(matches!(editor.take_action(), Some(Action::Close)));
        assert!(
            editor.take_action().is_none(),
            "cancel discards pending preview actions"
        );
    }

    #[test]
    fn task_filter_preview_inspection_cannot_send_or_change_policy() {
        let mut editor =
            Editor::task_inspection("/w".into(), Default::default(), preview("Active request"));
        let policy = editor.task_preview_policy();
        for pressed in ['p', 'r', 'a', ' '] {
            key(&mut editor, KeyCode::Char(pressed));
        }
        assert_eq!(editor.task_preview_policy(), policy);
        assert!(editor.take_action().is_none());
        key(&mut editor, KeyCode::Enter);
        assert!(!editor.is_task_preview());
        assert!(matches!(editor.take_action(), Some(Action::LoadSnapshot)));
    }

    #[test]
    fn task_filter_preview_blocked_request_stays_reviewable_and_cannot_send() {
        let mut editor = task_editor("Review private task");
        let generation = editor.task_preview_generation().unwrap();
        editor.take_action();
        let mut blocked = preview("Filtered copy remains inspectable");
        blocked.blocked = Some("Matched stop rule: private value".into());
        editor.apply_task_preview(generation, Ok(blocked));
        let text = draw(&editor, 90, 24);
        assert!(text.contains("Matched stop rule"));
        assert!(text.contains("Filtered copy remains inspectable"));
        key(&mut editor, KeyCode::Enter);
        assert!(editor.take_action().is_none());
        key(&mut editor, KeyCode::Esc);
        assert!(matches!(editor.take_action(), Some(Action::Close)));
    }

    #[test]
    fn task_filter_preview_keeps_discovery_gaps_and_send_visible() {
        let mut editor = task_editor("List the project folders");
        let generation = editor.task_preview_generation().unwrap();
        editor.take_action();
        let mut partial = preview("Prepared folder listing task");
        let gap = "Filename discovery is limited: 4 folders could not be read; deeper scanning stopped after 25,000 entries. Unscanned names may remain readable.";
        partial.coverage = format!("{gap}\n{}", crate::task_filter::COVERAGE);
        editor.apply_task_preview(generation, Ok(partial));
        for (width, height) in [(140, 40), (70, 30), (60, 24)] {
            let text = draw(&editor, width, height);
            let readable = text
                .replace('│', " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            assert!(readable.contains(gap), "missing coverage gap: {text}");
            assert!(text.contains("Send and run"), "missing Send: {text}");
            assert!(text.contains("Edit task"), "missing cancel: {text}");
            assert!(!text.contains("Preview unavailable"));
        }
        key(&mut editor, KeyCode::Enter);
        assert!(matches!(
            editor.take_action(),
            Some(Action::SendTask {
                remember: false,
                ..
            })
        ));
    }

    #[test]
    fn task_filter_preview_inspection_refresh_retains_pane_and_reveals_new_request() {
        let mut editor = Editor::task_inspection(
            "/w".into(),
            Default::default(),
            preview(&"old request\n".repeat(4_000)),
        );
        draw(&editor, 60, 24);
        key(&mut editor, KeyCode::End);
        draw(&editor, 60, 24);
        assert!(editor.window.get() > 0);
        editor.refresh_task_inspection(2, preview("SECOND LIVE REQUEST"));
        assert_eq!(editor.task_preview.as_ref().unwrap().view, 2);
        assert_eq!(editor.scroll.get(), 0);
        assert_eq!(editor.window.get(), 0);
        assert!(draw(&editor, 60, 24).contains("SECOND LIVE REQUEST"));
        editor.refresh_task_inspection(2, preview("SAME GENERATION IGNORED"));
        assert!(!draw(&editor, 60, 24).contains("SAME GENERATION IGNORED"));
        editor.refresh_task_inspection(3, preview("THIRD LIVE REQUEST"));
        assert!(draw(&editor, 60, 24).contains("THIRD LIVE REQUEST"));
        assert!(editor.take_action().is_none());
    }

    #[test]
    fn task_filter_preview_renders_controls_and_second_pending_update_with_long_history() {
        let mut editor = task_editor("Review malware");
        let first = editor.task_preview_generation().unwrap();
        editor.take_action();
        let mut first_preview = preview(&format!(
            "{}FIRST PENDING UPDATE",
            "long history line\n".repeat(4_000)
        ));
        first_preview.coverage = crate::task_filter::COVERAGE.into();
        editor.apply_task_preview(first, Ok(first_preview));
        for (width, height) in [(140, 40), (70, 30), (60, 24)] {
            let text = draw(&editor, width, height);
            for expected in [
                "Private values",
                "Provider copy",
                "Remember for this project",
                "Send and run",
                "Edit task",
            ] {
                assert!(text.contains(expected), "missing {expected}: {text}");
            }
            let readable = text
                .replace('│', " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                readable.contains(crate::task_filter::COVERAGE),
                "coverage or its limits were clipped: {text}"
            );
            key(&mut editor, KeyCode::End);
            assert!(draw(&editor, width, height).contains("FIRST PENDING UPDATE"));
        }
        key(&mut editor, KeyCode::Char('p'));
        let second = editor.task_preview_generation().unwrap();
        assert!(draw(&editor, 60, 24).contains("Updating preview"));
        editor.apply_task_preview(
            second,
            Ok(preview(&format!(
                "{}SECOND PENDING UPDATE",
                "long history line\n".repeat(4_000)
            ))),
        );
        key(&mut editor, KeyCode::End);
        let text = draw(&editor, 60, 24);
        assert!(text.contains("SECOND PENDING UPDATE"));
        assert!(!text.contains("FIRST PENDING UPDATE"));
    }

    #[test]
    fn task_filter_guide_breakdown_is_explicit_and_does_not_apply_selected_terms() {
        let task = "1. Inspect parser\n2. Update docs";
        let mut editor = Editor::guided("/w".into(), task, task).unwrap();
        assert!(draw(&editor, 90, 25).contains("B      Break into focused tasks"));
        key(&mut editor, KeyCode::Char('b'));
        key(&mut editor, KeyCode::Char('b'));
        assert!(matches!(editor.take_action(), Some(Action::BreakdownTask)));
        assert!(editor.take_action().is_none());
    }

    #[test]
    fn task_filter_guide_only_matches_present_terms_and_keeps_spelling() {
        assert!(Editor::guided("/w".into(), "Repair the menu", "Repair the menu").is_none());
        let mut editor = Editor::guided(
            "/w".into(),
            "Review MALWARE at https://example.test @notes.txt",
            "Review MALWARE malware at https://example.test @notes.txt\nA reverse shell payload",
        )
        .unwrap();
        assert!(
            editor.take_action().is_none(),
            "no provider or snapshot action"
        );
        key(&mut editor, KeyCode::Down);
        key(&mut editor, KeyCode::Char(' '));
        key(&mut editor, KeyCode::Enter);
        let Some(Action::ContinueTask {
            words, hide_urls, ..
        }) = editor.take_action()
        else {
            panic!("continues with selected terms")
        };
        assert_eq!(words, ["reverse shell", "payload"]);
        assert!(hide_urls);
        key(&mut editor, KeyCode::Enter);
        assert!(editor.take_action().is_none(), "no duplicate continuation");
    }

    #[test]
    fn task_filter_guide_plain_file_can_continue_unchanged_or_restore_task() {
        let mut editor = Editor::guided("/w".into(), "Read @notes.txt", "plain notes").unwrap();
        key(&mut editor, KeyCode::Char('u'));
        assert!(matches!(
            editor.take_action(),
            Some(Action::ContinueTask { words, hide_urls: false, .. }) if words.is_empty()
        ));
        editor.apply_guide_failed("Could not apply. Try again.".into());
        key(&mut editor, KeyCode::Esc);
        assert!(matches!(editor.take_action(), Some(Action::Close)));
    }

    #[test]
    fn task_filter_guide_can_add_and_edit_unicode_terms() {
        let mut editor = Editor::guided("/w".into(), "Read @notes.txt", "plain notes").unwrap();
        key(&mut editor, KeyCode::Char('a'));
        editor.handle(&Event::Paste("私有 café".into()));
        key(&mut editor, KeyCode::Enter);
        key(&mut editor, KeyCode::Char('e'));
        key(&mut editor, KeyCode::Backspace);
        key(&mut editor, KeyCode::Char('e'));
        key(&mut editor, KeyCode::Enter);
        key(&mut editor, KeyCode::Enter);
        assert!(matches!(
            editor.take_action(),
            Some(Action::ContinueTask { words, .. }) if words == ["私有 cafe"]
        ));
    }

    #[test]
    fn task_filter_guide_renders_actions_and_scope_at_wide_and_narrow_widths() {
        let mut editor = Editor::guided(
            "/w".into(),
            "Review malware https://example.test",
            "Review malware https://example.test",
        )
        .unwrap();
        for (width, height) in [(140, 40), (60, 24)] {
            let text = draw(&editor, width, height);
            for expected in [
                "Review task and filters before sending",
                "this CA session",
                "not known provider-banned terms",
                "Breakdown keeps existing filters and shared purpose",
                "[x] Hide URL hosts (keep paths)",
                "[x] malware",
                "Apply selected & continue",
                "Continue unchanged",
                "Edit task",
            ] {
                assert!(text.contains(expected), "missing {expected}: {text}");
            }
            assert!(!text.contains("Save for next launch"));
        }
        editor.apply_guide_failed("Apply failed; selections kept. Try again.".into());
        assert!(draw(&editor, 60, 24).contains("Apply failed; selections kept"));
    }

    #[test]
    fn task_filter_guide_history_labels_have_counts_and_typed_exact_selections() {
        let input = GuideInput {
            task: "Continue".into(),
            expanded: "Continue".into(),
            history: vec!["old context ".repeat(30_000), "EPT_WALK ept VMXNET3".into()],
            imported: None,
        };
        let mut editor = Editor::preparing("/w".into(), Some(input));
        assert!(editor.guide_analyzing());
        assert!(draw(&editor, 60, 24).contains("Checking the task"));
        key(&mut editor, KeyCode::Enter);
        let input = editor
            .take_guide_analysis()
            .expect("analysis remains queued, no premature continue");
        editor.apply_guide_analysis(Ok(input.analyze()));
        let text = draw(&editor, 60, 24);
        assert!(
            text.contains("EPT · alias · 2 hits (2 in history)"),
            "{text}"
        );
        assert!(text.contains("VMXNET3 · alias"), "{text}");
        key(&mut editor, KeyCode::Enter);
        assert!(
            matches!(editor.take_action(), Some(Action::ContinueTask { words, identifiers, .. })
            if words.is_empty() && identifiers == ["EPT", "ept", "VMXNET3"])
        );
    }

    #[test]
    fn task_filter_guide_worker_no_candidates_resumes_and_cancel_clears_analysis() {
        let mut editor = Editor::preparing(
            "/w".into(),
            Some(GuideInput {
                task: "Continue".into(),
                expanded: "Continue".into(),
                history: vec!["ordinary text".into()],
                imported: None,
            }),
        );
        let input = editor.take_guide_analysis().unwrap();
        editor.apply_guide_analysis(Ok(input.analyze()));
        assert!(
            matches!(editor.take_action(), Some(Action::ContinueTask { words, identifiers, hide_urls: false }) if words.is_empty() && identifiers.is_empty())
        );
        let mut editor = Editor::preparing("/w".into(), Some(input));
        key(&mut editor, KeyCode::Esc);
        assert!(!editor.guide_analyzing());
        assert!(matches!(editor.take_action(), Some(Action::Close)));
        assert!(editor.take_guide_analysis().is_none());
    }
}
