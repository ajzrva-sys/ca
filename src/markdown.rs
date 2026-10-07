use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::sync::LazyLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

#[derive(Debug, Clone, Copy)]
pub struct MarkdownPalette {
    pub text: Color,
    pub heading: Color,
    pub heading_text: Color,
    pub accent: Color,
    pub link: Color,
    pub strong: Color,
    pub muted: Color,
    pub border: Color,
    pub code_background: Color,
    pub md_h1: Color,
    pub md_h2: Color,
    pub md_h3: Color,
    pub md_h4: Color,
    pub md_h5: Color,
    pub md_h6: Color,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownLink {
    pub label: String,
    pub destination: String,
}

#[derive(Debug, Clone)]
pub struct MarkdownDocument {
    pub lines: Vec<Line<'static>>,
    pub links: Vec<MarkdownLink>,
}

impl Default for MarkdownPalette {
    fn default() -> Self {
        Self {
            text: Color::White,
            heading: Color::Cyan,
            heading_text: Color::White,
            accent: Color::Yellow,
            link: Color::Blue,
            strong: Color::White,
            muted: Color::DarkGray,
            border: Color::DarkGray,
            code_background: Color::DarkGray,
            md_h1: Color::Cyan,
            md_h2: Color::Cyan,
            md_h3: Color::LightCyan,
            md_h4: Color::LightCyan,
            md_h5: Color::Gray,
            md_h6: Color::Gray,
        }
    }
}

#[derive(Debug)]
struct ListState {
    next: Option<u64>,
}

#[cfg(test)]
pub fn render_markdown(text: &str, max_width: u16, show_line_numbers: bool) -> Vec<Line<'static>> {
    render_markdown_document_with_palette(
        text,
        max_width,
        show_line_numbers,
        MarkdownPalette::default(),
    )
    .lines
}

#[cfg(test)]
fn render_markdown_with_palette(
    text: &str,
    max_width: u16,
    show_line_numbers: bool,
    palette: MarkdownPalette,
) -> Vec<Line<'static>> {
    render_markdown_document_with_palette(text, max_width, show_line_numbers, palette).lines
}

