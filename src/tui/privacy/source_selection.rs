//! Process-local hit testing for the Original file pane.

use crate::tui::composer::ComposerLayout;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::ops::Range;

#[derive(Debug)]
struct Cell {
    end_column: usize,
    bytes: Range<usize>,
}

#[derive(Debug, Default)]
pub(super) struct HitMap {
    area: Rect,
    rows: Vec<Vec<Cell>>,
}

impl HitMap {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn is_empty(&self) -> bool {
        self.area.is_empty()
    }

    pub(super) fn contains(&self, column: u16, row: u16) -> bool {
        self.area.contains(Position::new(column, row))
    }

    pub(super) fn bytes_at(&self, column: u16, row: u16, dragging: bool) -> Option<Range<usize>> {
        if !self.contains(column, row) && (!dragging || self.area.is_empty()) {
            return None;
        }
        let row = usize::from(row.saturating_sub(self.area.y));
        let row = if dragging {
            row.min(self.rows.len().saturating_sub(1))
        } else {
            row
        };
        let cells = self.rows.get(row)?;
        let column = usize::from(column.saturating_sub(self.area.x));
        let index = cells.partition_point(|cell| cell.end_column <= column);
        let index = if dragging {
            index.min(cells.len().saturating_sub(1))
        } else {
            index
        };
        cells.get(index).map(|cell| cell.bytes.clone())
    }
}

pub(super) struct OriginalLayout {
    text: String,
    // Each displayed character retains its offset before terminal sanitization.
    origins: Vec<(usize, Range<usize>)>,
    layout: ComposerLayout,
}

impl OriginalLayout {
    pub(super) fn new(input: &str, offset: usize, width: u16) -> Self {
        let mut text = String::with_capacity(input.len());
        let mut origins = Vec::new();
        for (source, character) in crate::terminal_text::sanitized_char_indices(input) {
            let bytes = offset + source..offset + source + character.len_utf8();
            origins.push((text.len(), bytes));
            text.push(character);
        }
        let layout = ComposerLayout::new(&text, 0, usize::from(width), 0);
        Self {
            text,
            origins,
            layout,
        }
    }

    pub(super) fn row_count(&self) -> usize {
        self.layout.rows.len()
    }

    pub(super) fn visible(
        &self,
        area: Rect,
        scroll: u16,
        selection: Option<&Range<usize>>,
    ) -> (Vec<Line<'_>>, HitMap) {
        let mut map = HitMap {
            area,
            rows: Vec::new(),
        };
        let mut lines = Vec::new();
        for row in self
            .layout
            .rows
            .iter()
            .skip(usize::from(scroll))
            .take(usize::from(area.height))
        {
            let mut cells = Vec::new();
            let mut highlight: Option<Range<usize>> = None;
            for ((start, column), (end, end_column)) in
                row.boundaries.iter().zip(row.boundaries.iter().skip(1))
            {
                if *end_column > usize::from(area.width) || end_column == column {
                    continue;
                }
                let first = self.origins.partition_point(|(offset, _)| offset < start);
                let last = self.origins.partition_point(|(offset, _)| offset < end) - 1;
                let bytes = self.origins[first].1.start..self.origins[last].1.end;
                if selection.is_some_and(|range| range.start < bytes.end && bytes.start < range.end)
                {
                    let span = highlight.get_or_insert(*start..*end);
                    span.end = *end;
                }
                cells.push(Cell {
                    end_column: *end_column,
                    bytes,
                });
            }
            let line = if let Some(highlight) = highlight {
                Line::from(vec![
                    Span::raw(&self.text[row.range.start..highlight.start]),
                    Span::styled(
                        &self.text[highlight.clone()],
                        Style::default().add_modifier(Modifier::REVERSED),
                    ),
                    Span::raw(&self.text[highlight.end..row.range.end]),
                ])
            } else {
                Line::raw(&self.text[row.range.clone()])
            };
            lines.push(line);
            map.rows.push(cells);
        }
        (lines, map)
    }
}

#[derive(Debug)]
pub(super) struct Drag {
    pub anchor: Range<usize>,
    pub focus: Range<usize>,
    pub moved: bool,
}

impl Drag {
    pub(super) fn bytes(&self) -> Range<usize> {
        self.anchor.start.min(self.focus.start)..self.anchor.end.max(self.focus.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_cells_keep_offsets_across_sanitization_scroll_and_graphemes() {
        let input = "header\r\n\u{1b}]界 hidden\u{7}e\u{301}界 tail";
        let base = 32768;
        let area = Rect::new(7, 9, 10, 3);
        let layout = OriginalLayout::new(input, base, area.width);
        let start = input.find("e\u{301}").unwrap();
        let wide = start + "e\u{301}".len();
        let selection = base + start..base + wide + '界'.len_utf8();
        let (lines, hits) = layout.visible(area, 1, Some(&selection));
        assert_eq!(lines[0].spans[1].content, "e\u{301}界");
        assert!(lines[0].spans[1]
            .style
            .add_modifier
            .contains(Modifier::REVERSED));
        assert_eq!(hits.bytes_at(7, 9, false), Some(base + start..base + wide));
        assert_eq!(hits.bytes_at(9, 9, false), Some(base + wide..selection.end));
        assert_eq!(hits.bytes_at(6, 9, false), None);
        assert_eq!(hits.bytes_at(7, 8, false), None);
    }
}
