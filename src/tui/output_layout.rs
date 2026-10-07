//! Pure, process-only layout for bounded tool output.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const COLLAPSED_VISUAL_ROWS: usize = 20;
const TAB_STOP: usize = 8;
const MAX_DESIRED_COLUMN_WIDTH: usize = 32;
const COLUMN_SEPARATOR: &str = " │ ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolOutputRowKind {
    Plain,
    TableHeader,
    TableRow,
    TableDivider,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolOutputRow {
    pub text: String,
    pub kind: ToolOutputRowKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolOutputLayout {
    pub rows: Vec<ToolOutputRow>,
    pub omitted_source_lines: usize,
    pub total_source_lines: usize,
}

#[derive(Debug)]
struct RowUnit {
    rows: Vec<ToolOutputRow>,
    source_lines: usize,
}

#[derive(Debug)]
enum SourceBlock<'a> {
    Plain(&'a str),
    Table(Vec<&'a str>),
}

pub(crate) fn layout_tool_output(output: &str, width: usize, expanded: bool) -> ToolOutputLayout {
    let safe = crate::terminal_text::sanitize_terminal_text(output);
    let source_lines = safe.lines().collect::<Vec<_>>();
    let total_source_lines = source_lines.len();
    let width = width.max(1);
    let mut units = Vec::new();
    for block in source_blocks(&source_lines) {
        match block {
            SourceBlock::Plain(line) => units.push(plain_unit(line, width, expanded)),
            SourceBlock::Table(lines) => {
                units.extend(table_units(&lines, width, expanded).unwrap_or_else(|| {
                    lines
                        .iter()
                        .map(|line| plain_unit(line, width, expanded))
                        .collect()
                }));
            }
        }
    }

    if expanded {
        return ToolOutputLayout {
            rows: units.into_iter().flat_map(|unit| unit.rows).collect(),
            omitted_source_lines: 0,
            total_source_lines,
        };
    }

    let mut rows = Vec::new();
    let mut consumed_source_lines = 0usize;
    for unit in units {
        if rows.len().saturating_add(unit.rows.len()) > COLLAPSED_VISUAL_ROWS {
            break;
        }
        rows.extend(unit.rows);
        consumed_source_lines = consumed_source_lines.saturating_add(unit.source_lines);
    }
    ToolOutputLayout {
        rows,
        omitted_source_lines: total_source_lines.saturating_sub(consumed_source_lines),
        total_source_lines,
    }
}

fn source_blocks<'a>(lines: &[&'a str]) -> Vec<SourceBlock<'a>> {
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let Some(columns) = tabular_arity(lines[index]) else {
            blocks.push(SourceBlock::Plain(lines[index]));
            index += 1;
            continue;
        };
        let end = lines[index + 1..]
            .iter()
            .position(|line| tabular_arity(line) != Some(columns))
            .map_or(lines.len(), |offset| index + 1 + offset);
        if end.saturating_sub(index) >= 2 {
            blocks.push(SourceBlock::Table(lines[index..end].to_vec()));
            index = end;
        } else {
            blocks.push(SourceBlock::Plain(lines[index]));
            index += 1;
        }
    }
    blocks
}

fn tabular_arity(line: &str) -> Option<usize> {
    let columns = line.split('\t').count();
    (columns >= 2).then_some(columns)
}

fn plain_unit(line: &str, width: usize, expanded: bool) -> RowUnit {
    let expanded_tabs = expand_tabs(line);
    let rows = if expanded {
        wrap_cells(&expanded_tabs, width)
    } else {
        vec![truncate_cells(&expanded_tabs, width)]
    };
    RowUnit {
        rows: rows
            .into_iter()
            .map(|text| ToolOutputRow {
                text,
                kind: ToolOutputRowKind::Plain,
            })
            .collect(),
        source_lines: 1,
    }
}