pub fn render_markdown_document_with_palette(
    text: &str,
    max_width: u16,
    show_line_numbers: bool,
    palette: MarkdownPalette,
) -> MarkdownDocument {
    let mut lines: Vec<Line> = Vec::new();
    let mut links = Vec::new();
    let mut current_spans: Vec<Span> = Vec::new();
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buf = String::new();
    let mut in_bold = false;
    let mut in_italic = false;
    let mut in_heading = false;
    let mut current_link: Option<(String, String)> = None;
    let mut in_table = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_cell = String::new();
    let mut lists: Vec<ListState> = Vec::new();

    let normalized = normalize_math_delimiters(text);
    let parser = Parser::new_ext(&normalized, Options::ENABLE_TABLES | Options::ENABLE_MATH);

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { level, .. } => {
                    flush_inline(&mut lines, &mut current_spans);
                    in_heading = true;
                    let (prefix, heading_color) = match level {
                        pulldown_cmark::HeadingLevel::H1 => ("### ", palette.md_h1),
                        pulldown_cmark::HeadingLevel::H2 => ("## ", palette.md_h2),
                        pulldown_cmark::HeadingLevel::H3 => ("# ", palette.md_h3),
                        pulldown_cmark::HeadingLevel::H4 => ("#### ", palette.md_h4),
                        pulldown_cmark::HeadingLevel::H5 => ("##### ", palette.md_h5),
                        pulldown_cmark::HeadingLevel::H6 => ("###### ", palette.md_h6),
                    };
                    current_spans.push(Span::styled(
                        prefix,
                        Style::default()
                            .fg(heading_color)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
                Tag::CodeBlock(kind) => {
                    flush_inline(&mut lines, &mut current_spans);
                    in_code_block = true;
                    code_buf.clear();
                    if let CodeBlockKind::Fenced(lang) = kind {
                        code_lang = lang.to_string();
                    }
                }
                Tag::Emphasis => {
                    in_italic = true;
                }
                Tag::Strong => {
                    in_bold = true;
                }
                Tag::Link { dest_url, .. } => {
                    current_link = Some((dest_url.to_string(), String::new()));
                }
                Tag::List(start) => lists.push(ListState { next: start }),
                Tag::Item => {
                    flush_inline(&mut lines, &mut current_spans);
                    let depth = lists.len().max(1);
                    let indent = "  ".repeat(depth);
                    let marker = lists.last_mut().map_or_else(
                        || "• ".to_string(),
                        |list| match list.next.as_mut() {
                            Some(next) => {
                                let marker = format!("{next}. ");
                                *next = next.saturating_add(1);
                                marker
                            }
                            None => "• ".to_string(),
                        },
                    );
                    current_spans.push(Span::styled(
                        format!("{indent}{marker}"),
                        Style::default()
                            .fg(palette.accent)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
                Tag::Table(_) => {
                    flush_inline(&mut lines, &mut current_spans);
                    in_table = true;
                    table_rows.clear();
                }
                Tag::TableHead => {
                    current_row.clear();
                }
                Tag::TableCell => {
                    current_cell.clear();
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => {
                    if in_table {
                        // Don't flush inline during table — we're collecting cells
                    } else {
                        flush_inline(&mut lines, &mut current_spans);
                        lines.push(Line::from(""));
                    }
                }
                TagEnd::Heading(_) => {
                    flush_inline(&mut lines, &mut current_spans);
                    in_heading = false;
                    lines.push(Line::from(""));
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    render_code_block(
                        &mut lines,
                        &code_buf,
                        &code_lang,
                        max_width,
                        show_line_numbers,
                        palette,
                    );
                    code_buf.clear();
                    code_lang.clear();
                }
                TagEnd::Emphasis => {
                    in_italic = false;
                }
                TagEnd::Strong => {
                    in_bold = false;
                }
                TagEnd::Item => {
                    flush_inline(&mut lines, &mut current_spans);
                }
                TagEnd::List(_) => {
                    flush_inline(&mut lines, &mut current_spans);
                    lists.pop();
                    if lists.is_empty() && lines.last().is_some_and(|line| !line.spans.is_empty()) {
                        lines.push(Line::from(""));
                    }
                }
                TagEnd::Link => {
                    if let Some((destination, label)) = current_link.take() {
                        if !label.is_empty() {
                            links.push(MarkdownLink { label, destination });
                        }
                    }
                }
                TagEnd::Table => {
                    render_table(&mut lines, &table_rows, max_width, palette);
                    table_rows.clear();
                    in_table = false;
                }
                TagEnd::TableHead => {
                    table_rows.push(std::mem::take(&mut current_row));
                }
                TagEnd::TableRow => {
                    if !current_row.is_empty() {
                        table_rows.push(std::mem::take(&mut current_row));
                    }
                }
                TagEnd::TableCell => {
                    current_row.push(std::mem::take(&mut current_cell));
                }
                _ => {}
            },
            Event::Text(text) => {
                if let Some((_, label)) = current_link.as_mut() {
                    label.push_str(&text);
                }
                if in_code_block {
                    code_buf.push_str(&text);
                } else if in_table {
                    current_cell.push_str(&text);
                } else {
                    let mut style = Style::default().fg(palette.text);
                    if in_heading {
                        style = style.fg(palette.heading_text).add_modifier(Modifier::BOLD);
                    }
                    if in_bold {
                        style = style.fg(palette.strong).add_modifier(Modifier::BOLD);
                    }
                    if in_italic {
                        style = style.add_modifier(Modifier::ITALIC);
                    }
                    if current_link.is_some() {
                        style = style.fg(palette.link).add_modifier(Modifier::UNDERLINED);
                    }
                    for (i, part) in text.split('\n').enumerate() {
                        if i > 0 {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                        if !part.is_empty() {
                            current_spans.push(Span::styled(part.to_string(), style));
                        }
                    }
                }
            }
            Event::Code(text) => {
                if let Some((_, label)) = current_link.as_mut() {
                    label.push_str(&text);
                }
                if in_table {
                    current_cell.push_str(&text);
                } else {
                    let mut style = Style::default()
                        .fg(palette.accent)
                        .bg(palette.code_background);
                    if current_link.is_some() {
                        style = style.fg(palette.link).add_modifier(Modifier::UNDERLINED);
                    }
                    current_spans.push(Span::styled(text.to_string(), style));
                }
            }
            Event::InlineMath(text) => {
                let currency = is_currency_literal(&text);
                let rendered = if currency {
                    format!("${text}$")
                } else {
                    convert_math(&text)
                };
                if let Some((_, label)) = current_link.as_mut() {
                    label.push_str(&rendered);
                }
                if in_table {
                    current_cell.push_str(&rendered);
                } else {
                    let mut style = if currency {
                        Style::default().fg(palette.text)
                    } else {
                        Style::default().fg(palette.heading)
                    };
                    if in_bold {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    if in_italic {
                        style = style.add_modifier(Modifier::ITALIC);
                    }
                    if current_link.is_some() {
                        style = style.fg(palette.link).add_modifier(Modifier::UNDERLINED);
                    }
                    current_spans.push(Span::styled(rendered, style));
                }
            }
            Event::DisplayMath(text) => {
                let rendered = convert_math(&text);
                if in_table {
                    current_cell.push_str(&rendered);
                } else {
                    flush_inline(&mut lines, &mut current_spans);
                    for line in rendered.lines() {
                        lines.push(Line::from(Span::styled(
                            format!("  {line}"),
                            Style::default().fg(palette.heading),
                        )));
                    }
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_table {
                    current_cell.push(' ');
                } else {
                    lines.push(Line::from(std::mem::take(&mut current_spans)));
                }
            }
            Event::Rule => {
                flush_inline(&mut lines, &mut current_spans);
                lines.push(Line::from(Span::styled(
                    "─".repeat(40),
                    Style::default().fg(palette.border),
                )));
            }
            _ => {}
        }
    }

    flush_inline(&mut lines, &mut current_spans);
    MarkdownDocument { lines, links }
}

fn flush_inline<'a>(lines: &mut Vec<Line<'a>>, spans: &mut Vec<Span<'a>>) {
    if !spans.is_empty() {
        lines.push(Line::from(std::mem::take(spans)));
    }
}

fn render_code_block(
    lines: &mut Vec<Line>,
    code_buf: &str,
    code_lang: &str,
    max_width: u16,
    show_line_numbers: bool,
    palette: MarkdownPalette,
) {
    if code_lang.eq_ignore_ascii_case("mermaid") {
        if let crate::mermaid::MermaidRender::Ready(rendered) =
            crate::mermaid::render_or_queue(code_buf, max_width.saturating_sub(4))
        {
            lines.push(Line::from(Span::styled(
                "  Mermaid",
                Style::default()
                    .fg(palette.muted)
                    .add_modifier(Modifier::DIM),
            )));
            for line in rendered.lines() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(line.to_string(), Style::default().fg(palette.text)),
                ]));
            }
            lines.push(Line::from(""));
            return;
        }
    }
    let width = 60.min(max_width.saturating_sub(4) as usize);
    let border = "─".repeat(width);
    lines.push(Line::from(Span::styled(
        format!("  ┌{}┐", border),
        Style::default().fg(palette.border),
    )));
    if !code_lang.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  │ {} ", code_lang),
            Style::default()
                .fg(palette.muted)
                .add_modifier(Modifier::DIM),
        )));
    }
    let syntax = SYNTAX_SET
        .find_syntax_by_token(code_lang)
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
    let theme = &THEME_SET.themes["base16-ocean.dark"];
    let mut highlighter = HighlightLines::new(syntax, theme);
    let total = code_buf.lines().count();
    let num_width = if show_line_numbers {
        total.to_string().len()
    } else {
        0
    };
    for (idx, code_line) in LinesWithEndings::from(code_buf).enumerate() {
        let trimmed = code_line.trim_end_matches('\n');
        if let Ok(highlighted) = highlighter.highlight_line(trimmed, &SYNTAX_SET) {
            let mut spans: Vec<Span> =
                vec![Span::styled("  │ ", Style::default().fg(palette.border))];
            if show_line_numbers {
                spans.push(Span::styled(
                    format!("{:>width$} │ ", idx + 1, width = num_width),
                    Style::default()
                        .fg(palette.muted)
                        .add_modifier(Modifier::DIM),
                ));
            }
            for (style, text) in highlighted {
                let color = syntect_to_ratatui(style.foreground);
                spans.push(Span::styled(text.to_string(), Style::default().fg(color)));
            }
            lines.push(Line::from(spans));
        } else {
            let mut spans: Vec<Span> =
                vec![Span::styled("  │ ", Style::default().fg(palette.border))];
            if show_line_numbers {
                spans.push(Span::styled(
                    format!("{:>width$} │ ", idx + 1, width = num_width),
                    Style::default()
                        .fg(palette.muted)
                        .add_modifier(Modifier::DIM),
                ));
            }
            spans.push(Span::styled(
                trimmed.to_string(),
                Style::default().fg(palette.text),
            ));
            lines.push(Line::from(spans));
        }
    }
    lines.push(Line::from(Span::styled(
        format!("  └{}┘", border),
        Style::default().fg(palette.border),
    )));
    lines.push(Line::from(""));
}

