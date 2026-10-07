//! Process-local, bottom-relative transcript viewport state.

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use std::cell::RefCell;
use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;

const MAX_SELECTABLE_CELLS: usize = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelectionPoint {
    x: u16,
    row: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionEdge {
    Up,
    Down,
}

#[derive(Debug, Clone)]
struct VisibleSelection {
    anchor: SelectionPoint,
    focus: SelectionPoint,
    moved: bool,
    dragging: bool,
    geometry: Rect,
    rows: BTreeMap<u32, Vec<SelectableCell>>,
    cached_cells: usize,
    bound_exceeded: bool,
    edge: Option<SelectionEdge>,
}

#[derive(Debug, Clone, Default)]
struct SelectableCell {
    symbol: String,
    continuation: bool,
}

#[derive(Debug, Clone, Default)]
struct SelectableFrame {
    area: Rect,
    document_top: u32,
    cells: Vec<SelectableCell>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityHitTarget {
    pub key: crate::app::TranscriptBlockKey,
    pub row: u32,
    pub left: u16,
    pub right: u16,
}

impl SelectableFrame {
    fn contains_screen(&self, x: u16, y: u16) -> bool {
        self.area.contains(Position::new(x, y))
    }

    fn clamp_screen(&self, x: u16, y: u16) -> Option<Position> {
        if self.area.is_empty() {
            return None;
        }
        Some(Position::new(
            x.clamp(self.area.left(), self.area.right().saturating_sub(1)),
            y.clamp(self.area.top(), self.area.bottom().saturating_sub(1)),
        ))
    }

    fn index_screen(&self, point: Position) -> Option<usize> {
        self.area.contains(point).then(|| {
            usize::from(point.y - self.area.y) * usize::from(self.area.width)
                + usize::from(point.x - self.area.x)
        })
    }

    fn cell_screen(&self, point: Position) -> Option<&SelectableCell> {
        self.index_screen(point)
            .and_then(|index| self.cells.get(index))
    }

    fn normalize_screen(&self, mut point: Position) -> Position {
        while point.x > self.area.left()
            && self
                .cell_screen(point)
                .is_some_and(|cell| cell.continuation)
        {
            point.x -= 1;
        }
        point
    }

    fn document_point(&self, point: Position) -> SelectionPoint {
        SelectionPoint {
            x: point.x,
            row: self
                .document_top
                .saturating_add(u32::from(point.y.saturating_sub(self.area.top()))),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TranscriptScrollState {
    bottom_offset: std::cell::Cell<u16>,
    pub unseen_messages: usize,
    known_messages: usize,
    known_stream_bytes: usize,
    measured_maximum: std::cell::Cell<u16>,
    has_measurement: std::cell::Cell<bool>,
    suppress_next_growth_adjustment: std::cell::Cell<bool>,
    selectable_frame: RefCell<SelectableFrame>,
    selection: RefCell<Option<VisibleSelection>>,
    activity_hit_targets: RefCell<Vec<ActivityHitTarget>>,
}

impl TranscriptScrollState {
    pub fn following(&self) -> bool {
        self.bottom_offset.get() == 0
    }

    pub fn bottom_offset(&self) -> u16 {
        self.bottom_offset.get().min(self.measured_maximum.get())
    }

    pub fn request_offset(&mut self, rows: u16) {
        self.bottom_offset.set(rows);
    }

    pub fn page_up(&mut self, rows: u16) {
        let maximum = self.measured_maximum.get();
        let next = self
            .bottom_offset()
            .saturating_add(rows.max(1))
            .min(maximum);
        self.bottom_offset.set(next);
    }

    pub fn page_down(&mut self, rows: u16) {
        let next = self.bottom_offset().saturating_sub(rows.max(1));
        self.bottom_offset.set(next);
        if next == 0 {
            self.unseen_messages = 0;
        }
    }

    pub fn oldest(&mut self) {
        self.bottom_offset.set(if self.has_measurement.get() {
            self.measured_maximum.get()
        } else {
            u16::MAX
        });
    }

    pub fn latest(&mut self) {
        self.bottom_offset.set(0);
        self.unseen_messages = 0;
    }

    pub fn observe_tail(&mut self, messages: usize, stream_bytes: usize) {
        if self.following() {
            self.unseen_messages = 0;
        } else {
            self.unseen_messages = self
                .unseen_messages
                .saturating_add(messages.saturating_sub(self.known_messages));
            if stream_bytes > self.known_stream_bytes && messages == self.known_messages {
                self.unseen_messages = self.unseen_messages.max(1);
            }
        }
        self.known_messages = messages;
        self.known_stream_bytes = stream_bytes;
    }

    pub fn note_history_prepend(&mut self, count: usize) {
        self.known_messages = self.known_messages.saturating_add(count);
        self.suppress_next_growth_adjustment.set(true);
    }

    /// Reconcile the requested bottom-relative offset with the exact wrapped
    /// row count produced by the paragraph renderer. Growth at the transcript
    /// tail increases the offset so the visible top row does not move; a
    /// history prepend is explicitly exempt because bottom-relative anchoring
    /// already preserves the same old content.
    pub fn observe_rendered_maximum(&self, maximum: u16) -> u16 {
        let had_measurement = self.has_measurement.replace(true);
        let previous_maximum = self.measured_maximum.replace(maximum);
        let suppress = self.suppress_next_growth_adjustment.replace(false);
        let mut offset = self.bottom_offset.get();
        if had_measurement && offset > 0 && maximum > previous_maximum && !suppress {
            offset = offset.saturating_add(maximum - previous_maximum);
        }
        offset = offset.min(maximum);
        self.bottom_offset.set(offset);
        offset
    }

    pub fn capture_selectable_frame(&self, buffer: &Buffer, area: Rect) {
        self.capture_selectable_frame_at(buffer, area, 0);
    }

    pub fn capture_selectable_frame_at(&self, buffer: &Buffer, area: Rect, document_top: u32) {
        let area = buffer.area.intersection(area);
        if area.is_empty() || area.area() as usize > MAX_SELECTABLE_CELLS {
            *self.selectable_frame.borrow_mut() = SelectableFrame::default();
            *self.selection.borrow_mut() = None;
            return;
        }
        let mut cells = Vec::with_capacity(area.area() as usize);
        for y in area.top()..area.bottom() {
            let mut continuation_cells = 0_usize;
            for x in area.left()..area.right() {
                let symbol = buffer[(x, y)].symbol();
                if continuation_cells > 0 {
                    cells.push(SelectableCell {
                        symbol: String::new(),
                        continuation: true,
                    });
                    continuation_cells -= 1;
                } else {
                    continuation_cells = UnicodeWidthStr::width(symbol).saturating_sub(1);
                    cells.push(SelectableCell {
                        symbol: symbol.to_string(),
                        continuation: false,
                    });
                }
            }
        }
        let frame = SelectableFrame {
            area,
            document_top,
            cells,
        };
        {
            let mut selected = self.selection.borrow_mut();
            if let Some(selection) = selected.as_mut().filter(|selection| selection.dragging) {
                if selection.geometry != area {
                    *selected = None;
                } else {
                    cache_frame(selection, &frame);
                }
            }
        }
        *self.selectable_frame.borrow_mut() = frame;
    }

    pub fn set_activity_hit_targets(&self, targets: Vec<ActivityHitTarget>) {
        *self.activity_hit_targets.borrow_mut() = targets;
    }

    pub fn clear_activity_hit_targets(&self) {
        self.activity_hit_targets.borrow_mut().clear();
    }

    pub fn activity_key_for_click_release(
        &self,
        x: u16,
        y: u16,
    ) -> Option<crate::app::TranscriptBlockKey> {
        let selected = self.selection.borrow();
        let selection = selected
            .as_ref()
            .filter(|selection| selection.dragging && !selection.moved)?;
        let frame = self.selectable_frame.borrow();
        if selection.geometry != frame.area || !frame.contains_screen(x, y) {
            return None;
        }
        let point = frame.normalize_screen(Position::new(x, y));
        let point = frame.document_point(point);
        if point != selection.anchor {
            return None;
        }
        self.activity_hit_targets
            .borrow()
            .iter()
            .find(|target| target.row == point.row && x >= target.left && x < target.right)
            .map(|target| target.key.clone())
    }

    pub fn begin_selection(&mut self, x: u16, y: u16) -> bool {
        let frame = self.selectable_frame.borrow();
        if !frame.contains_screen(x, y) {
            *self.selection.get_mut() = None;
            return false;
        }
        let point = frame.normalize_screen(Position::new(x, y));
        let point = frame.document_point(point);
        let mut selection = VisibleSelection {
            anchor: point,
            focus: point,
            moved: false,
            dragging: true,
            geometry: frame.area,
            rows: BTreeMap::new(),
            cached_cells: 0,
            bound_exceeded: false,
            edge: edge_for_y(frame.area, y),
        };
        cache_frame(&mut selection, &frame);
        *self.selection.get_mut() = Some(selection);
        true
    }

    pub fn update_selection(&mut self, x: u16, y: u16) -> bool {
        let frame = self.selectable_frame.borrow();
        let Some(point) = frame.clamp_screen(x, y) else {
            return false;
        };
        let point = frame.normalize_screen(point);
        let point = frame.document_point(point);
        let selected = self.selection.get_mut();
        let Some(selection) = selected.as_mut().filter(|selection| selection.dragging) else {
            return false;
        };
        if selection.geometry != frame.area {
            *selected = None;
            return false;
        }
        cache_frame(selection, &frame);
        selection.focus = point;
        selection.moved |= point != selection.anchor;
        selection.edge = edge_for_y(frame.area, y);
        true
    }

    pub fn finish_selection(&mut self, x: u16, y: u16) -> Option<String> {
        if !self.update_selection(x, y) {
            return None;
        }
        let selected = self.selection.get_mut();
        if let Some(selection) = selected.as_mut() {
            selection.dragging = false;
            selection.edge = None;
            if !selection.moved {
                *selected = None;
                return None;
            }
        }
        self.selected_text()
    }

    pub fn clear_selection(&mut self) {
        *self.selection.get_mut() = None;
    }

    pub fn has_selection(&self) -> bool {
        self.selection
            .borrow()
            .as_ref()
            .is_some_and(|selection| selection.moved)
    }

    pub fn is_dragging(&self) -> bool {
        self.selection
            .borrow()
            .as_ref()
            .is_some_and(|selection| selection.dragging)
    }

    pub fn drag_edge(&self) -> Option<SelectionEdge> {
        self.selection
            .borrow()
            .as_ref()
            .filter(|selection| selection.dragging)
            .and_then(|selection| selection.edge)
    }

    pub fn selection_limit_exceeded(&self) -> bool {
        self.selection
            .borrow()
            .as_ref()
            .is_some_and(|selection| selection.bound_exceeded)
    }

    pub(crate) fn selection_text(&self) -> Option<String> {
        self.selected_text()
    }

    fn selected_text(&self) -> Option<String> {
        let selection = self.selection.borrow().as_ref()?.clone();
        if selection.bound_exceeded {
            return None;
        }
        let (start, end) = if selection_linear(&selection, selection.anchor)
            <= selection_linear(&selection, selection.focus)
        {
            (selection.anchor, selection.focus)
        } else {
            (selection.focus, selection.anchor)
        };
        let mut lines =
            Vec::with_capacity(usize::try_from(end.row.saturating_sub(start.row)).unwrap_or(0) + 1);
        for row in start.row..=end.row {
            let cells = selection.rows.get(&row)?;
            let left = if row == start.row {
                start.x
            } else {
                selection.geometry.left()
            };
            let right = if row == end.row {
                end.x
            } else {
                selection.geometry.right().saturating_sub(1)
            };
            let mut line = String::new();
            for x in left..=right {
                if let Some(cell) = cells.get(usize::from(x - selection.geometry.left())) {
                    if !cell.continuation {
                        line.push_str(&cell.symbol);
                    }
                }
            }
            lines.push(line.trim_end().to_string());
        }
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        let selected = lines.join("\n");
        (!selected.is_empty()).then_some(selected)
    }

    pub fn render_selection(&self, buffer: &mut Buffer) {
        let selection = self.selection.borrow();
        let Some(selection) = selection.as_ref().filter(|selection| selection.moved) else {
            return;
        };
        let frame = self.selectable_frame.borrow();
        if selection.geometry != frame.area {
            return;
        }
        let (start, end) = if selection_linear(selection, selection.anchor)
            <= selection_linear(selection, selection.focus)
        {
            (selection.anchor, selection.focus)
        } else {
            (selection.focus, selection.anchor)
        };
        for y in frame.area.top()..frame.area.bottom() {
            let row = frame
                .document_top
                .saturating_add(u32::from(y - frame.area.top()));
            for x in frame.area.left()..frame.area.right() {
                let point = SelectionPoint { x, row };
                let linear = selection_linear(selection, point);
                if linear < selection_linear(selection, start)
                    || linear > selection_linear(selection, end)
                {
                    continue;
                }
                if let Some(cell) = buffer.cell_mut(Position::new(x, y)) {
                    cell.modifier.insert(Modifier::REVERSED);
                }
            }
        }
    }
}

fn edge_for_y(area: Rect, y: u16) -> Option<SelectionEdge> {
    if y <= area.top() {
        Some(SelectionEdge::Up)
    } else if y >= area.bottom().saturating_sub(1) {
        Some(SelectionEdge::Down)
    } else {
        None
    }
}

fn selection_linear(selection: &VisibleSelection, point: SelectionPoint) -> u64 {
    u64::from(point.row) * u64::from(selection.geometry.width)
        + u64::from(point.x.saturating_sub(selection.geometry.left()))
}

fn cache_frame(selection: &mut VisibleSelection, frame: &SelectableFrame) {
    for local_row in 0..frame.area.height {
        let document_row = frame.document_top.saturating_add(u32::from(local_row));
        if selection.rows.contains_key(&document_row) {
            continue;
        }
        let width = usize::from(frame.area.width);
        if selection.cached_cells.saturating_add(width) > MAX_SELECTABLE_CELLS {
            selection.bound_exceeded = true;
            return;
        }
        let start = usize::from(local_row) * width;
        let end = start.saturating_add(width);
        selection
            .rows
            .insert(document_row, frame.cells[start..end].to_vec());
        selection.cached_cells = selection.cached_cells.saturating_add(width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Modifier, Style};

    #[test]
    fn page_return_to_zero_restores_follow_and_clears_unseen() {
        let mut state = TranscriptScrollState::default();
        state.observe_rendered_maximum(40);
        state.observe_tail(2, 0);
        state.page_up(10);
        state.observe_tail(4, 0);
        assert_eq!(state.unseen_messages, 2);
        state.page_down(5);
        assert!(!state.following());
        state.page_down(5);
        assert!(state.following());
        assert_eq!(state.unseen_messages, 0);
    }

    #[test]
    fn history_prepend_keeps_bottom_anchor_without_counting_as_unseen() {
        let mut state = TranscriptScrollState::default();
        state.observe_rendered_maximum(40);
        state.observe_tail(3, 0);
        state.page_up(8);
        state.note_history_prepend(4);
        state.observe_tail(7, 0);
        assert_eq!(state.observe_rendered_maximum(55), 8);
        assert_eq!(state.unseen_messages, 0);
    }

    #[test]
    fn rendered_tail_growth_freezes_the_visible_top_row() {
        let mut state = TranscriptScrollState::default();
        state.observe_rendered_maximum(30);
        state.page_up(8);
        state.observe_tail(1, 10);
        assert_eq!(state.observe_rendered_maximum(35), 13);
        assert_eq!(state.bottom_offset(), 13);
    }

    #[test]
    fn visible_selection_extracts_forward_reverse_multiline_and_unicode_text() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 3));
        buffer.set_string(0, 0, "alpha", Style::default());
        buffer.set_string(0, 1, "βeta界", Style::default());
        let mut state = TranscriptScrollState::default();
        state.capture_selectable_frame(&buffer, Rect::new(0, 0, 8, 3));

        assert!(state.begin_selection(1, 0));
        assert!(state.update_selection(5, 1));
        assert_eq!(
            state.finish_selection(5, 1).as_deref(),
            Some("lpha\nβeta界")
        );

        assert!(state.begin_selection(5, 1));
        assert!(state.update_selection(1, 0));
        assert_eq!(
            state.finish_selection(1, 0).as_deref(),
            Some("lpha\nβeta界")
        );
    }

    #[test]
    fn activity_hit_targets_respect_document_scroll_drag_and_clear() {
        let mut buffer = Buffer::empty(Rect::new(0, 4, 30, 3));
        buffer.set_string(2, 5, "[ activity ]", Style::default());
        let mut state = TranscriptScrollState::default();
        state.capture_selectable_frame_at(&buffer, Rect::new(0, 4, 30, 3), 10);
        let key = crate::app::TranscriptBlockKey::tool_calls("edit-one").unwrap();
        state.set_activity_hit_targets(vec![ActivityHitTarget {
            key: key.clone(),
            row: 11,
            left: 2,
            right: 14,
        }]);

        assert!(state.begin_selection(3, 5));
        assert_eq!(
            state.activity_key_for_click_release(3, 5),
            Some(key.clone())
        );
        assert!(state.finish_selection(3, 5).is_none());

        assert!(state.begin_selection(3, 5));
        assert!(state.update_selection(8, 5));
        assert_eq!(state.activity_key_for_click_release(8, 5), None);
        assert!(state.finish_selection(8, 5).is_some());

        state.clear_activity_hit_targets();
        assert!(state.begin_selection(3, 5));
        assert_eq!(state.activity_key_for_click_release(3, 5), None);
    }

    #[test]
    fn visible_selection_clamps_bounds_omits_padding_and_highlights_without_mutating_text() {
        let mut buffer = Buffer::empty(Rect::new(2, 3, 8, 2));
        buffer.set_string(2, 3, "copy me", Style::default());
        buffer.set_string(2, 4, "界", Style::default());
        let mut state = TranscriptScrollState::default();
        state.capture_selectable_frame(&buffer, Rect::new(2, 3, 8, 2));

        assert!(!state.begin_selection(1, 3));
        assert!(state.begin_selection(3, 3));
        assert!(state.update_selection(u16::MAX, 3));
        assert_eq!(
            state.finish_selection(u16::MAX, 3).as_deref(),
            Some("opy me")
        );
        state.render_selection(&mut buffer);
        assert_eq!(buffer[(3, 3)].symbol(), "o");
        assert!(buffer[(3, 3)].modifier.contains(Modifier::REVERSED));
        assert!(!buffer[(2, 3)].modifier.contains(Modifier::REVERSED));

        assert!(state.begin_selection(3, 3));
        assert!(state.finish_selection(3, 3).is_none());
        assert!(!state.has_selection());

        assert!(state.begin_selection(3, 4));
        assert!(state.update_selection(9, 4));
        assert_eq!(state.finish_selection(9, 4).as_deref(), Some("界"));
    }

    #[test]
    fn selection_caches_forward_and_reverse_ranges_across_scroll_frames() {
        let mut first = Buffer::empty(Rect::new(0, 0, 8, 2));
        first.set_string(0, 0, "alpha", Style::default());
        first.set_string(0, 1, "βeta界", Style::default());
        let mut second = Buffer::empty(Rect::new(0, 0, 8, 2));
        second.set_string(0, 0, "gamma界", Style::default());
        second.set_string(0, 1, "delta", Style::default());
        let mut state = TranscriptScrollState::default();

        state.capture_selectable_frame_at(&first, first.area, 0);
        assert!(state.begin_selection(1, 0));
        assert!(state.update_selection(7, 1));
        state.capture_selectable_frame_at(&second, second.area, 2);
        assert!(state.update_selection(3, 1));
        assert_eq!(
            state.finish_selection(3, 1).as_deref(),
            Some("lpha\nβeta界\ngamma界\ndelt")
        );

        assert!(state.begin_selection(3, 1));
        state.capture_selectable_frame_at(&first, first.area, 0);
        assert!(state.update_selection(1, 0));
        assert_eq!(
            state.finish_selection(1, 0).as_deref(),
            Some("lpha\nβeta界\ngamma界\ndelt")
        );
    }

    #[test]
    fn incompatible_geometry_cancels_drag_and_cell_bound_stops_growth() {
        let mut small = Buffer::empty(Rect::new(0, 0, 8, 2));
        small.set_string(0, 0, "select", Style::default());
        let mut state = TranscriptScrollState::default();
        state.capture_selectable_frame_at(&small, small.area, 0);
        assert!(state.begin_selection(0, 0));
        let resized = Buffer::empty(Rect::new(0, 0, 9, 2));
        state.capture_selectable_frame_at(&resized, resized.area, 0);
        assert!(!state.is_dragging());

        let page = Buffer::empty(Rect::new(0, 0, 100, 100));
        state.capture_selectable_frame_at(&page, page.area, 0);
        assert!(state.begin_selection(0, 0));
        for page_index in 1..=20_u32 {
            state.capture_selectable_frame_at(&page, page.area, page_index * 100);
            state.update_selection(99, 99);
        }
        assert!(state.selection_limit_exceeded());
        assert!(state.finish_selection(99, 99).is_none());
    }
}
