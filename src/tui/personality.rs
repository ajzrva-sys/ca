//! Interactive, provider-free personality setup. The owner performs all I/O;
//! this editor only keeps a draft and emits an explicit revision-bound save.

use crate::personality_preferences::{self, Answer, Preferences, Snapshot, QUESTIONS};
use crate::tui::Event;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap};
use std::cell::Cell;
use std::collections::BTreeMap;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const MENU: [&str; 6] = [
    "Quick setup",
    "Full walkthrough",
    "Explore by section",
    "Review and save",
    "Additional instructions",
    "Close questionnaire",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Close,
    Save {
        revision: String,
        preferences: Preferences,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Busy {
    Loading,
    Saving,
}

#[derive(Debug, Clone)]
struct Walkthrough {
    questions: Vec<usize>,
    position: usize,
    selected: usize,
    label: String,
}

#[derive(Debug, Clone)]
enum TextTarget {
    Answer { question: usize, walk: Walkthrough },
    Additional,
}

#[derive(Debug, Clone)]
struct TextEditor {
    target: TextTarget,
    text: String,
    cursor: usize,
}

#[derive(Debug, Clone)]
enum Screen {
    Overview(usize),
    Sections(usize),
    Question(Walkthrough),
    Text(TextEditor),
    Review(u16),
    Discard(bool),
}

#[derive(Debug, Clone)]
pub struct Editor {
    profile_label: String,
    snapshot: Option<Snapshot>,
    preferences: Preferences,
    screen: Screen,
    review_walk: Option<Walkthrough>,
    busy: Option<Busy>,
    error: Option<String>,
    notice: Option<String>,
    review_max_scroll: Cell<u16>,
}

impl Editor {
    pub fn loading(profile_label: String) -> Self {
        Self {
            profile_label,
            snapshot: None,
            preferences: Preferences {
                version: 1,
                answers: BTreeMap::new(),
                additional: String::new(),
            },
            screen: Screen::Overview(0),
            review_walk: None,
            busy: Some(Busy::Loading),
            error: None,
            notice: None,
            review_max_scroll: Cell::new(0),
        }
    }

    pub fn from_snapshot(profile_label: String, snapshot: Snapshot) -> Self {
        let mut editor = Self::loading(profile_label);
        editor.preferences = snapshot.preferences.clone();
        editor.snapshot = Some(snapshot);
        editor.busy = None;
        editor
    }

    pub fn load_completed(&mut self, result: Result<Snapshot, String>) {
        self.busy = None;
        match result {
            Ok(snapshot) => {
                *self = Self::from_snapshot(self.profile_label.clone(), snapshot);
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn save_completed(&mut self, result: Result<Snapshot, String>) {
        self.busy = None;
        match result {
            Ok(snapshot) => {
                self.preferences = snapshot.preferences.clone();
                self.snapshot = Some(snapshot);
                self.error = None;
                self.notice = Some("Preferences saved. New turns use your choices.".to_owned());
            }
            Err(error) => {
                self.error = Some(error);
                self.notice = None;
            }
        }
        self.screen = Screen::Review(0);
    }

    pub fn is_busy(&self) -> bool {
        self.busy.is_some()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn profile_label(&self) -> &str {
        &self.profile_label
    }

    fn dirty(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.preferences != self.preferences)
    }

    fn answered(&self) -> usize {
        QUESTIONS
            .iter()
            .filter(|question| self.preferences.answers.contains_key(question.id))
            .count()
    }

    fn walkthrough_scope(&self) -> Option<&Walkthrough> {
        match &self.screen {
            Screen::Question(walk) => Some(walk),
            Screen::Text(TextEditor {
                target: TextTarget::Answer { walk, .. },
                ..
            }) => Some(walk),
            Screen::Review(_) => self.review_walk.as_ref(),
            _ => None,
        }
    }

    fn request_close(&mut self) -> Action {
        if self.dirty() {
            self.screen = Screen::Discard(false);
            Action::None
        } else {
            Action::Close
        }
    }

    fn sections() -> Vec<&'static str> {
        let mut sections = Vec::new();
        for question in QUESTIONS {
            if !sections.contains(&question.section) {
                sections.push(question.section);
            }
        }
        sections
    }

    fn selected_answer(&self, question: usize) -> usize {
        let question = &QUESTIONS[question];
        match self.preferences.answers.get(question.id) {
            Some(Answer::Choice(id)) => question
                .choices
                .iter()
                .position(|choice| choice.id == id)
                .unwrap_or(question.choices.len() + 1),
            Some(Answer::Custom(_)) => question.choices.len(),
            None => question.choices.len() + 1,
        }
    }

    fn begin_walk(&mut self, questions: Vec<usize>, label: String) {
        if questions.is_empty() {
            self.screen = Screen::Overview(0);
            return;
        }
        // Resume at the first unanswered question, preserving previous answers.
        let position = questions
            .iter()
            .position(|&index| !self.preferences.answers.contains_key(QUESTIONS[index].id))
            .unwrap_or(0);
        let selected = self.selected_answer(questions[position]);
        let walk = Walkthrough {
            questions,
            position,
            selected,
            label,
        };
        self.review_walk = Some(walk.clone());
        self.screen = Screen::Question(walk);
        self.notice = None;
    }

    fn advance(&mut self, mut walk: Walkthrough) {
        if walk.position + 1 < walk.questions.len() {
            walk.position += 1;
            walk.selected = self.selected_answer(walk.questions[walk.position]);
            self.screen = Screen::Question(walk);
        } else {
            self.notice = Some(if walk.label == "Quick setup" {
                format!(
                    "Quick setup complete. The other {} questions are optional.",
                    QUESTIONS.len() - walk.questions.len()
                )
            } else {
                format!(
                    "{} complete. Review your choices before saving.",
                    walk.label
                )
            });
            self.screen = Screen::Review(0);
        }
    }

    fn previous(&mut self, mut walk: Walkthrough) {
        if walk.position == 0 {
            self.screen = Screen::Overview(0);
        } else {
            walk.position -= 1;
            walk.selected = self.selected_answer(walk.questions[walk.position]);
            self.screen = Screen::Question(walk);
        }
    }

    fn accept_answer(&mut self, walk: Walkthrough) {
        let index = walk.questions[walk.position];
        let question = &QUESTIONS[index];
        if let Some(choice) = question.choices.get(walk.selected) {
            self.preferences
                .answers
                .insert(question.id.to_owned(), Answer::Choice(choice.id.to_owned()));
            self.advance(walk);
        } else if walk.selected == question.choices.len() {
            let text = match self.preferences.answers.get(question.id) {
                Some(Answer::Custom(text)) => text.clone(),
                _ => String::new(),
            };
            let cursor = text.len();
            self.screen = Screen::Text(TextEditor {
                target: TextTarget::Answer {
                    question: index,
                    walk,
                },
                text,
                cursor,
            });
        } else {
            self.preferences.answers.remove(question.id);
            self.advance(walk);
        }
    }

    fn save(&mut self) -> Action {
        let Some(snapshot) = self.snapshot.as_ref() else {
            self.error = Some("Preferences could not be loaded. Close and try again.".to_owned());
            return Action::None;
        };
        if let Err(error) = personality_preferences::preview(snapshot, &self.preferences) {
            self.error = Some(error.to_string());
            return Action::None;
        }
        self.busy = Some(Busy::Saving);
        self.error = None;
        self.notice = None;
        Action::Save {
            revision: snapshot.revision.clone(),
            preferences: self.preferences.clone(),
        }
    }

    pub fn handle(&mut self, event: &Event) -> Action {
        // A save is already authorized and in flight. Closing cannot undo it.
        if self.busy == Some(Busy::Saving) {
            return Action::None;
        }
        if matches!(event, Event::Key(KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL))
        {
            return Action::Close;
        }
        if self.busy == Some(Busy::Loading) {
            return if matches!(event, Event::Key(KeyCode::Esc, _)) {
                Action::Close
            } else {
                Action::None
            };
        }
        if let Event::Paste(text) = event {
            if let Screen::Text(editor) = &mut self.screen {
                if !insert_text(editor, text) {
                    self.error = Some(text_limit_message(editor));
                } else {
                    self.error = None;
                }
            }
            return Action::None;
        }
        if let Event::MouseScroll(event) = event {
            if let Screen::Review(scroll) = &mut self.screen {
                *scroll = (*scroll).min(self.review_max_scroll.get());
                match event.direction {
                    crate::tui::event::ScrollDirection::Up => *scroll = scroll.saturating_sub(3),
                    crate::tui::event::ScrollDirection::Down => {
                        *scroll = scroll.saturating_add(3).min(self.review_max_scroll.get())
                    }
                }
            }
            return Action::None;
        }
        let Event::Key(key, modifiers) = event else {
            return Action::None;
        };
        let key = *key;
        let control = modifiers.contains(KeyModifiers::CONTROL);
        if control && key == KeyCode::Char('s') && !matches!(self.screen, Screen::Text(_)) {
            if !matches!(self.screen, Screen::Review(_)) {
                self.notice = Some("Review and save whenever you are ready.".to_owned());
            }
            self.screen = Screen::Review(0);
            self.error = None;
            return Action::None;
        }
        let screen = self.screen.clone();
        match screen {
            Screen::Overview(mut selected) => {
                match key {
                    KeyCode::Up => selected = selected.saturating_sub(1),
                    KeyCode::Down => selected = (selected + 1).min(MENU.len() - 1),
                    KeyCode::Char(number @ '1'..='6') => {
                        selected = number as usize - '1' as usize;
                    }
                    KeyCode::Esc => return self.request_close(),
                    KeyCode::Enter => {
                        match selected {
                            0 => self.begin_walk(
                                QUESTIONS
                                    .iter()
                                    .enumerate()
                                    .filter_map(|(index, question)| question.quick.then_some(index))
                                    .collect(),
                                "Quick setup".to_owned(),
                            ),
                            1 => self.begin_walk(
                                (0..QUESTIONS.len()).collect(),
                                "Full walkthrough".to_owned(),
                            ),
                            2 => self.screen = Screen::Sections(0),
                            3 => self.screen = Screen::Review(0),
                            4 => {
                                let text = self.preferences.additional.clone();
                                let cursor = text.len();
                                self.screen = Screen::Text(TextEditor {
                                    target: TextTarget::Additional,
                                    text,
                                    cursor,
                                });
                            }
                            _ => return self.request_close(),
                        }
                        return Action::None;
                    }
                    _ => {}
                }
                self.screen = Screen::Overview(selected);
            }
            Screen::Sections(mut selected) => {
                let sections = Self::sections();
                match key {
                    KeyCode::Up => selected = selected.saturating_sub(1),
                    KeyCode::Down => {
                        selected = (selected + 1).min(sections.len().saturating_sub(1))
                    }
                    KeyCode::Esc | KeyCode::Left | KeyCode::Backspace => {
                        self.screen = Screen::Overview(2);
                        return Action::None;
                    }
                    KeyCode::Enter => {
                        if let Some(&section) = sections.get(selected) {
                            self.begin_walk(
                                QUESTIONS
                                    .iter()
                                    .enumerate()
                                    .filter_map(|(index, question)| {
                                        (question.section == section).then_some(index)
                                    })
                                    .collect(),
                                section.to_owned(),
                            );
                        }
                        return Action::None;
                    }
                    _ => {}
                }
                self.screen = Screen::Sections(selected);
            }
            Screen::Question(mut walk) => {
                let choices = QUESTIONS[walk.questions[walk.position]].choices.len() + 2;
                match key {
                    KeyCode::Up => walk.selected = walk.selected.saturating_sub(1),
                    KeyCode::Down => walk.selected = (walk.selected + 1).min(choices - 1),
                    KeyCode::Char(number @ '1'..='9') => {
                        let selected = number as usize - '1' as usize;
                        if selected < choices {
                            walk.selected = selected;
                        }
                    }
                    KeyCode::Enter | KeyCode::Right => {
                        self.accept_answer(walk);
                        return Action::None;
                    }
                    KeyCode::Left | KeyCode::Backspace => {
                        self.previous(walk);
                        return Action::None;
                    }
                    KeyCode::Esc => {
                        self.screen = Screen::Overview(0);
                        return Action::None;
                    }
                    _ => {}
                }
                self.screen = Screen::Question(walk);
            }
            Screen::Text(mut editor) => {
                if key == KeyCode::Esc {
                    self.screen = match editor.target {
                        TextTarget::Answer { walk, .. } => Screen::Question(walk),
                        TextTarget::Additional => Screen::Overview(4),
                    };
                    self.error = None;
                    return Action::None;
                }
                if control && key == KeyCode::Char('s') {
                    match editor.target {
                        TextTarget::Answer { question, walk } => {
                            let text = editor.text.trim().to_owned();
                            if text.is_empty() {
                                self.preferences.answers.remove(QUESTIONS[question].id);
                            } else {
                                self.preferences.answers.insert(
                                    QUESTIONS[question].id.to_owned(),
                                    Answer::Custom(text),
                                );
                            }
                            self.advance(walk);
                        }
                        TextTarget::Additional => {
                            self.preferences.additional = editor.text.trim().to_owned();
                            self.screen = Screen::Overview(4);
                        }
                    }
                    self.error = None;
                    return Action::None;
                }
                if !edit_text(&mut editor, key, *modifiers) {
                    self.error = Some(text_limit_message(&editor));
                } else {
                    self.error = None;
                }
                self.screen = Screen::Text(editor);
            }
            Screen::Review(mut scroll) => {
                scroll = scroll.min(self.review_max_scroll.get());
                match key {
                    KeyCode::Up => scroll = scroll.saturating_sub(1),
                    KeyCode::Down => {
                        scroll = scroll.saturating_add(1).min(self.review_max_scroll.get())
                    }
                    KeyCode::PageUp => scroll = scroll.saturating_sub(10),
                    KeyCode::PageDown => {
                        scroll = scroll.saturating_add(10).min(self.review_max_scroll.get())
                    }
                    KeyCode::Home => scroll = 0,
                    KeyCode::End => scroll = self.review_max_scroll.get(),
                    KeyCode::Esc | KeyCode::Left | KeyCode::Backspace => {
                        self.screen = Screen::Overview(3);
                        return Action::None;
                    }
                    KeyCode::Enter => return self.save(),
                    _ => {}
                }
                self.screen = Screen::Review(scroll);
            }
            Screen::Discard(mut discard) => {
                match key {
                    KeyCode::Up | KeyCode::Left | KeyCode::Char('1') => discard = false,
                    KeyCode::Down | KeyCode::Right | KeyCode::Char('2') => discard = true,
                    KeyCode::Esc => {
                        self.screen = Screen::Overview(5);
                        return Action::None;
                    }
                    KeyCode::Enter => {
                        if discard {
                            return Action::Close;
                        }
                        self.screen = Screen::Overview(0);
                        return Action::None;
                    }
                    _ => {}
                }
                self.screen = Screen::Discard(discard);
            }
        }
        Action::None
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

fn insert_text(editor: &mut TextEditor, text: &str) -> bool {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let text: String = normalized
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .collect();
    if editor
        .text
        .chars()
        .count()
        .saturating_add(text.chars().count())
        > text_limit(editor)
    {
        return false;
    }
    editor.text.insert_str(editor.cursor, &text);
    editor.cursor += text.len();
    true
}

fn text_limit(editor: &TextEditor) -> usize {
    match editor.target {
        TextTarget::Answer { .. } => personality_preferences::MAX_CUSTOM_CHARS,
        TextTarget::Additional => personality_preferences::MAX_ADDITIONAL_CHARS,
    }
}

fn text_limit_message(editor: &TextEditor) -> String {
    format!(
        "This answer is too long. Keep it under {} characters.",
        text_limit(editor)
    )
}

fn move_line(editor: &mut TextEditor, down: bool) {
    let prefix = &editor.text[..editor.cursor];
    let start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let column = UnicodeWidthStr::width(&editor.text[start..editor.cursor]);
    let target = if down {
        let Some(end) = editor.text[editor.cursor..].find('\n') else {
            return;
        };
        editor.cursor + end + 1
    } else if start > 0 {
        editor.text[..start - 1]
            .rfind('\n')
            .map_or(0, |index| index + 1)
    } else {
        return;
    };
    let end = editor.text[target..]
        .find('\n')
        .map_or(editor.text.len(), |index| target + index);
    let mut width = 0;
    editor.cursor = target;
    for grapheme in editor.text[target..end].graphemes(true) {
        let next_width = width + UnicodeWidthStr::width(grapheme);
        if next_width > column {
            break;
        }
        width = next_width;
        editor.cursor += grapheme.len();
    }
}

fn edit_text(editor: &mut TextEditor, key: KeyCode, modifiers: KeyModifiers) -> bool {
    let control = modifiers.contains(KeyModifiers::CONTROL);
    match key {
        KeyCode::Left => editor.cursor = previous_grapheme(&editor.text, editor.cursor),
        KeyCode::Right => editor.cursor = next_grapheme(&editor.text, editor.cursor),
        KeyCode::Up => move_line(editor, false),
        KeyCode::Down => move_line(editor, true),
        KeyCode::Home => {
            editor.cursor = if control {
                0
            } else {
                editor.text[..editor.cursor]
                    .rfind('\n')
                    .map_or(0, |index| index + 1)
            };
        }
        KeyCode::End => {
            editor.cursor = if control {
                editor.text.len()
            } else {
                editor.text[editor.cursor..]
                    .find('\n')
                    .map_or(editor.text.len(), |index| editor.cursor + index)
            };
        }
        KeyCode::Backspace => {
            let previous = previous_grapheme(&editor.text, editor.cursor);
            editor.text.replace_range(previous..editor.cursor, "");
            editor.cursor = previous;
        }
        KeyCode::Delete => {
            let next = next_grapheme(&editor.text, editor.cursor);
            editor.text.replace_range(editor.cursor..next, "");
        }
        KeyCode::Enter => return insert_text(editor, "\n"),
        KeyCode::Tab => return insert_text(editor, "    "),
        KeyCode::Char(character)
            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            return insert_text(editor, &character.to_string());
        }
        _ => {}
    }
    true
}

fn accent() -> Style {
    Style::default().fg(Color::Cyan)
}

fn muted() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn selected_style(selected: bool) -> Style {
    if selected {
        accent().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn paragraph(frame: &mut ratatui::Frame, area: Rect, lines: Vec<Line<'_>>) {
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn clipped(text: &str, width: u16) -> String {
    crate::tui::selection::truncate_cells(text, usize::from(width))
}

fn footer(editor: &Editor) -> &'static str {
    match editor.busy {
        Some(Busy::Saving) => "Saving your preferences…",
        Some(Busy::Loading) => "Esc or Ctrl-C Close",
        None => match editor.screen {
            Screen::Overview(_) => "↑↓ Choose · Enter Open · Esc Close · Ctrl-S Review",
            Screen::Sections(_) => "↑↓ Choose section · Enter Open · Esc Menu",
            Screen::Question(_) => "↑↓ Choose · Enter Next · ← Back · Esc Menu · Ctrl-S Review",
            Screen::Text(_) => "Ctrl-S Use answer · Enter New line · Esc Cancel edit",
            Screen::Review(_) => "↑↓ Scroll · Enter Save · Esc Menu",
            Screen::Discard(_) => "↑↓ Choose · Enter Confirm · Esc Keep editing",
        },
    }
}

pub fn render(frame: &mut ratatui::Frame, area: Rect, editor: &Editor) {
    render_with_version_label(frame, area, editor, super::VERSION_LABEL);
}

pub(super) fn render_with_version_label(
    frame: &mut ratatui::Frame,
    area: Rect,
    editor: &Editor,
    version_label: &str,
) {
    frame.render_widget(Clear, area);
    let title = if UnicodeWidthStr::width(version_label) + 3 > usize::from(area.width) {
        version_label.to_owned()
    } else {
        format!(" {version_label} · CA personality ")
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(accent());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 35 || inner.height < 10 {
        paragraph(
            frame,
            inner,
            vec![
                Line::from("Make the terminal wider to set up CA."),
                Line::from("Your draft is preserved. Ctrl-C closes."),
            ],
        );
        return;
    }
    let message_height = if editor.error().is_some() {
        3
    } else if editor.notice.is_some() {
        2
    } else {
        0
    };
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(message_height),
            Constraint::Length(2),
        ])
        .split(inner);
    let status = if editor.dirty() {
        " · Unsaved changes"
    } else {
        ""
    };
    let progress = if let Some(walk) = editor.walkthrough_scope() {
        let answered = walk
            .questions
            .iter()
            .filter(|&&index| editor.preferences.answers.contains_key(QUESTIONS[index].id))
            .count();
        format!(
            " · {} · {answered}/{} answered",
            walk.label,
            walk.questions.len()
        )
    } else {
        format!(" · {} preferences answered", editor.answered())
    };
    paragraph(
        frame,
        areas[0],
        vec![Line::from(vec![
            Span::styled(format!("{} profile", editor.profile_label()), accent()),
            Span::raw(format!("{progress}{status}")),
        ])],
    );
    if let Some(error) = editor.error() {
        paragraph(
            frame,
            areas[2],
            vec![Line::styled(error, Style::default().fg(Color::Red))],
        );
    } else if let Some(notice) = editor.notice.as_ref() {
        paragraph(frame, areas[2], vec![Line::styled(notice, accent())]);
    }
    paragraph(
        frame,
        areas[3],
        vec![
            Line::styled(footer(editor), muted()),
            Line::styled(
                "Save at any point. Unanswered questions are optional.",
                muted(),
            ),
        ],
    );
    if editor.is_busy() {
        let busy = editor.busy.expect("busy editor has a busy state");
        paragraph(
            frame,
            areas[1],
            vec![Line::from(match busy {
                Busy::Loading => "Loading your preferences…",
                Busy::Saving => "Saving your preferences… Please wait.",
            })],
        );
        return;
    }
    match &editor.screen {
        Screen::Overview(selected) => render_overview(frame, areas[1], editor, *selected),
        Screen::Sections(selected) => render_sections(frame, areas[1], editor, *selected),
        Screen::Question(walk) => render_question(frame, areas[1], editor, walk),
        Screen::Text(text) => render_text(frame, areas[1], text),
        Screen::Review(scroll) => render_review(frame, areas[1], editor, *scroll),
        Screen::Discard(discard) => paragraph(
            frame,
            areas[1],
            vec![
                Line::styled(
                    "Leave without saving?",
                    accent().add_modifier(Modifier::BOLD),
                ),
                Line::from("Your saved preferences will stay as they are."),
                Line::from(""),
                Line::styled(
                    if !discard {
                        "› 1 Keep editing"
                    } else {
                        "  1 Keep editing"
                    },
                    selected_style(!discard),
                ),
                Line::styled(
                    if *discard {
                        "› 2 Discard changes"
                    } else {
                        "  2 Discard changes"
                    },
                    selected_style(*discard),
                ),
            ],
        ),
    }
}

fn render_overview(frame: &mut ratatui::Frame, area: Rect, editor: &Editor, selected: usize) {
    let quick = QUESTIONS.iter().filter(|question| question.quick).count();
    let descriptions = [
        format!("Start with {quick} questions. Resume where you left off."),
        format!(
            "Explore all {} questions at your own pace.",
            QUESTIONS.len()
        ),
        "Choose a behavior area and change just that section.".to_owned(),
        "Read the exact preferences CA will use, then save.".to_owned(),
        "Add your own instructions in this screen.".to_owned(),
        "Return to CA. You can save or discard your draft.".to_owned(),
    ];
    let mut lines = vec![
        Line::styled(
            "How would you like CA to work with you?",
            accent().add_modifier(Modifier::BOLD),
        ),
        Line::from(
            if editor
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| !snapshot.preserved_markdown.trim().is_empty())
            {
                "Existing instructions are in Additional instructions."
            } else {
                "Choose answers, write your own, or leave questions open."
            },
        ),
        Line::from(""),
    ];
    for (index, label) in MENU.iter().enumerate() {
        let suffix = match index {
            0 => format!(" · {quick} questions"),
            1 => format!(" · {} questions", QUESTIONS.len()),
            2 => format!(" · {} areas", Editor::sections().len()),
            _ => String::new(),
        };
        lines.push(Line::styled(
            clipped(
                &format!(
                    "{} {} {label}{suffix}",
                    if index == selected { "›" } else { " " },
                    index + 1
                ),
                area.width,
            ),
            selected_style(index == selected),
        ));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(&descriptions[selected], muted()));
    if !editor.preferences.additional.is_empty() && area.height > 14 {
        lines.push(Line::from(
            "Your additional instructions are included in the review.",
        ));
    }
    paragraph(frame, area, lines);
}

fn render_sections(frame: &mut ratatui::Frame, area: Rect, editor: &Editor, selected: usize) {
    let sections = Editor::sections();
    let capacity = usize::from(area.height.saturating_sub(2)).max(1);
    let start = selected.saturating_sub(capacity - 1);
    let mut lines = vec![
        Line::styled("Explore by section", accent().add_modifier(Modifier::BOLD)),
        Line::from(""),
    ];
    for (index, section) in sections.iter().enumerate().skip(start).take(capacity) {
        let questions: Vec<_> = QUESTIONS
            .iter()
            .filter(|question| question.section == *section)
            .collect();
        let answered = questions
            .iter()
            .filter(|question| editor.preferences.answers.contains_key(question.id))
            .count();
        lines.push(Line::styled(
            clipped(
                &format!(
                    "{} {section} · {answered}/{} answered",
                    if index == selected { "›" } else { " " },
                    questions.len()
                ),
                area.width,
            ),
            selected_style(index == selected),
        ));
    }
    paragraph(frame, area, lines);
}

fn render_question(frame: &mut ratatui::Frame, area: Rect, editor: &Editor, walk: &Walkthrough) {
    let question = &QUESTIONS[walk.questions[walk.position]];
    let title = format!("{} · {}", question.section, question.title);
    let title_height = Paragraph::new(title.as_str())
        .wrap(Wrap { trim: false })
        .line_count(area.width.max(1)) as u16;
    let prompt_height = Paragraph::new(question.prompt)
        .wrap(Wrap { trim: false })
        .line_count(area.width.max(1)) as u16;
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(title_height),
            Constraint::Length(prompt_height),
            Constraint::Length(question.choices.len() as u16 + 2),
            Constraint::Min(1),
        ])
        .split(area);
    frame.render_widget(
        Gauge::default()
            .gauge_style(accent())
            .ratio((walk.position + 1) as f64 / walk.questions.len() as f64)
            .label(format!(
                "{} · {} of {}",
                walk.label,
                walk.position + 1,
                walk.questions.len()
            )),
        areas[0],
    );
    paragraph(
        frame,
        areas[1],
        vec![Line::styled(title, accent().add_modifier(Modifier::BOLD))],
    );
    paragraph(frame, areas[2], vec![Line::from(question.prompt)]);
    let mut lines = Vec::new();
    for (index, choice) in question.choices.iter().enumerate() {
        let current = matches!(editor.preferences.answers.get(question.id), Some(Answer::Choice(id)) if id == choice.id);
        lines.push(Line::styled(
            clipped(
                &format!(
                    "{} {} {}{}",
                    if index == walk.selected { "›" } else { " " },
                    index + 1,
                    choice.label,
                    if current { " (current)" } else { "" }
                ),
                area.width,
            ),
            selected_style(index == walk.selected),
        ));
    }
    let custom = question.choices.len();
    let custom_current = matches!(
        editor.preferences.answers.get(question.id),
        Some(Answer::Custom(_))
    );
    lines.push(Line::styled(
        format!(
            "{} {} Custom answer{}",
            if walk.selected == custom { "›" } else { " " },
            custom + 1,
            if custom_current { " (current)" } else { "" }
        ),
        selected_style(walk.selected == custom),
    ));
    lines.push(Line::styled(
        format!(
            "{} {} Leave unanswered",
            if walk.selected == custom + 1 {
                "›"
            } else {
                " "
            },
            custom + 2
        ),
        selected_style(walk.selected == custom + 1),
    ));
    paragraph(frame, areas[3], lines);
    let detail = question.choices.get(walk.selected).map_or_else(
        || {
            if walk.selected == custom {
                "Write the preference in your own words. Ctrl-S accepts it."
            } else {
                "Leave this open. CA keeps its usual behavior for this question."
            }
        },
        |choice| choice.detail,
    );
    let mut explanation = vec![Line::styled(detail, accent())];
    if areas[4].height >= 4 {
        explanation.push(Line::styled(question.help, muted()));
    }
    paragraph(frame, areas[4], explanation);
}

/// Hard wrapping makes the cursor agree with the rendered text, including
/// combining characters and wide Unicode graphemes.
fn wrapped_text(text: &str, width: usize, cursor: usize) -> (Vec<String>, usize, usize) {
    let width = width.max(1);
    let mut rows = vec![String::new()];
    let mut column = 0;
    let mut cursor_row = 0;
    let mut cursor_column = 0;
    for (index, grapheme) in text.grapheme_indices(true) {
        let grapheme_width = UnicodeWidthStr::width(grapheme).min(width);
        if grapheme != "\n" && column + grapheme_width > width {
            rows.push(String::new());
            column = 0;
        }
        if index == cursor {
            cursor_row = rows.len() - 1;
            cursor_column = column;
        }
        if grapheme == "\n" {
            rows.push(String::new());
            column = 0;
        } else {
            rows.last_mut().expect("one text row").push_str(grapheme);
            column += grapheme_width;
        }
    }
    if cursor == text.len() {
        if column == width {
            rows.push(String::new());
            column = 0;
        }
        cursor_row = rows.len() - 1;
        cursor_column = column;
    }
    (rows, cursor_row, cursor_column)
}

fn render_text(frame: &mut ratatui::Frame, area: Rect, editor: &TextEditor) {
    let (title, prompt) = match editor.target {
        TextTarget::Answer { question, .. } => ("Custom answer", QUESTIONS[question].prompt),
        TextTarget::Additional => (
            "Additional instructions",
            "What else should CA know about how you want it to work?",
        ),
    };
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(3),
        ])
        .split(area);
    paragraph(
        frame,
        areas[0],
        vec![Line::styled(
            format!(
                "{title} · {}/{} characters",
                editor.text.chars().count(),
                text_limit(editor)
            ),
            accent().add_modifier(Modifier::BOLD),
        )],
    );
    paragraph(frame, areas[1], vec![Line::from(prompt)]);
    let block = Block::default().borders(Borders::ALL).border_style(muted());
    let text_area = block.inner(areas[2]);
    frame.render_widget(block, areas[2]);
    if text_area.width == 0 || text_area.height == 0 {
        return;
    }
    let (rows, cursor_row, cursor_column) =
        wrapped_text(&editor.text, usize::from(text_area.width), editor.cursor);
    let start = cursor_row.saturating_sub(usize::from(text_area.height) - 1);
    let visible: Vec<_> = rows
        .iter()
        .skip(start)
        .take(usize::from(text_area.height))
        .map(|line| Line::from(line.as_str()))
        .collect();
    frame.render_widget(Paragraph::new(visible), text_area);
    frame.set_cursor_position((
        text_area.x + cursor_column.min(usize::from(text_area.width) - 1) as u16,
        text_area.y + (cursor_row - start) as u16,
    ));
}

fn render_review(frame: &mut ratatui::Frame, area: Rect, editor: &Editor, scroll: u16) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);
    paragraph(
        frame,
        areas[0],
        vec![
            Line::styled("Review and save", accent().add_modifier(Modifier::BOLD)),
            Line::from("Enter saves these preferences. Esc returns to editing."),
        ],
    );
    let text = editor.snapshot.as_ref().map_or_else(
        || "Preferences are unavailable. Close and try again.".to_owned(),
        |snapshot| {
            personality_preferences::preview(snapshot, &editor.preferences)
                .unwrap_or_else(|error| error.to_string())
        },
    );
    let text = if text.trim().is_empty() {
        "No answers yet. CA will keep its usual behavior."
    } else {
        text.as_str()
    };
    let (rows, _, _) = wrapped_text(text, usize::from(areas[1].width), 0);
    let maximum = rows.len().saturating_sub(usize::from(areas[1].height));
    editor
        .review_max_scroll
        .set(maximum.min(usize::from(u16::MAX)) as u16);
    let start = usize::from(scroll).min(maximum);
    let visible: Vec<_> = rows
        .iter()
        .skip(start)
        .take(usize::from(areas[1].height))
        .map(|line| Line::from(line.as_str()))
        .collect();
    frame.render_widget(Paragraph::new(visible), areas[1]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn snapshot() -> Snapshot {
        Snapshot {
            preferences: Preferences {
                version: 1,
                answers: BTreeMap::new(),
                additional: String::new(),
            },
            revision: "initial-revision".to_owned(),
            preserved_markdown: String::new(),
        }
    }

    fn editor() -> Editor {
        Editor::from_snapshot("default".to_owned(), snapshot())
    }

    fn key(editor: &mut Editor, code: KeyCode) -> Action {
        editor.handle(&Event::Key(code, KeyModifiers::NONE))
    }

    fn ctrl(editor: &mut Editor, character: char) -> Action {
        editor.handle(&Event::Key(KeyCode::Char(character), KeyModifiers::CONTROL))
    }

    fn rendered(editor: &Editor, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(frame, frame.area(), editor))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                let mut line = String::new();
                let mut x = 0;
                while x < width {
                    let symbol = buffer[(x, y)].symbol();
                    line.push_str(symbol);
                    // Wide graphemes occupy following buffer cells. Those
                    // cells are padding, not spaces between the characters.
                    x = x.saturating_add(UnicodeWidthStr::width(symbol).max(1) as u16);
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn quick_setup_stops_at_eight_and_marks_remaining_questions_optional() {
        for (width, height) in [(80, 24), (60, 20)] {
            let mut editor = editor();
            key(&mut editor, KeyCode::Char('1'));
            key(&mut editor, KeyCode::Enter);
            assert!(rendered(&editor, width, height).contains("Quick setup · 0/8 answered"));
            for position in 0..8 {
                assert!(matches!(&editor.screen, Screen::Question(walk)
                    if walk.questions.len() == 8 && walk.position == position));
                key(&mut editor, KeyCode::Char('1'));
                key(&mut editor, KeyCode::Enter);
            }
            assert!(matches!(editor.screen, Screen::Review(_)));
            let review = rendered(&editor, width, height);
            assert!(review.contains("Quick setup · 8/8 answered"));
            assert!(review.contains("Quick setup complete."));
            assert!(review.contains("40 questions are optional"));
            assert!(!review.contains("8/48 answered"));
            assert_eq!(editor.preferences.answers.len(), 8);
            assert!(
                matches!(key(&mut editor, KeyCode::Enter), Action::Save { preferences, .. }
                if preferences.answers.len() == 8)
            );
        }
    }

    #[test]
    fn quick_setup_can_save_two_answers_and_resume_at_three() {
        let mut editor = editor();
        key(&mut editor, KeyCode::Enter);
        for _ in 0..2 {
            key(&mut editor, KeyCode::Char('2'));
            key(&mut editor, KeyCode::Enter);
        }
        let question = rendered(&editor, 60, 20);
        assert!(question.contains("Ctrl-S Review"));
        assert!(question.contains("Save at any point."));
        assert!(question.contains("Unanswered questions are optional."));
        ctrl(&mut editor, 's');
        assert!(matches!(editor.screen, Screen::Review(_)));
        assert!(rendered(&editor, 60, 20).contains("Quick setup · 2/8 answered"));
        let Action::Save { preferences, .. } = key(&mut editor, KeyCode::Enter) else {
            panic!("partial preferences must be savable");
        };
        assert_eq!(preferences.answers.len(), 2);
        let mut reopened = Editor::from_snapshot(
            "default".to_owned(),
            Snapshot {
                preferences,
                revision: "partial-save".to_owned(),
                preserved_markdown: String::new(),
            },
        );
        key(&mut reopened, KeyCode::Enter);
        assert!(matches!(&reopened.screen, Screen::Question(walk)
            if walk.questions.len() == 8 && walk.position == 2));
        let resumed = rendered(&reopened, 60, 20);
        assert!(resumed.contains("Quick setup · 3 of 8"));
        assert!(resumed.contains("Quick setup · 2/8 answered"));
    }

    #[test]
    fn answers_resume_back_and_save_only_after_review() {
        let mut editor = editor();
        assert_eq!(key(&mut editor, KeyCode::Enter), Action::None);
        let first = match &editor.screen {
            Screen::Question(walk) => walk.questions[walk.position],
            _ => panic!("walkthrough"),
        };
        key(&mut editor, KeyCode::Char('2'));
        key(&mut editor, KeyCode::Enter);
        assert_eq!(
            editor.preferences.answers.get(QUESTIONS[first].id),
            Some(&Answer::Choice(QUESTIONS[first].choices[1].id.to_owned()))
        );
        key(&mut editor, KeyCode::Left);
        key(&mut editor, KeyCode::Char('3'));
        key(&mut editor, KeyCode::Enter);
        assert_eq!(
            editor.preferences.answers.get(QUESTIONS[first].id),
            Some(&Answer::Choice(QUESTIONS[first].choices[2].id.to_owned()))
        );
        key(&mut editor, KeyCode::Esc);
        key(&mut editor, KeyCode::Enter);
        assert!(matches!(&editor.screen, Screen::Question(walk) if walk.position == 1));
        ctrl(&mut editor, 's');
        assert!(!editor.is_busy());
        let action = key(&mut editor, KeyCode::Enter);
        assert!(matches!(action, Action::Save { revision, .. } if revision == "initial-revision"));
        assert!(editor.is_busy());
        assert_eq!(ctrl(&mut editor, 'c'), Action::None);
        assert_eq!(key(&mut editor, KeyCode::Esc), Action::None);
        let saved = Snapshot {
            preferences: editor.preferences.clone(),
            revision: "saved-revision".to_owned(),
            preserved_markdown: String::new(),
        };
        editor.save_completed(Ok(saved));
        assert!(!editor.dirty());
        assert!(rendered(&editor, 80, 24).contains("Preferences saved"));
    }

    #[test]
    fn unicode_custom_edit_and_cancel_preserve_prior_answer() {
        let mut editor = editor();
        key(&mut editor, KeyCode::Enter);
        let first = match &editor.screen {
            Screen::Question(walk) => walk.questions[walk.position],
            _ => panic!("walkthrough"),
        };
        key(&mut editor, KeyCode::Char('4'));
        key(&mut editor, KeyCode::Enter);
        editor.handle(&Event::Paste("Use café 👩‍💻 中文".to_owned()));
        key(&mut editor, KeyCode::Backspace);
        key(&mut editor, KeyCode::Backspace);
        key(&mut editor, KeyCode::Backspace);
        key(&mut editor, KeyCode::Backspace);
        ctrl(&mut editor, 's');
        assert_eq!(
            editor.preferences.answers.get(QUESTIONS[first].id),
            Some(&Answer::Custom("Use café".to_owned()))
        );
        key(&mut editor, KeyCode::Left);
        key(&mut editor, KeyCode::Char('4'));
        key(&mut editor, KeyCode::Enter);
        editor.handle(&Event::Paste("changed".to_owned()));
        key(&mut editor, KeyCode::Esc);
        assert_eq!(
            editor.preferences.answers.get(QUESTIONS[first].id),
            Some(&Answer::Custom("Use café".to_owned()))
        );
    }

    #[test]
    fn skip_clears_answer_and_unsaved_close_is_explicit() {
        let mut editor = editor();
        key(&mut editor, KeyCode::Enter);
        key(&mut editor, KeyCode::Char('1'));
        key(&mut editor, KeyCode::Enter);
        assert!(editor.dirty());
        key(&mut editor, KeyCode::Esc);
        assert_eq!(key(&mut editor, KeyCode::Esc), Action::None);
        assert!(matches!(editor.screen, Screen::Discard(false)));
        key(&mut editor, KeyCode::Enter);
        assert!(matches!(editor.screen, Screen::Overview(_)));
        key(&mut editor, KeyCode::Enter);
        key(&mut editor, KeyCode::Left);
        key(&mut editor, KeyCode::Char('5'));
        key(&mut editor, KeyCode::Enter);
        assert!(!editor.dirty());
        key(&mut editor, KeyCode::Esc);
        assert_eq!(key(&mut editor, KeyCode::Esc), Action::Close);
    }

    #[test]
    fn additional_text_is_multiline_and_load_failures_cannot_write() {
        let mut editor = editor();
        key(&mut editor, KeyCode::Char('5'));
        key(&mut editor, KeyCode::Enter);
        editor.handle(&Event::Paste("First line\r\nSecond line".to_owned()));
        ctrl(&mut editor, 's');
        assert_eq!(editor.preferences.additional, "First line\nSecond line");
        ctrl(&mut editor, 's');
        assert!(matches!(
            key(&mut editor, KeyCode::Enter),
            Action::Save { .. }
        ));
        editor.save_completed(Err(
            "The saved file changed. Reopen to merge your choices.".to_owned()
        ));
        assert!(editor.dirty());
        assert!(editor.error().unwrap().contains("changed"));
        let mut unavailable = Editor::loading("default".to_owned());
        unavailable.load_completed(Err("Cannot load".to_owned()));
        ctrl(&mut unavailable, 's');
        assert_eq!(key(&mut unavailable, KeyCode::Enter), Action::None);
        assert!(!unavailable.is_busy());
    }

    #[test]
    fn compact_and_standard_views_keep_navigation_and_choices_visible() {
        for (width, height) in [(80, 24), (60, 20)] {
            let mut editor = editor();
            let overview = rendered(&editor, width, height);
            assert!(overview.contains("Quick setup"));
            assert!(overview.contains("Full walkthrough"));
            assert!(overview.contains("Additional instructions"));
            assert!(overview.contains("Enter Open"));
            key(&mut editor, KeyCode::Enter);
            let question = rendered(&editor, width, height);
            assert!(question.contains("Custom answer"));
            assert!(question.contains("Leave unanswered"));
            assert!(question.contains("Enter Next"));
            key(&mut editor, KeyCode::Char('4'));
            key(&mut editor, KeyCode::Enter);
            editor.handle(&Event::Paste("Unicode café 中文".to_owned()));
            let text = rendered(&editor, width, height);
            assert!(text.contains("Custom answer"));
            assert!(text.contains("Ctrl-S Use answer"));
            ctrl(&mut editor, 's');
            ctrl(&mut editor, 's');
            let review = rendered(&editor, width, height);
            assert!(review.contains("Review and save"));
            assert!(review.contains("Enter Save"));
            assert!(review.contains("Unicode café 中文"));
        }
    }

    #[test]
    fn hard_wrap_cursor_matches_graphemes_and_line_boundaries() {
        let (rows, row, column) = wrapped_text("ab中é\n👩‍💻x", 4, "ab中é\n👩‍💻".len());
        assert_eq!(rows, ["ab中", "é", "👩‍💻x"]);
        assert_eq!((row, column), (2, 2));
        let mut text = TextEditor {
            target: TextTarget::Additional,
            text: "café\n中文x".to_owned(),
            cursor: "café".len(),
        };
        move_line(&mut text, true);
        assert_eq!(text.cursor, "café\n中文".len());
        move_line(&mut text, false);
        assert_eq!(text.cursor, "café".len());
    }

    #[test]
    fn every_question_and_selected_explanation_fit_compact_terminal() {
        fn normalized(text: &str) -> String {
            text.replace('│', " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        }
        let mut editor = editor();
        for (index, question) in QUESTIONS.iter().enumerate() {
            for selected in 0..question.choices.len() {
                editor.screen = Screen::Question(Walkthrough {
                    questions: (0..QUESTIONS.len()).collect(),
                    position: index,
                    selected,
                    label: "Full walkthrough".to_owned(),
                });
                let text = normalized(&rendered(&editor, 60, 20));
                assert!(text.contains(question.title), "{} title", question.id);
                assert!(text.contains(question.prompt), "{} prompt", question.id);
                assert!(
                    text.contains(question.choices[selected].detail),
                    "{} explanation",
                    question.id
                );
                for choice in question.choices {
                    assert!(
                        text.contains(choice.label),
                        "{} choice {}",
                        question.id,
                        choice.id
                    );
                }
                assert!(text.contains("Enter Next"));
            }
        }
    }

    #[test]
    fn review_end_and_up_scroll_without_a_stuck_offset() {
        let mut editor = editor();
        editor.preferences.additional = (0..40)
            .map(|index| format!("Instruction number {index}\n"))
            .collect();
        ctrl(&mut editor, 's');
        rendered(&editor, 60, 20);
        key(&mut editor, KeyCode::End);
        let end = match editor.screen {
            Screen::Review(offset) => offset,
            _ => panic!("review"),
        };
        assert!(end > 0);
        key(&mut editor, KeyCode::Up);
        assert!(matches!(editor.screen, Screen::Review(offset) if offset == end - 1));
        key(&mut editor, KeyCode::PageDown);
        key(&mut editor, KeyCode::Down);
        assert!(matches!(editor.screen, Screen::Review(offset) if offset == end));
        key(&mut editor, KeyCode::Home);
        assert!(matches!(editor.screen, Screen::Review(0)));
    }

    #[test]
    fn oversized_paste_is_rejected_without_truncation_or_losing_the_draft() {
        let mut editor = editor();
        key(&mut editor, KeyCode::Enter);
        key(&mut editor, KeyCode::Char('4'));
        key(&mut editor, KeyCode::Enter);
        editor.handle(&Event::Paste("Keep this answer".to_owned()));
        editor.handle(&Event::Paste(
            "中".repeat(personality_preferences::MAX_CUSTOM_CHARS),
        ));
        assert!(matches!(&editor.screen, Screen::Text(text) if text.text == "Keep this answer"));
        assert!(editor.error().unwrap().contains("1200 characters"));
        key(&mut editor, KeyCode::Backspace);
        assert!(editor.error().is_none());
        assert!(rendered(&editor, 60, 20).contains("15/1200 characters"));
    }
}