fn syntect_to_ratatui(color: syntect::highlighting::Color) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

fn normalize_math_delimiters(input: &str) -> String {
    let protected = code_protection_mask(input);
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut i = 0;

    while i < bytes.len() {
        if protected[i] || bytes[i] != b'\\' {
            let ch = input[i..].chars().next().expect("valid UTF-8 boundary");
            output.push(ch);
            i += ch.len_utf8();
            continue;
        }

        let slash_start = i;
        while i < bytes.len() && bytes[i] == b'\\' && !protected[i] {
            i += 1;
        }
        let slash_count = i - slash_start;
        let Some(&delimiter) = bytes.get(i) else {
            output.push_str(&input[slash_start..i]);
            break;
        };
        let closing = match delimiter {
            b'(' => b')',
            b'[' => b']',
            b')' | b']' if slash_count % 2 == 1 => {
                output.push_str(&input[slash_start..i]);
                output.push('\\');
                continue;
            }
            _ => {
                output.push_str(&input[slash_start..i]);
                continue;
            }
        };
        if slash_count % 2 == 0 {
            output.push_str(&input[slash_start..i]);
            continue;
        }

        let body_start = i + 1;
        let Some((close_start, close_end)) =
            find_unescaped_math_close(input, &protected, body_start, closing)
        else {
            output.push_str(&input[slash_start..i]);
            output.push_str(&input[i - 1..body_start]);
            i = body_start;
            continue;
        };

        output.push_str(&input[slash_start..i - 1]);
        if delimiter == b'[' {
            output.push_str("$$");
        } else {
            output.push('$');
        }
        output.push_str(&input[body_start..close_start]);
        if delimiter == b'[' {
            output.push_str("$$");
        } else {
            output.push('$');
        }
        i = close_end;
    }

    output
}

fn find_unescaped_math_close(
    input: &str,
    protected: &[bool],
    mut i: usize,
    closing: u8,
) -> Option<(usize, usize)> {
    let bytes = input.as_bytes();
    while i < bytes.len() {
        if protected[i] || bytes[i] != b'\\' {
            i += input[i..].chars().next()?.len_utf8();
            continue;
        }
        let slash_start = i;
        while i < bytes.len() && bytes[i] == b'\\' && !protected[i] {
            i += 1;
        }
        if (i - slash_start) % 2 == 1 && bytes.get(i) == Some(&closing) {
            return Some((i - 1, i + 1));
        }
    }
    None
}