fn table_units(lines: &[&str], width: usize, expanded: bool) -> Option<Vec<RowUnit>> {
    let rows = lines
        .iter()
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let column_count = rows.first()?.len();
    let framing = COLUMN_SEPARATOR
        .width()
        .saturating_mul(column_count.saturating_sub(1));
    let available = width.checked_sub(framing)?;

    let has_header = looks_like_header(&rows[0]);
    let data_start = usize::from(has_header);
    let numeric = (0..column_count)
        .map(|column| {
            let values = rows[data_start..]
                .iter()
                .filter_map(|row| row.get(column).copied())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            !values.is_empty() && values.iter().all(|value| value.parse::<f64>().is_ok())
        })
        .collect::<Vec<_>>();

    let mut minimums = vec![1usize; column_count];
    let mut desired = vec![1usize; column_count];
    for row in &rows {
        for (column, cell) in row.iter().enumerate() {
            let natural = cell.width();
            let grapheme = cell
                .graphemes(true)
                .map(UnicodeWidthStr::width)
                .max()
                .unwrap_or(1)
                .max(1);
            minimums[column] = minimums[column].max(grapheme);
            desired[column] = desired[column].max(natural.min(MAX_DESIRED_COLUMN_WIDTH));
        }
    }
    let minimum_total = minimums.iter().sum::<usize>();
    if minimum_total > available {
        return None;
    }
    let mut widths = minimums;
    let mut remaining = available.saturating_sub(minimum_total);
    while remaining > 0 {
        let mut changed = false;
        for (column_width, desired_width) in widths.iter_mut().zip(&desired) {
            if *column_width < *desired_width && remaining > 0 {
                *column_width += 1;
                remaining -= 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut units = Vec::with_capacity(rows.len());
    for (row_index, row) in rows.iter().enumerate() {
        let wrapped = row
            .iter()
            .zip(&widths)
            .map(|(cell, column_width)| {
                if expanded {
                    wrap_cells(cell, *column_width)
                } else {
                    vec![truncate_cells(cell, *column_width)]
                }
            })
            .collect::<Vec<_>>();
        let row_height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
        let kind = if has_header && row_index == 0 {
            ToolOutputRowKind::TableHeader
        } else {
            ToolOutputRowKind::TableRow
        };
        let mut visual_rows = Vec::with_capacity(row_height + 1);
        for visual_index in 0..row_height {
            let cells = wrapped
                .iter()
                .enumerate()
                .map(|(column, fragments)| {
                    let fragment = fragments.get(visual_index).map_or("", String::as_str);
                    pad_cell(fragment, widths[column], numeric[column])
                })
                .collect::<Vec<_>>();
            visual_rows.push(ToolOutputRow {
                text: cells.join(COLUMN_SEPARATOR),
                kind,
            });
        }
        if has_header && row_index == 0 {
            visual_rows.push(ToolOutputRow {
                text: widths
                    .iter()
                    .map(|column_width| "─".repeat(*column_width))
                    .collect::<Vec<_>>()
                    .join("─┼─"),
                kind: ToolOutputRowKind::TableDivider,
            });
        }
        units.push(RowUnit {
            rows: visual_rows,
            source_lines: 1,
        });
    }
    Some(units)
}

fn looks_like_header(row: &[&str]) -> bool {
    row.iter().all(|cell| {
        let cell = cell.trim();
        !cell.is_empty()
            && cell.chars().any(char::is_alphabetic)
            && cell
                .chars()
                .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-'))
            && cell.parse::<f64>().is_err()
    })
}

fn expand_tabs(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut column = 0usize;
    for grapheme in line.graphemes(true) {
        if grapheme == "\t" {
            let spaces = TAB_STOP - (column % TAB_STOP);
            output.push_str(&" ".repeat(spaces));
            column += spaces;
        } else {
            output.push_str(grapheme);
            column += grapheme.width();
        }
    }
    output
}

pub(crate) fn wrap_cells(input: &str, width: usize) -> Vec<String> {
    if input.is_empty() {
        return vec![String::new()];
    }
    let mut rows = Vec::new();
    let mut current = String::new();
    let mut used = 0usize;
    for grapheme in input.graphemes(true) {
        let grapheme_width = grapheme.width();
        if grapheme_width > width {
            if !current.is_empty() {
                rows.push(std::mem::take(&mut current));
                used = 0;
            }
            rows.push("…".into());
            continue;
        }
        if !current.is_empty() && used.saturating_add(grapheme_width) > width {
            rows.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push_str(grapheme);
        used = used.saturating_add(grapheme_width);
    }
    if !current.is_empty() || rows.is_empty() {
        rows.push(current);
    }
    rows
}

fn truncate_cells(input: &str, width: usize) -> String {
    if input.width() <= width {
        return input.to_string();
    }
    if width == 1 {
        return "…".into();
    }
    let target = width - 1;
    let mut output = String::new();
    let mut used = 0usize;
    for grapheme in input.graphemes(true) {
        let next = grapheme.width();
        if used.saturating_add(next) > target {
            break;
        }
        output.push_str(grapheme);
        used += next;
    }
    output.push('…');
    output
}

fn pad_cell(input: &str, width: usize, right_aligned: bool) -> String {
    let padding = width.saturating_sub(input.width());
    if right_aligned {
        format!("{}{input}", " ".repeat(padding))
    } else {
        format!("{input}{}", " ".repeat(padding))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    const TSV: &str = concat!(
        "summary: highest ratios\n",
        "rank\tsite\tid\tname\tratio\tuploaded_bytes\tdata_size_bytes\tcumulative_size_bytes\tcurrent_upload_Bps\n",
        "1\tOPS\t11952\tAstral Projection – Power Gen EP [FLAC]\t2.001586952\t737125708\t368270640\t1004897068\t0\n",
        "2\tRED\t31873\t佐井好子 – 1976.6.29 ライブ\t1.520707435\t395325056\t259961283\t3779522457\t\n",
    );

    #[test]
    fn stable_tsv_is_bounded_aligned_and_lossless_when_expanded() {
        let layout = layout_tool_output(TSV, 176, true);
        assert_eq!(layout.total_source_lines, 4);
        assert_eq!(layout.omitted_source_lines, 0);
        assert!(layout.rows.iter().all(|row| row.text.width() <= 176));
        let visible = layout
            .rows
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for value in [
            "summary: highest ratios",
            "rank",
            "OPS",
            "Astral Projection",
            "2.001586952",
            "佐井好子",
            "1.520707435",
        ] {
            assert!(visible.contains(value), "missing {value:?}:\n{visible}");
        }
        assert!(visible.lines().filter(|line| line.contains('│')).count() >= 3);
        let first_data_row = layout
            .rows
            .iter()
            .find(|row| row.text.contains("OPS"))
            .unwrap();
        assert!(first_data_row.text.starts_with("   1 │ OPS"));
    }

    #[test]
    fn isolated_and_inconsistent_tabs_use_eight_cell_stops() {
        let layout = layout_tool_output("a\tb\nleft\tone\ttwo\nright\tonly", 40, true);
        assert_eq!(layout.rows[0].text, "a       b");
        assert!(!layout.rows.iter().any(|row| row.text.contains('│')));
        assert!(layout.rows.iter().all(|row| row.text.width() <= 40));
    }

    #[test]
    fn collapsed_preview_keeps_complete_rows_and_reports_omissions() {
        let input = (0..30)
            .map(|index| format!("row {index}: {}", "界".repeat(24)))
            .collect::<Vec<_>>()
            .join("\n");
        let layout = layout_tool_output(&input, 32, false);
        assert!(layout.rows.len() <= 20);
        assert!(layout.omitted_source_lines > 0);
        assert!(layout.rows.iter().all(|row| row.text.width() <= 32));
    }

    #[test]
    fn impossible_table_width_falls_back_without_losing_content() {
        let input = "a\tb\tc\td\te\tf\n1\t2\t3\t4\t5\t6";
        let layout = layout_tool_output(input, 5, true);
        let visible = layout
            .rows
            .iter()
            .map(|row| row.text.as_str())
            .collect::<String>();
        assert!(layout.rows.iter().all(|row| row.text.width() <= 5));
        for value in ['a', 'b', 'c', 'd', 'e', 'f', '1', '2', '3', '4', '5', '6'] {
            assert!(visible.contains(value));
        }
    }

    #[test]
    fn terminal_controls_are_removed_before_layout() {
        let layout = layout_tool_output("\u{1b}[31mred\u{1b}[0m\t1\nblue\t2\u{7}", 24, true);
        let visible = layout
            .rows
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible.contains("red"));
        assert!(visible.contains("blue"));
        assert!(!visible.contains('\u{1b}'));
        assert!(!visible.contains('\u{7}'));
    }
}
