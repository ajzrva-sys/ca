//! Shared selection viewport and Unicode-safe row helpers.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListViewport {
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionRowLayout {
    pub label: String,
    pub gap: String,
    pub description: String,
    pub stacked: bool,
}

/// Lay out a picker label and its secondary description by terminal display
/// cells. Narrow surfaces stack the description; wider surfaces reserve a
/// stable label column so rows remain easy to scan.
pub fn layout_row(
    label: &str,
    description: &str,
    width: usize,
    stacked: bool,
) -> SelectionRowLayout {
    if stacked {
        return SelectionRowLayout {
            label: truncate_cells(label, width),
            gap: String::new(),
            description: truncate_cells(&format!("    {description}"), width),
            stacked: true,
        };
    }
    let label_width = (width / 2).max(12).min(width);
    let label = truncate_cells(label, label_width);
    let used = UnicodeWidthStr::width(label.as_str());
    let gap = " ".repeat(label_width.saturating_sub(used).saturating_add(1));
    let description = truncate_cells(
        description,
        width.saturating_sub(label_width.saturating_add(1)),
    );
    SelectionRowLayout {
        label,
        gap,
        description,
        stacked: false,
    }
}

impl ListViewport {
    pub fn visible(self, selected: usize, count: usize, rows: usize) -> Range<usize> {
        if count == 0 || rows == 0 {
            return 0..0;
        }
        let selected = selected.min(count - 1);
        let maximum = count.saturating_sub(rows);
        let mut offset = self.offset.min(maximum);
        if selected < offset {
            offset = selected;
        } else if selected >= offset + rows {
            offset = selected + 1 - rows;
        }
        offset..(offset + rows).min(count)
    }
}

pub fn previous_wrap(selected: usize, count: usize) -> usize {
    if count == 0 || selected == 0 {
        count.saturating_sub(1)
    } else {
        selected - 1
    }
}

pub fn next_wrap(selected: usize, count: usize) -> usize {
    if count == 0 {
        0
    } else {
        (selected + 1) % count
    }
}

pub fn page_up(selected: usize, rows: usize) -> usize {
    selected.saturating_sub(rows.max(1))
}

pub fn page_down(selected: usize, count: usize, rows: usize) -> usize {
    selected
        .saturating_add(rows.max(1))
        .min(count.saturating_sub(1))
}

pub fn truncate_cells(value: &str, width: usize) -> String {
    if UnicodeWidthStr::width(value) <= width {
        return value.to_string();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".into();
    }
    let target = width - 1;
    let mut result = String::new();
    let mut used = 0;
    for grapheme in value.graphemes(true) {
        let next = UnicodeWidthStr::width(grapheme);
        if used + next > target {
            break;
        }
        result.push_str(grapheme);
        used += next;
    }
    result.push('…');
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_wraps_pages_and_keeps_selection_visible() {
        assert_eq!(previous_wrap(0, 4), 3);
        assert_eq!(next_wrap(3, 4), 0);
        assert_eq!(page_up(2, 5), 0);
        assert_eq!(page_down(2, 10, 5), 7);
        assert_eq!(ListViewport { offset: 0 }.visible(8, 10, 4), 5..9);
    }

    #[test]
    fn truncation_preserves_graphemes_and_display_width() {
        let truncated = truncate_cells("e\u{301}你abc", 5);
        assert_eq!(truncated, "e\u{301}你a…");
        assert_eq!(UnicodeWidthStr::width(truncated.as_str()), 5);
    }

    #[test]
    fn rows_stack_descriptions_on_narrow_surfaces() {
        let wide = layout_row("/provider", "configure providers", 40, false);
        assert!(!wide.stacked);
        assert_eq!(
            UnicodeWidthStr::width(
                format!("{}{}{}", wide.label, wide.gap, wide.description).as_str()
            ),
            40
        );
        let narrow = layout_row("/provider", "configure providers", 18, true);
        assert!(narrow.stacked);
        assert!(narrow.description.starts_with("    "));
    }
}