fn code_protection_mask(input: &str) -> Vec<bool> {
    let mut protected = vec![false; input.len()];
    let mut fence: Option<(u8, usize)> = None;
    let mut offset = 0;

    for line in input.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let marker = fence_marker(content);
        let in_fence = fence.is_some();
        let is_close =
            fence
                .zip(marker)
                .is_some_and(|((kind, minimum), (candidate, count, rest))| {
                    kind == candidate && count >= minimum && rest.trim().is_empty()
                });
        let is_open = fence.is_none() && marker.is_some();
        let is_indented_code =
            fence.is_none() && (content.starts_with("    ") || content.starts_with('\t'));
        if in_fence || is_open || is_indented_code {
            protected[offset..offset + line.len()].fill(true);
        }
        if is_close {
            fence = None;
        } else if is_open {
            let (kind, count, _) = marker.expect("checked above");
            fence = Some((kind, count));
        }
        offset += line.len();
    }

    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if protected[i] || bytes[i] != b'`' {
            i += input[i..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] == b'`' && !protected[i] {
            i += 1;
        }
        let run = i - start;
        let mut candidate = i;
        let mut close = None;
        while candidate < bytes.len() {
            if protected[candidate] || bytes[candidate] != b'`' {
                candidate += input[candidate..].chars().next().map_or(1, char::len_utf8);
                continue;
            }
            let close_start = candidate;
            while candidate < bytes.len() && bytes[candidate] == b'`' && !protected[candidate] {
                candidate += 1;
            }
            if candidate - close_start == run {
                close = Some(candidate);
                break;
            }
        }
        if let Some(end) = close {
            protected[start..end].fill(true);
            i = end;
        }
    }

    protected
}

fn fence_marker(line: &str) -> Option<(u8, usize, &str)> {
    let leading = line.bytes().take_while(|byte| *byte == b' ').count();
    if leading > 3 {
        return None;
    }
    let bytes = line.as_bytes();
    let kind = *bytes.get(leading)?;
    if kind != b'`' && kind != b'~' {
        return None;
    }
    let count = bytes[leading..]
        .iter()
        .take_while(|byte| **byte == kind)
        .count();
    (count >= 3).then(|| (kind, count, &line[leading + count..]))
}

fn is_numeric_currency(body: &str) -> bool {
    let mut separator = None;
    let mut fractional = 0;
    for ch in body.chars() {
        if ch.is_ascii_digit() {
            if separator.is_some() {
                fractional += 1;
            }
        } else if matches!(ch, '.' | ',') && separator.is_none() {
            separator = Some(ch);
        } else {
            return false;
        }
    }
    !body.is_empty() && separator.is_none_or(|_| (1..=2).contains(&fractional))
}

fn is_currency_literal(body: &str) -> bool {
    is_numeric_currency(body)
        || (body.chars().next().is_some_and(|ch| ch.is_ascii_digit())
            && body.chars().last().is_some_and(char::is_whitespace))
}

