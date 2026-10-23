//! Composer layout and process-local editing state.
//!
//! `AppState::input` remains the text source of truth. This module supplies the
//! single grapheme/display-cell model used by drawing, cursor placement, and
//! vertical movement, plus registered process-only atomic ranges.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[path = "composer_history.rs"]
mod history;
pub use history::{ComposerHistory, EditKind};

pub const MAX_COMPOSER_ROWS: usize = 7;
pub const LARGE_PASTE_THRESHOLD: usize = 1_000;
/// Visible body rows of the large-paste preview strip above the draft.
pub const PASTE_PREVIEW_ROWS: usize = 4;
/// Cap for a sent user-prompt card body before local wheel scroll.
pub const USER_PROMPT_CARD_ROWS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowBreak {
    Soft,
    Hard,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerRow {
    pub range: Range<usize>,
    pub hard_line_range: Range<usize>,
    pub width: usize,
    pub capacity: usize,
    pub break_kind: RowBreak,
    pub boundaries: Vec<(usize, usize)>,
}

impl ComposerRow {
    pub fn byte_for_column(&self, desired: usize) -> usize {
        self.boundaries
            .iter()
            .min_by_key(|(_, column)| column.abs_diff(desired))
            .map(|(byte, _)| *byte)
            .unwrap_or(self.range.start)
    }

    fn column_for_byte(&self, byte: usize) -> usize {
        self.boundaries
            .iter()
            .take_while(|(boundary, _)| *boundary <= byte)
            .last()
            .map(|(_, column)| *column)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerLayout {
    pub rows: Vec<ComposerRow>,
    pub cursor_row: usize,
    pub cursor_column: usize,
}

impl ComposerLayout {
    /// Lay out a draft within the composer interior.
    ///
    /// The first visual row reserves `prompt_width` cells for the prompt and
    /// continuation rows reserve the same width as indentation. A full final
    /// row receives a virtual empty row so the native caret wraps exactly as a
    /// terminal caret does.
    pub fn new(input: &str, cursor: usize, interior_width: usize, prompt_width: usize) -> Self {
        let cursor = normalize_boundary(input, cursor);
        let first_capacity = interior_width.saturating_sub(prompt_width).max(1);
        let continuation_capacity = first_capacity;
        let hard_lines = hard_lines(input);
        let mut rows = Vec::new();

        for (hard_index, hard) in hard_lines.iter().enumerate() {
            let mut graphemes = input[hard.text.clone()]
                .grapheme_indices(true)
                .map(|(offset, grapheme)| Grapheme {
                    start: hard.text.start + offset,
                    end: hard.text.start + offset + grapheme.len(),
                    width: UnicodeWidthStr::width(grapheme),
                    whitespace: grapheme.chars().all(char::is_whitespace),
                })
                .collect::<Vec<_>>();

            if graphemes.is_empty() {
                rows.push(ComposerRow {
                    range: hard.text.start..hard.text.start,
                    hard_line_range: hard.text.clone(),
                    width: 0,
                    capacity: if rows.is_empty() {
                        first_capacity
                    } else {
                        continuation_capacity
                    },
                    break_kind: if hard.has_newline {
                        RowBreak::Hard
                    } else {
                        RowBreak::End
                    },
                    boundaries: vec![(hard.text.start, 0)],
                });
                continue;
            }

            let mut start = 0;
            while start < graphemes.len() {
                let capacity = if rows.is_empty() {
                    first_capacity
                } else {
                    continuation_capacity
                };
                let mut end = start;
                let mut width: usize = 0;
                let mut last_word_break = None;
                while end < graphemes.len() {
                    let next = graphemes[end].width;
                    if end > start && width.saturating_add(next) > capacity {
                        break;
                    }
                    if end == start && next > capacity {
                        width = next;
                        end += 1;
                        break;
                    }
                    if width.saturating_add(next) > capacity {
                        break;
                    }
                    width += next;
                    end += 1;
                    if graphemes[end - 1].whitespace {
                        last_word_break = Some(end);
                    }
                }

                if end < graphemes.len() {
                    if let Some(word_break) = last_word_break.filter(|boundary| *boundary > start) {
                        end = word_break;
                        width = graphemes[start..end].iter().map(|item| item.width).sum();
                    }
                }
                if end == start {
                    end += 1;
                    width = graphemes[start].width;
                }

                let row_start = graphemes[start].start;
                let row_end = graphemes[end - 1].end;
                let mut column = 0;
                let mut boundaries = vec![(row_start, 0)];
                for grapheme in &graphemes[start..end] {
                    column += grapheme.width;
                    boundaries.push((grapheme.end, column));
                }
                let final_visual = end == graphemes.len();
                rows.push(ComposerRow {
                    range: row_start..row_end,
                    hard_line_range: hard.text.clone(),
                    width,
                    capacity,
                    break_kind: if !final_visual {
                        RowBreak::Soft
                    } else if hard.has_newline {
                        RowBreak::Hard
                    } else {
                        RowBreak::End
                    },
                    boundaries,
                });
                start = end;
            }

            if hard_index + 1 == hard_lines.len()
                && !hard.has_newline
                && rows
                    .last()
                    .is_some_and(|row| row.width == row.capacity && cursor == input.len())
            {
                rows.push(ComposerRow {
                    range: input.len()..input.len(),
                    hard_line_range: hard.text.clone(),
                    width: 0,
                    capacity: continuation_capacity,
                    break_kind: RowBreak::End,
                    boundaries: vec![(input.len(), 0)],
                });
            }

            graphemes.clear();
        }

        if rows.is_empty() {
            rows.push(ComposerRow {
                range: 0..0,
                hard_line_range: 0..0,
                width: 0,
                capacity: first_capacity,
                break_kind: RowBreak::End,
                boundaries: vec![(0, 0)],
            });
        }

        let cursor_row = locate_cursor_row(&rows, cursor);
        let cursor_column = rows[cursor_row].column_for_byte(cursor);
        Self {
            rows,
            cursor_row,
            cursor_column,
        }
    }

    pub fn move_vertical(&self, direction: i8, preferred: &mut Option<usize>) -> Option<usize> {
        let target = if direction < 0 {
            self.cursor_row.checked_sub(1)?
        } else {
            let target = self.cursor_row + 1;
            (target < self.rows.len()).then_some(target)?
        };
        let desired = *preferred.get_or_insert(self.cursor_column);
        Some(self.rows[target].byte_for_column(desired))
    }

    pub fn visible_window(&self, current_top: usize) -> ComposerViewport {
        let maximum_top = self.rows.len().saturating_sub(MAX_COMPOSER_ROWS);
        let mut top = current_top.min(maximum_top);
        if self.cursor_row < top {
            top = self.cursor_row;
        } else if self.cursor_row >= top + MAX_COMPOSER_ROWS {
            top = self.cursor_row + 1 - MAX_COMPOSER_ROWS;
        }
        let end = (top + MAX_COMPOSER_ROWS).min(self.rows.len());
        ComposerViewport {
            top,
            end,
            hidden_above: top,
            hidden_below: self.rows.len().saturating_sub(end),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposerViewport {
    pub top: usize,
    pub end: usize,
    pub hidden_above: usize,
    pub hidden_below: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtomicKind {
    Reference,
    File,
    Folder { outside_workspace: bool },
    LargePaste { payload: std::sync::Arc<str> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtomicElement {
    pub range: Range<usize>,
    pub kind: AtomicKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryDraft {
    pub text: String,
    pub cursor: usize,
    pub atomic_elements: Vec<AtomicElement>,
    pub attachments: Vec<crate::attachments::ComposerAttachment>,
    pub images: Vec<crate::image_content::ModelImage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathCompletionEntry {
    pub display: String,
    pub replacement: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathCompletionResult {
    pub generation: u64,
    pub token: String,
    pub entries: Vec<PathCompletionEntry>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PathCompletionState {
    pub open: bool,
    pub generation: u64,
    pub token: String,
    pub loading: bool,
    pub selected: usize,
    pub entries: Vec<PathCompletionEntry>,
    pub error: Option<String>,
}

/// Screen-space map of the composer text area captured during the last render.
///
/// Clicks use this map to place `AppState::input_cursor` at the nearest
/// grapheme boundary under the pointer, matching the drawn prompt/indent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComposerHitMap {
    /// Composer text rows as `(x, y, width, height)` after queue/attachment rows.
    pub text_area: Option<(u16, u16, u16, u16)>,
    /// Visible text rows with their grapheme column boundaries.
    pub rows: Vec<ComposerRowHit>,
    /// True when the empty-input placeholder occupies `text_area`.
    pub empty_placeholder: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerRowHit {
    pub y: u16,
    /// Prompt / indent / scroll-marker cells before row content.
    pub prefix_width: u16,
    /// `(byte_offset, display_column)` for each grapheme start plus the row end.
    pub boundaries: Vec<(usize, usize)>,
}

impl ComposerHitMap {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Map a screen cell to a byte offset in `input`, if the point is text.
    pub fn cursor_for_click(&self, input: &str, x: u16, y: u16) -> Option<usize> {
        let (area_x, area_y, width, height) = self.text_area?;
        if width == 0 || height == 0 || x < area_x || y < area_y {
            return None;
        }
        if x >= area_x.saturating_add(width) || y >= area_y.saturating_add(height) {
            return None;
        }
        if self.empty_placeholder || input.is_empty() {
            return Some(0);
        }
        let row = self.rows.iter().find(|row| row.y == y)?;
        let content_x = u32::from(area_x) + u32::from(row.prefix_width);
        let desired = u32::from(x).saturating_sub(content_x) as usize;
        Some(
            row.boundaries
                .iter()
                .min_by_key(|(_, column)| column.abs_diff(desired))
                .map(|(byte, _)| *byte)
                .unwrap_or(0),
        )
    }
}

/// Screen rectangle of the large-paste preview strip captured at render.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PastePreviewHit {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl PastePreviewHit {
    pub fn contains(self, column: u16, row: u16) -> bool {
        self.width > 0
            && self.height > 0
            && column >= self.x
            && row >= self.y
            && column < self.x.saturating_add(self.width)
            && row < self.y.saturating_add(self.height)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ComposerUiState {
    pub preferred_column: Option<usize>,
    pub viewport_top: usize,
    pub history_draft: Option<HistoryDraft>,
    pub atomic_elements: Vec<AtomicElement>,
    pub dismissed_completion_token: Option<String>,
    pub path_completion: PathCompletionState,
    pub hit_map: std::cell::RefCell<ComposerHitMap>,
    /// Left-button gesture began on the composer text area.
    pub mouse_capture_active: bool,
    /// First visible payload line for each large-paste element index.
    pub paste_preview_scroll: std::collections::HashMap<usize, usize>,
    pub paste_preview_hit: std::cell::RefCell<PastePreviewHit>,
}

impl ComposerUiState {
    pub fn register_atomic(&mut self, element: AtomicElement) {
        self.atomic_elements.push(element);
        self.sort_elements();
    }

    pub fn register_large_paste(&mut self, range: Range<usize>, payload: String) {
        self.register_atomic(AtomicElement {
            range,
            kind: AtomicKind::LargePaste {
                payload: payload.into(),
            },
        });
    }

    /// Index of the large-paste element nearest the caret (or the first).
    pub fn active_large_paste_index(&self, cursor: usize) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (index, element) in self.atomic_elements.iter().enumerate() {
            if !matches!(element.kind, AtomicKind::LargePaste { .. }) {
                continue;
            }
            let distance = if cursor < element.range.start {
                element.range.start - cursor
            } else {
                cursor.saturating_sub(element.range.end)
            };
            if best.is_none_or(|(best_distance, _)| distance < best_distance) {
                best = Some((distance, index));
            }
        }
        best.map(|(_, index)| index)
    }

    pub fn large_paste_payload(&self, index: usize) -> Option<&str> {
        match self.atomic_elements.get(index)?.kind {
            AtomicKind::LargePaste { ref payload } => Some(payload.as_ref()),
            _ => None,
        }
    }

    /// Scroll the large-paste preview nearest `cursor` (usually `input_cursor`).
    pub fn scroll_paste_preview(
        &mut self,
        cursor: usize,
        direction: crate::tui::event::ScrollDirection,
        rows: usize,
    ) {
        let Some(index) = self.active_large_paste_index(cursor) else {
            return;
        };
        let payload = match self.large_paste_payload(index) {
            Some(payload) => payload,
            None => return,
        };
        let line_count = payload.lines().count().max(1);
        let window = PASTE_PREVIEW_ROWS.max(1);
        let maximum = line_count.saturating_sub(window);
        let offset = self.paste_preview_scroll.entry(index).or_insert(0);
        match direction {
            crate::tui::event::ScrollDirection::Up => {
                *offset = offset.saturating_sub(rows.max(1));
            }
            crate::tui::event::ScrollDirection::Down => {
                *offset = (*offset + rows.max(1)).min(maximum);
            }
        }
    }

    pub fn element_adjacent_to_backspace(&self, cursor: usize) -> Option<Range<usize>> {
        self.atomic_elements
            .iter()
            .find(|element| element.range.end == cursor)
            .map(|element| element.range.clone())
    }

    pub fn element_adjacent_to_delete(&self, cursor: usize) -> Option<Range<usize>> {
        self.atomic_elements
            .iter()
            .find(|element| element.range.start == cursor)
            .map(|element| element.range.clone())
    }

    pub fn element_containing(&self, cursor: usize) -> Option<Range<usize>> {
        self.atomic_elements
            .iter()
            .find(|element| element.range.start < cursor && cursor < element.range.end)
            .map(|element| element.range.clone())
    }

    pub fn previous_boundary(&self, input: &str, cursor: usize) -> usize {
        if let Some(element) = self
            .atomic_elements
            .iter()
            .find(|element| element.range.end == cursor)
        {
            return element.range.start;
        }
        if let Some(range) = self.element_containing(cursor) {
            return range.start;
        }
        previous_grapheme_boundary(input, cursor)
    }

    pub fn next_boundary(&self, input: &str, cursor: usize) -> usize {
        if let Some(element) = self
            .atomic_elements
            .iter()
            .find(|element| element.range.start == cursor)
        {
            return element.range.end;
        }
        if let Some(range) = self.element_containing(cursor) {
            return range.end;
        }
        next_grapheme_boundary(input, cursor)
    }

    /// Keep process-local ranges aligned with a replacement in the draft.
    pub fn apply_edit(&mut self, replaced: Range<usize>, inserted_len: usize) {
        let removed_len = replaced.end.saturating_sub(replaced.start);
        self.atomic_elements.retain(|element| {
            element.range.end <= replaced.start || element.range.start >= replaced.end
        });
        for element in &mut self.atomic_elements {
            if element.range.start >= replaced.end {
                element.range.start = shift_offset(element.range.start, removed_len, inserted_len);
                element.range.end = shift_offset(element.range.end, removed_len, inserted_len);
            }
        }
        self.preferred_column = None;
        self.sort_elements();
    }

    pub fn remove_range(&mut self, range: &Range<usize>) {
        self.atomic_elements
            .retain(|element| element.range != *range);
    }

    pub fn expanded_text(&self, input: &str) -> String {
        let mut result = input.to_string();
        for element in self.atomic_elements.iter().rev() {
            if let AtomicKind::LargePaste { payload } = &element.kind {
                if element.range.end <= result.len()
                    && result.is_char_boundary(element.range.start)
                    && result.is_char_boundary(element.range.end)
                {
                    result.replace_range(element.range.clone(), payload);
                }
            }
        }
        result
    }

    pub fn clear_for_new_draft(&mut self) {
        self.preferred_column = None;
        self.viewport_top = 0;
        self.history_draft = None;
        self.atomic_elements.clear();
        self.dismissed_completion_token = None;
        self.path_completion = PathCompletionState::default();
        self.mouse_capture_active = false;
        self.hit_map.borrow_mut().clear();
        self.paste_preview_scroll.clear();
        *self.paste_preview_hit.borrow_mut() = PastePreviewHit::default();
    }

    /// Scroll the draft viewport without moving the caret. Returns true if moved.
    pub fn scroll_composer_viewport(
        &mut self,
        input: &str,
        interior_width: usize,
        direction: crate::tui::event::ScrollDirection,
        rows: usize,
    ) -> bool {
        let layout = ComposerLayout::new(input, input.len(), interior_width, 2);
        let maximum_top = layout.rows.len().saturating_sub(MAX_COMPOSER_ROWS);
        let previous = self.viewport_top.min(maximum_top);
        let next = match direction {
            crate::tui::event::ScrollDirection::Up => previous.saturating_sub(rows.max(1)),
            crate::tui::event::ScrollDirection::Down => (previous + rows.max(1)).min(maximum_top),
        };
        self.viewport_top = next;
        next != previous
    }

    /// Snap a click-placed cursor out of an atomic element interior to its nearer edge.
    pub fn snap_click_cursor(&self, cursor: usize) -> usize {
        if let Some(range) = self.element_containing(cursor) {
            if cursor.saturating_sub(range.start) <= range.end.saturating_sub(cursor) {
                range.start
            } else {
                range.end
            }
        } else {
            cursor
        }
    }

    fn sort_elements(&mut self) {
        self.atomic_elements
            .sort_by_key(|element| (element.range.start, element.range.end));
    }
}

pub fn large_paste_placeholder(char_count: usize) -> String {
    format!("[Pasted Content {char_count} chars]")
}

pub fn normalize_boundary(input: &str, cursor: usize) -> usize {
    let clamped = cursor.min(input.len());
    if clamped == input.len() {
        return clamped;
    }
    input
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .take_while(|offset| *offset <= clamped)
        .last()
        .unwrap_or(0)
}

pub fn previous_grapheme_boundary(input: &str, cursor: usize) -> usize {
    let cursor = normalize_boundary(input, cursor);
    input
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .take_while(|offset| *offset < cursor)
        .last()
        .unwrap_or(0)
}

pub fn next_grapheme_boundary(input: &str, cursor: usize) -> usize {
    let cursor = normalize_boundary(input, cursor);
    input
        .grapheme_indices(true)
        .map(|(offset, _)| offset)
        .find(|offset| *offset > cursor)
        .unwrap_or(input.len())
}

pub fn hard_line_bounds(input: &str, cursor: usize) -> Range<usize> {
    let cursor = normalize_boundary(input, cursor);
    let start = input[..cursor].rfind('\n').map_or(0, |index| index + 1);
    let end = input[cursor..]
        .find('\n')
        .map_or(input.len(), |offset| cursor + offset);
    start..end
}

pub fn path_shaped(token: &str) -> bool {
    token.starts_with("./")
        || token.starts_with("../")
        || token.starts_with("~/")
        || token.starts_with('/')
}

/// List exactly one parent directory without recursion or symlink following.
#[cfg(test)]
pub fn discover_path_completions(
    token: &str,
    cwd: &std::path::Path,
    generation: u64,
) -> PathCompletionResult {
    discover_path_completions_inner(token, cwd, generation, None)
}

pub fn discover_path_completions_cancellable(
    token: &str,
    cwd: &std::path::Path,
    generation: u64,
    cancelled: &std::sync::atomic::AtomicBool,
) -> PathCompletionResult {
    discover_path_completions_inner(token, cwd, generation, Some(cancelled))
}

fn discover_path_completions_inner(
    token: &str,
    cwd: &std::path::Path,
    generation: u64,
    cancelled: Option<&std::sync::atomic::AtomicBool>,
) -> PathCompletionResult {
    use std::path::{Path, PathBuf};

    let expanded = if let Some(rest) = token.strip_prefix("~/") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.to_path_buf())
            .join(rest)
    } else {
        let candidate = Path::new(token);
        if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            cwd.join(candidate)
        }
    };
    let ends_with_separator = token.ends_with(std::path::MAIN_SEPARATOR);
    let (parent, prefix, textual_parent) = if ends_with_separator {
        (expanded.clone(), String::new(), token.to_string())
    } else {
        (
            expanded.parent().unwrap_or(cwd).to_path_buf(),
            expanded
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            token
                .rfind('/')
                .map_or_else(String::new, |index| token[..=index].to_string()),
        )
    };
    let mut result = PathCompletionResult {
        generation,
        token: token.to_string(),
        entries: Vec::new(),
        error: None,
    };
    let read = match std::fs::read_dir(&parent) {
        Ok(read) => read,
        Err(error) => {
            result.error = Some(format!("Path completion failed: {error}"));
            return result;
        }
    };
    for entry in read.flatten() {
        if cancelled.is_some_and(|cancelled| cancelled.load(std::sync::atomic::Ordering::Relaxed)) {
            result.entries.clear();
            return result;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() || (!file_type.is_file() && !file_type.is_dir()) {
            continue;
        }
        let is_dir = file_type.is_dir();
        let suffix = if is_dir {
            std::path::MAIN_SEPARATOR.to_string()
        } else {
            String::new()
        };
        result.entries.push(PathCompletionEntry {
            display: format!("{name}{suffix}"),
            replacement: format!("{textual_parent}{name}{suffix}"),
            is_dir,
        });
    }
    result.entries.sort_by(|left, right| {
        right.is_dir.cmp(&left.is_dir).then_with(|| {
            left.display
                .to_lowercase()
                .cmp(&right.display.to_lowercase())
        })
    });
    result.entries.truncate(256);
    result
}

#[derive(Debug, Clone)]
struct HardLine {
    text: Range<usize>,
    has_newline: bool,
}

#[derive(Debug, Clone)]
struct Grapheme {
    start: usize,
    end: usize,
    width: usize,
    whitespace: bool,
}

fn hard_lines(input: &str) -> Vec<HardLine> {
    let mut result = Vec::new();
    let mut start = 0;
    for (index, character) in input.char_indices() {
        if character == '\n' {
            result.push(HardLine {
                text: start..index,
                has_newline: true,
            });
            start = index + 1;
        }
    }
    result.push(HardLine {
        text: start..input.len(),
        has_newline: false,
    });
    result
}

fn locate_cursor_row(rows: &[ComposerRow], cursor: usize) -> usize {
    for (index, row) in rows.iter().enumerate() {
        if row.range.start <= cursor && cursor < row.range.end {
            return index;
        }
        if cursor == row.range.end {
            if row.break_kind == RowBreak::End
                && row.width == row.capacity
                && rows
                    .get(index + 1)
                    .is_some_and(|next| next.range.is_empty() && next.range.start == cursor)
            {
                return index + 1;
            }
            if row.break_kind != RowBreak::Soft {
                return index;
            }
            if rows
                .get(index + 1)
                .is_some_and(|next| next.range.start == cursor)
            {
                return index + 1;
            }
        }
        if row.range.is_empty() && cursor == row.range.start {
            return index;
        }
    }
    rows.len().saturating_sub(1)
}

fn shift_offset(offset: usize, removed_len: usize, inserted_len: usize) -> usize {
    if inserted_len >= removed_len {
        offset.saturating_add(inserted_len - removed_len)
    } else {
        offset.saturating_sub(removed_len - inserted_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_newlines_and_word_wrap_share_one_cursor_model() {
        let input = "one two\n你e\u{301}nd";
        let layout = ComposerLayout::new(input, input.len(), 8, 2);
        assert_eq!(layout.rows.len(), 3);
        assert_eq!(layout.rows[0].range, 0..4);
        assert_eq!(layout.rows[1].range, 4..7);
        assert_eq!(layout.cursor_row, 2);
        assert_eq!(layout.cursor_column, 5);
    }

    #[test]
    fn exact_width_end_wraps_native_caret_to_a_virtual_row() {
        let layout = ComposerLayout::new("abcd", 4, 6, 2);
        assert_eq!(layout.rows.len(), 2);
        assert_eq!(layout.cursor_row, 1);
        assert_eq!(layout.cursor_column, 0);
    }

    #[test]
    fn viewport_tracks_cursor_and_reports_both_overflows() {
        let input = (0..10)
            .map(|index| format!("row{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let cursor = input.find("row8").unwrap();
        let layout = ComposerLayout::new(&input, cursor, 40, 2);
        let viewport = layout.visible_window(2);
        assert_eq!(viewport.top, 2);
        assert_eq!(viewport.end, 9);
        assert_eq!(viewport.hidden_above, 2);
        assert_eq!(viewport.hidden_below, 1);
    }

    #[test]
    fn vertical_motion_preserves_a_preferred_display_column() {
        let input = "abcdef\n你x\nabcdef";
        let cursor = 5;
        let layout = ComposerLayout::new(input, cursor, 20, 2);
        let mut preferred = None;
        let middle = layout.move_vertical(1, &mut preferred).unwrap();
        assert_eq!(middle, "abcdef\n你x".len());
        let middle_layout = ComposerLayout::new(input, middle, 20, 2);
        let bottom = middle_layout.move_vertical(1, &mut preferred).unwrap();
        assert_eq!(bottom, input.len() - 1);
        assert_eq!(preferred, Some(5));
    }

    #[test]
    fn combining_sequences_and_wide_glyphs_are_atomic() {
        let input = "ae\u{301}你z";
        let after_combining = "ae\u{301}".len();
        assert_eq!(previous_grapheme_boundary(input, after_combining), 1);
        assert_eq!(next_grapheme_boundary(input, 1), after_combining);
        let layout = ComposerLayout::new(input, "ae\u{301}你".len(), 20, 2);
        assert_eq!(layout.cursor_column, 4);
    }

    #[test]
    fn registered_paste_expands_by_range_not_lookalike_text() {
        let marker = large_paste_placeholder(1_100);
        let input = format!("typed {marker} actual {marker}");
        let start = input.rfind(&marker).unwrap();
        let mut state = ComposerUiState::default();
        state.register_large_paste(start..start + marker.len(), "x".repeat(1_100));
        let expanded = state.expanded_text(&input);
        assert!(expanded.starts_with(&format!("typed {marker} actual ")));
        assert!(expanded.ends_with(&"x".repeat(1_100)));
    }

    #[test]
    fn atomic_ranges_shift_and_delete_as_one_element() {
        let mut state = ComposerUiState::default();
        state.atomic_elements.push(AtomicElement {
            range: 4..10,
            kind: AtomicKind::File,
        });
        state.apply_edit(0..0, 2);
        assert_eq!(state.element_adjacent_to_backspace(12), Some(6..12));
        state.remove_range(&(6..12));
        assert!(state.atomic_elements.is_empty());
    }

    #[test]
    fn horizontal_motion_skips_complete_atomic_elements() {
        let input = "a@src/main.rs z";
        let mut state = ComposerUiState::default();
        state.atomic_elements.push(AtomicElement {
            range: 1..13,
            kind: AtomicKind::File,
        });
        assert_eq!(state.next_boundary(input, 1), 13);
        assert_eq!(state.previous_boundary(input, 13), 1);
        assert_eq!(state.next_boundary(input, 5), 13);
        assert_eq!(state.previous_boundary(input, 5), 1);
    }

    #[test]
    fn inline_path_completion_requires_explicit_path_syntax() {
        for path in ["./src", "../src", "~/src", "/usr/local"] {
            assert!(path_shaped(path), "expected an explicit path: {path}");
        }
        for prose in ["red/ops", "and/or", "src/main.rs"] {
            assert!(!path_shaped(prose), "unexpected prose path: {prose}");
        }
    }

    #[test]
    fn path_completion_lists_one_level_and_skips_symlinks() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("spec.md"), "x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("spec.md", root.path().join("symlink.md")).unwrap();
        let result = discover_path_completions("s", root.path(), 4);
        assert_eq!(result.generation, 4);
        assert_eq!(
            result
                .entries
                .iter()
                .map(|entry| entry.display.as_str())
                .collect::<Vec<_>>(),
            ["src/", "spec.md"]
        );
    }

    #[test]
    fn hit_map_maps_empty_placeholder_and_nearest_grapheme() {
        let empty = ComposerHitMap {
            text_area: Some((2, 10, 20, 2)),
            rows: Vec::new(),
            empty_placeholder: true,
        };
        assert_eq!(empty.cursor_for_click("", 2, 10), Some(0));
        assert_eq!(empty.cursor_for_click("", 22, 10), None);
        assert_eq!(empty.cursor_for_click("", 2, 12), None);

        let input = "a你b";
        // "a" width 1, "你" width 2, "b" width 1 → boundaries columns 0,1,3,4
        let map = ComposerHitMap {
            text_area: Some((2, 10, 20, 2)),
            rows: vec![ComposerRowHit {
                y: 10,
                prefix_width: 2,
                boundaries: vec![(0, 0), (1, 1), (4, 3), (5, 4)],
            }],
            empty_placeholder: false,
        };
        assert_eq!(map.cursor_for_click(input, 4, 10), Some(0));
        assert_eq!(map.cursor_for_click(input, 5, 10), Some(1));
        // Mid-wide-glyph column snaps to its start byte
        assert_eq!(map.cursor_for_click(input, 6, 10), Some(1));
        assert_eq!(map.cursor_for_click(input, 8, 10), Some(5));
    }

    #[test]
    fn snap_click_cursor_chooses_nearer_atomic_edge() {
        let mut state = ComposerUiState::default();
        state.atomic_elements.push(AtomicElement {
            range: 2..9,
            kind: AtomicKind::File,
        });
        assert_eq!(state.snap_click_cursor(3), 2);
        assert_eq!(state.snap_click_cursor(8), 9);
        assert_eq!(state.snap_click_cursor(1), 1);
    }
}