fn convert_math(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let ch = input[i..].chars().next().expect("valid UTF-8 boundary");
        match ch {
            '\\' => {
                let command_start = i;
                i += 1;
                let name_start = i;
                while i < input.len() && input.as_bytes()[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let name = &input[name_start..i];
                if name.is_empty() {
                    let Some(symbol) = input[i..].chars().next() else {
                        output.push('\\');
                        break;
                    };
                    match symbol {
                        ',' | ':' | ';' => output.push(' '),
                        '!' => {}
                        _ => {
                            output.push('\\');
                            output.push(symbol);
                        }
                    }
                    i += symbol.len_utf8();
                } else if name == "sqrt" {
                    let converted = braced_operand(input, i).and_then(|(operand, end)| {
                        safe_atomic_operand(operand).map(|converted| (converted, end))
                    });
                    if let Some((converted, end)) = converted {
                        output.push('√');
                        output.push_str(&converted);
                        i = end;
                    } else {
                        output.push_str(&input[command_start..i]);
                    }
                } else if name == "frac" {
                    let converted = braced_operand(input, i).and_then(|(numerator, first_end)| {
                        braced_operand(input, first_end).and_then(|(denominator, second_end)| {
                            safe_atomic_operand(numerator)
                                .zip(safe_atomic_operand(denominator))
                                .map(|(numerator, denominator)| {
                                    (numerator, denominator, second_end)
                                })
                        })
                    });
                    if let Some((numerator, denominator, second_end)) = converted {
                        output.push_str(&numerator);
                        output.push('⁄');
                        output.push_str(&denominator);
                        i = second_end;
                    } else {
                        output.push_str(&input[command_start..i]);
                    }
                } else if matches!(name, "left" | "right") {
                    match input[i..].chars().next() {
                        Some(delimiter)
                            if matches!(delimiter, '(' | ')' | '[' | ']' | '{' | '}' | '|') =>
                        {
                            output.push(delimiter);
                            i += delimiter.len_utf8();
                        }
                        _ => output.push_str(&input[command_start..i]),
                    }
                } else if name == "quad" {
                    output.push(' ');
                } else if name == "qquad" {
                    output.push_str("  ");
                } else if let Some(replacement) = math_command(name) {
                    output.push_str(replacement);
                } else {
                    output.push_str(&input[command_start..i]);
                }
            }
            '^' | '_' => {
                let marker_start = i;
                i += 1;
                let (body, end) = if input.as_bytes().get(i) == Some(&b'{') {
                    match braced_operand(input, i) {
                        Some(value) => value,
                        None => {
                            output.push(ch);
                            continue;
                        }
                    }
                } else if let Some(next) = input[i..].chars().next() {
                    (&input[i..i + next.len_utf8()], i + next.len_utf8())
                } else {
                    output.push(ch);
                    continue;
                };
                let converted = if ch == '^' {
                    convert_script(body, superscript_char)
                } else {
                    convert_script(body, subscript_char)
                };
                if let Some(converted) = converted {
                    output.push_str(&converted);
                } else {
                    output.push_str(&input[marker_start..end]);
                }
                i = end;
            }
            _ => {
                output.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    output
}

fn braced_operand(input: &str, start: usize) -> Option<(&str, usize)> {
    if input.as_bytes().get(start) != Some(&b'{') {
        return None;
    }
    let close = input[start + 1..].find('}')? + start + 1;
    let body = &input[start + 1..close];
    (!body.contains(['{', '}'])).then_some((body, close + 1))
}

fn safe_atomic_operand(input: &str) -> Option<String> {
    let mut chars = input.chars();
    if let Some(ch) = chars.next() {
        if chars.next().is_none() && ch.is_alphanumeric() {
            return Some(ch.to_string());
        }
    }
    let name = input.strip_prefix('\\')?;
    let replacement = math_command(name)?;
    let mut chars = replacement.chars();
    let ch = chars.next()?;
    (chars.next().is_none() && is_greek(ch)).then(|| ch.to_string())
}

fn is_greek(ch: char) -> bool {
    matches!(ch as u32, 0x0370..=0x03ff)
}

fn convert_script(input: &str, map: fn(char) -> Option<char>) -> Option<String> {
    input.chars().map(map).collect()
}

fn superscript_char(ch: char) -> Option<char> {
    Some(match ch {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '-' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'i' => 'ⁱ',
        'n' => 'ⁿ',
        _ => return None,
    })
}

fn subscript_char(ch: char) -> Option<char> {
    Some(match ch {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'h' => 'ₕ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'k' => 'ₖ',
        'l' => 'ₗ',
        'm' => 'ₘ',
        'n' => 'ₙ',
        'o' => 'ₒ',
        'p' => 'ₚ',
        'r' => 'ᵣ',
        's' => 'ₛ',
        't' => 'ₜ',
        'u' => 'ᵤ',
        'v' => 'ᵥ',
        'x' => 'ₓ',
        _ => return None,
    })
}

fn math_command(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ε",
        "varepsilon" => "ϵ",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "vartheta" => "ϑ",
        "iota" => "ι",
        "kappa" => "κ",
        "varkappa" => "ϰ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "omicron" => "ο",
        "pi" => "π",
        "varpi" => "ϖ",
        "rho" => "ρ",
        "varrho" => "ϱ",
        "sigma" => "σ",
        "varsigma" => "ς",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "φ",
        "varphi" => "ϕ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Alpha" => "Α",
        "Beta" => "Β",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Epsilon" => "Ε",
        "Zeta" => "Ζ",
        "Eta" => "Η",
        "Theta" => "Θ",
        "Iota" => "Ι",
        "Kappa" => "Κ",
        "Lambda" => "Λ",
        "Mu" => "Μ",
        "Nu" => "Ν",
        "Xi" => "Ξ",
        "Omicron" => "Ο",
        "Pi" => "Π",
        "Rho" => "Ρ",
        "Sigma" => "Σ",
        "Tau" => "Τ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Chi" => "Χ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "pm" => "±",
        "mp" => "∓",
        "times" => "×",
        "div" => "÷",
        "cdot" => "·",
        "ast" => "∗",
        "le" | "leq" => "≤",
        "ge" | "geq" => "≥",
        "ne" | "neq" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "in" => "∈",
        "notin" => "∉",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "supset" => "⊃",
        "supseteq" => "⊇",
        "cup" => "∪",
        "cap" => "∩",
        "leftarrow" | "gets" => "←",
        "rightarrow" | "to" => "→",
        "leftrightarrow" => "↔",
        "Leftarrow" => "⇐",
        "Rightarrow" => "⇒",
        "Leftrightarrow" => "⇔",
        "longleftarrow" => "⟵",
        "longrightarrow" => "⟶",
        "longleftrightarrow" => "⟷",
        "Longleftarrow" => "⟸",
        "Longrightarrow" => "⟹",
        "Longleftrightarrow" => "⟺",
        "uparrow" => "↑",
        "downarrow" => "↓",
        "updownarrow" => "↕",
        "Uparrow" => "⇑",
        "Downarrow" => "⇓",
        "Updownarrow" => "⇕",
        "mapsto" => "↦",
        "hookleftarrow" => "↩",
        "hookrightarrow" => "↪",
        "nearrow" => "↗",
        "searrow" => "↘",
        "swarrow" => "↙",
        "nwarrow" => "↖",
        "sum" => "∑",
        "prod" => "∏",
        "int" => "∫",
        "partial" => "∂",
        "nabla" => "∇",
        "infty" => "∞",
        _ => return None,
    })
}

fn render_table(
    lines: &mut Vec<Line>,
    rows: &[Vec<String>],
    max_width: u16,
    palette: MarkdownPalette,
) {
    if rows.is_empty() {
        return;
    }

    let num_cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if num_cols == 0 {
        return;
    }
    let mut natural_widths = vec![0usize; num_cols];
    let mut minimum_widths = vec![1usize; num_cols];
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            natural_widths[index] =
                natural_widths[index].max(UnicodeWidthStr::width(cell.as_str()));
            minimum_widths[index] = minimum_widths[index].max(
                cell.graphemes(true)
                    .map(UnicodeWidthStr::width)
                    .max()
                    .unwrap_or(1),
            );
        }
    }
    let framing = 3usize.saturating_mul(num_cols).saturating_add(3);
    let available_content = usize::from(max_width).saturating_sub(framing);
    let minimum_content = minimum_widths.iter().copied().sum::<usize>();
    let readable_content = num_cols.saturating_mul(3);
    if framing > usize::from(max_width)
        || minimum_content > available_content
        || readable_content > available_content
    {
        render_stacked_table(lines, rows, max_width, palette);
        return;
    }

    let desired_widths = natural_widths
        .iter()
        .zip(&minimum_widths)
        .map(|(natural, minimum)| (*natural).clamp(*minimum, 32))
        .collect::<Vec<_>>();
    let mut col_widths = minimum_widths;
    let mut remaining = available_content.saturating_sub(minimum_content);
    while remaining > 0 {
        let mut expanded = false;
        for (width, desired) in col_widths.iter_mut().zip(&desired_widths) {
            if *width < *desired && remaining > 0 {
                *width += 1;
                remaining -= 1;
                expanded = true;
            }
        }
        if !expanded {
            break;
        }
    }

    let border = |left: char, join: char, right: char| {
        let cells = col_widths
            .iter()
            .map(|width| "─".repeat(width.saturating_add(2)))
            .collect::<Vec<_>>()
            .join(&join.to_string());
        Line::from(Span::styled(
            format!("  {left}{cells}{right}"),
            Style::default().fg(palette.border),
        ))
    };
    lines.push(border('┌', '┬', '┐'));

    for (row_idx, row) in rows.iter().enumerate() {
        let wrapped_cells = col_widths
            .iter()
            .copied()
            .enumerate()
            .map(|(column, width)| wrap_to_width(row.get(column).map_or("", String::as_str), width))
            .collect::<Vec<_>>();
        let row_height = wrapped_cells.iter().map(Vec::len).max().unwrap_or(1);
        let style = if row_idx == 0 {
            Style::default()
                .fg(palette.heading)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.text)
        };

        for visual_row in 0..row_height {
            let mut spans: Vec<Span> = vec![
                Span::raw("  "),
                Span::styled("│", Style::default().fg(palette.border)),
            ];
            for (column, width) in col_widths.iter().copied().enumerate() {
                let fragment = wrapped_cells[column]
                    .get(visual_row)
                    .map_or("", String::as_str);
                let padding = width.saturating_sub(UnicodeWidthStr::width(fragment));
                let padded = format!(" {fragment}{} ", " ".repeat(padding));
                spans.push(Span::styled(padded, style));
                spans.push(Span::styled("│", Style::default().fg(palette.border)));
            }
            lines.push(Line::from(spans));
        }
        if row_idx == 0 && rows.len() > 1 {
            lines.push(border('├', '┼', '┤'));
        }
    }
    lines.push(border('└', '┴', '┘'));
    lines.push(Line::from(""));
}

fn render_stacked_table(
    lines: &mut Vec<Line>,
    rows: &[Vec<String>],
    max_width: u16,
    palette: MarkdownPalette,
) {
    let max_width = usize::from(max_width);
    let content_indent = if max_width >= 4 { "    " } else { "" };
    let content_width = max_width
        .saturating_sub(UnicodeWidthStr::width(content_indent))
        .max(1);
    for (row_index, row) in rows.iter().enumerate() {
        let row_label = if row_index == 0 {
            "  Columns".to_string()
        } else {
            format!("  Row {row_index}")
        };
        lines.push(Line::from(Span::styled(
            row_label,
            Style::default()
                .fg(palette.heading)
                .add_modifier(Modifier::BOLD),
        )));
        for (column_index, cell) in row.iter().enumerate() {
            lines.push(Line::from(Span::styled(
                format!("  [{}]", column_index + 1),
                Style::default().fg(palette.muted),
            )));
            let style = if row_index == 0 {
                Style::default()
                    .fg(palette.heading)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette.text)
            };
            for fragment in wrap_to_width(cell, content_width) {
                lines.push(Line::from(vec![
                    Span::raw(content_indent),
                    Span::styled(fragment, style),
                ]));
            }
        }
    }
    lines.push(Line::from(""));
}

fn wrap_to_width(input: &str, maximum: usize) -> Vec<String> {
    if input.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width: usize = 0;
    for grapheme in input.graphemes(true) {
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if !current.is_empty() && current_width.saturating_add(grapheme_width) > maximum {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push_str(grapheme);
        current_width = current_width.saturating_add(grapheme_width);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered_text(markdown: &str) -> String {
        render_markdown(markdown, 100, false)
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn plan_list_end_separates_the_following_paragraph() {
        for input in [
            "- tests\n\nApprove the plan.",
            "1. tests\n\nApprove the plan.",
            "- parent\n  - tests\n\nApprove the plan.",
        ] {
            let rendered = rendered_text(input);
            assert!(!rendered.contains("testsApprove"), "{rendered}");
            assert!(
                rendered.lines().any(|line| line == "Approve the plan."),
                "{rendered}"
            );
        }
    }

    #[test]
    fn math_delimiter_normalization_preserves_code_escapes_and_unmatched_input() {
        let input = "before \\(x_2\\) and \\[\\alpha + 1\\]\n\n`\\(code\\)`\n\n```txt\n\\[fenced\\]\n```\n\n    \\(indented\\)\n\nescaped \\\\(literal\\) unmatched \\(tail";
        let normalized = normalize_math_delimiters(input);
        assert!(normalized.contains("before $x_2$ and $$\\alpha + 1$$"));
        assert!(normalized.contains("`\\(code\\)`"));
        assert!(normalized.contains("\\[fenced\\]"));
        assert!(normalized.contains("    \\(indented\\)"));
        assert!(
            normalized.contains("escaped \\\\(literal\\\\) unmatched \\\\(tail"),
            "normalized input: {normalized:?}"
        );
    }

    #[test]
    fn math_converter_handles_supported_subset_and_preserves_unknown_tex() {
        assert_eq!(
            convert_math(r"\alpha + \Gamma \leq \infty \rightarrow x_2^n"),
            "α + Γ ≤ ∞ → x₂ⁿ"
        );
        assert_eq!(
            convert_math(r"\sqrt{\beta} + \frac{a}{\pi} + \left(x\right)"),
            "√β + a⁄π + (x)"
        );
        assert_eq!(
            convert_math(r"\sqrt{x+1} \frac{ab}{c} \unknown{x} \left\langle"),
            r"\sqrt{x+1} \frac{ab}{c} \unknown{x} \left\langle"
        );
    }

    #[test]
    fn inline_and_display_math_render_without_mutating_currency_or_code() {
        let rendered = rendered_text(
            "Price $5$ or $5.00$, math $x^2 + \\alpha$, and `\\(x_2\\)`.\n\n\\[\\sum_{i=1}^n i\\]",
        );
        assert!(rendered.contains("Price $5$ or $5.00$, math x² + α, and \\(x_2\\)."));
        assert!(rendered.contains("  ∑ᵢ₌₁ⁿ i"));
    }

    #[test]
    fn currency_escapes_and_urls_remain_literal() {
        let rendered = rendered_text(
            r"Pay $5 and $6, keep $5$ and $5.00$, escape \$9, and visit https://example.test/$5/path.",
        );
        assert!(rendered.contains(
            "Pay $5 and $6, keep $5$ and $5.00$, escape $9, and visit https://example.test/$5/path."
        ));
    }

    #[test]
    fn math_renders_in_lists_and_tables_and_leaves_malformed_input_visible() {
        let rendered = rendered_text(
            "- value \\(x_1\\)\n\n| Name | Value |\n| --- | --- |\n| alpha | $\\alpha$ |\n\nMalformed $x and \\(tail",
        );
        assert!(rendered.contains("• value x₁"));
        assert!(rendered.contains("alpha"));
        assert!(rendered.contains('α'));
        assert!(
            rendered.contains("Malformed $x and \\(tail"),
            "rendered output: {rendered:?}"
        );
    }

    #[test]
    fn ordered_lists_keep_their_start_and_sequence() {
        let rendered = rendered_text("3. inspect\n4. implement");
        assert!(rendered.contains("3. inspect"), "{rendered:?}");
        assert!(rendered.contains("4. implement"), "{rendered:?}");
    }

    #[test]
    fn tables_render_as_bounded_grids() {
        let rendered = render_markdown(
            "| Path | Meaning |\n| --- | --- |\n| /very/long/path/to/file | useful output |",
            30,
            false,
        );
        let text = rendered
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains('┌'));
        assert!(text.contains('┼'));
        assert!(text.contains('┘'));
        assert!(rendered.iter().all(|line| line.width() <= 30));
    }

    #[test]
    fn links_use_the_label_as_exact_layout_text() {
        let rendered = rendered_text(
            "Read [Unicode docs 界](https://example.test/a/very/long/destination) now.",
        );

        assert_eq!(rendered.trim_end(), "Read Unicode docs 界 now.");
    }

    #[test]
    fn link_destinations_are_preserved_out_of_band_without_changing_width() {
        let document = render_markdown_document_with_palette(
            "Read [Unicode docs 界](https://example.test/path) now.",
            80,
            false,
            MarkdownPalette::default(),
        );
        assert_eq!(
            document.links,
            vec![MarkdownLink {
                label: "Unicode docs 界".into(),
                destination: "https://example.test/path".into(),
            }]
        );
        assert_eq!(
            document.lines.iter().map(Line::width).max(),
            Some(25),
            "destination must not enter layout width"
        );
    }

    #[test]
    fn mermaid_fences_use_cached_terminal_render_and_invalid_input_keeps_code() {
        let source = "flowchart LR\nA[Start] --> B[Done]";
        let fenced_source = format!("{source}\n");
        assert!(matches!(
            crate::mermaid::wait_for_render(&fenced_source, 96),
            crate::mermaid::MermaidRender::Ready(_)
        ));
        let rendered = rendered_text(&format!("```mermaid\n{source}\n```"));
        assert!(rendered.contains("Mermaid"));
        assert!(rendered.contains("Start"));

        let fallback = rendered_text("```mermaid\nflowchart LR\nA -->\n```");
        assert!(fallback.contains("flowchart LR"));
    }

    #[test]
    fn link_destinations_never_enter_terminal_layout_text() {
        let rendered = rendered_text(
            "[unsafe](javascript:alert) and [encoded](https://example.test/%1B%5D8%3B%3Bbad)",
        );

        assert_eq!(rendered.trim_end(), "unsafe and encoded");
        assert!(!rendered.contains("javascript:"));
        assert!(!rendered.contains("%1B"));
        assert!(!rendered.contains('\u{1b}'));
    }

    #[test]
    fn unicode_link_width_excludes_the_destination() {
        let rendered = render_markdown(
            "[界e\u{301}](https://example.test/a/destination/that/must/not/affect/wrapping)",
            4,
            false,
        );
        let visible = rendered
            .iter()
            .find(|line| !line.spans.is_empty())
            .expect("link line");

        assert_eq!(visible.width(), UnicodeWidthStr::width("界e\u{301}"));
    }

    #[test]
    fn malformed_link_syntax_keeps_the_plain_text_fallback() {
        let rendered = rendered_text("Keep [this](https://example.test visible");

        assert_eq!(
            rendered.trim_end(),
            "Keep [this](https://example.test visible"
        );
    }

    #[test]
    fn tables_wrap_without_losing_cell_content_at_supported_widths() {
        let key = "αβγδεζηθικλμνξοπρστυφχψω";
        let value = "/docs/reference/important-output/file.rs";
        let markdown = format!("| Key | Value |\n| --- | --- |\n| {key} | {value} |");

        for width in [100, 60, 40] {
            let rendered = render_markdown(&markdown, width, false);
            assert!(
                rendered.iter().all(|line| line.width() <= width as usize),
                "table exceeded width {width}: {rendered:?}"
            );

            let mut payloads = vec![String::new(), String::new()];
            let mut in_data_row = false;
            for line in &rendered {
                let text = line
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>();
                if text.contains('├') {
                    in_data_row = true;
                    continue;
                }
                if text.contains('└') {
                    break;
                }
                if !in_data_row {
                    continue;
                }
                for (index, fragment) in text.split('│').skip(1).take(2).enumerate() {
                    payloads[index].push_str(fragment.trim());
                }
            }
            assert_eq!(payloads, [key, value], "content changed at width {width}");
        }
    }

    #[test]
    fn tables_preserve_columns_beyond_eight_at_supported_widths() {
        let headers = (1..=9)
            .map(|index| format!("h{index:02}"))
            .collect::<Vec<_>>();
        let values = (1..=9)
            .map(|index| format!("v{index:02}"))
            .collect::<Vec<_>>();
        let markdown = format!(
            "| {} |\n| {} |\n| {} |",
            headers.join(" | "),
            ["---"; 9].join(" | "),
            values.join(" | ")
        );

        for width in [100, 60, 40] {
            let rendered = render_markdown(&markdown, width, false);
            assert!(
                rendered.iter().all(|line| line.width() <= width as usize),
                "table exceeded width {width}: {rendered:?}"
            );
            let visible = rendered
                .iter()
                .flat_map(|line| line.spans.iter())
                .map(|span| span.content.as_ref())
                .collect::<String>();
            for token in headers.iter().chain(&values) {
                assert_eq!(
                    visible.matches(token).count(),
                    1,
                    "{token:?} was lost or duplicated at width {width}: {visible:?}"
                );
            }
        }
    }

    #[test]
    fn wide_graphemes_force_a_bounded_stacked_table() {
        let headers = (1..=8)
            .map(|index| format!("界{index}"))
            .collect::<Vec<_>>();
        let values = (1..=8)
            .map(|index| format!("🙂{index}"))
            .collect::<Vec<_>>();
        let markdown = format!(
            "| {} |\n| {} |\n| {} |",
            headers.join(" | "),
            ["---"; 8].join(" | "),
            values.join(" | ")
        );
        let rendered = render_markdown(&markdown, 40, false);
        assert!(rendered.iter().all(|line| line.width() <= 40));
        let visible = rendered
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();
        for token in headers.iter().chain(&values) {
            assert!(visible.contains(token), "missing {token:?}: {visible:?}");
        }
    }

    #[test]
    fn inline_code_inside_a_link_keeps_link_indication() {
        let rendered = render_markdown("[read `code`](https://example.test)", 80, false);
        let visible = rendered
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(visible.trim_end(), "read code");
        for span in rendered
            .iter()
            .flat_map(|line| line.spans.iter())
            .filter(|span| !span.content.is_empty())
        {
            assert_eq!(span.style.fg, Some(Color::Blue));
            assert!(span.style.add_modifier.contains(Modifier::UNDERLINED));
        }
        let code = rendered
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content == "code")
            .unwrap();
        assert_eq!(code.style.bg, Some(Color::DarkGray));
    }

    #[test]
    fn custom_palette_styles_headings_strong_text_and_inline_code() {
        let palette = MarkdownPalette {
            text: Color::White,
            heading: Color::Blue,
            heading_text: Color::Blue,
            accent: Color::Red,
            link: Color::Cyan,
            strong: Color::Yellow,
            muted: Color::Gray,
            border: Color::DarkGray,
            code_background: Color::Black,
            md_h1: Color::Blue,
            md_h2: Color::Blue,
            md_h3: Color::Blue,
            md_h4: Color::Blue,
            md_h5: Color::Gray,
            md_h6: Color::Gray,
        };
        let rendered = render_markdown_with_palette(
            "# Heading\n\n**important**, [docs](https://example.test), and `path`",
            80,
            false,
            palette,
        );
        let spans = rendered
            .iter()
            .flat_map(|line| line.spans.iter())
            .collect::<Vec<_>>();
        assert!(spans
            .iter()
            .any(|span| span.content == "Heading" && span.style.fg == Some(Color::Blue)));
        assert!(spans
            .iter()
            .any(|span| span.content == "important" && span.style.fg == Some(Color::Yellow)));
        assert!(spans.iter().any(|span| {
            span.content == "docs"
                && span.style.fg == Some(Color::Cyan)
                && span.style.add_modifier.contains(Modifier::UNDERLINED)
        }));
        assert!(spans.iter().any(|span| {
            span.content == "path"
                && span.style.fg == Some(Color::Red)
                && span.style.bg == Some(Color::Black)
        }));
    }

    #[test]
    fn default_palette_keeps_legacy_inline_colors() {
        let rendered = render_markdown(
            "# Heading\n\n**important** [site](https://example.test) `path`",
            80,
            false,
        );
        let spans = rendered
            .iter()
            .flat_map(|line| line.spans.iter())
            .collect::<Vec<_>>();
        assert!(spans
            .iter()
            .any(|span| span.content == "Heading" && span.style.fg == Some(Color::White)));
        assert!(spans
            .iter()
            .any(|span| span.content == "important" && span.style.fg == Some(Color::White)));
        assert!(spans.iter().any(|span| {
            span.content == "site"
                && span.style.fg == Some(Color::Blue)
                && span.style.add_modifier.contains(Modifier::UNDERLINED)
        }));
        assert!(spans.iter().any(|span| {
            span.content == "path"
                && span.style.fg == Some(Color::Yellow)
                && span.style.bg == Some(Color::DarkGray)
        }));
    }
}
