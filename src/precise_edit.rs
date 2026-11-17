//! Precise replacements for an exact-string edit.
//!
//! Order: exact, line-trimmed, whitespace, indentation, escape, trimmed
//! boundary, Unicode. A strategy that matches more than once fails unless
//! replace-all was requested. Similarity matching is intentionally absent.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedEdit {
    pub content: String,
    pub strategy: &'static str,
    pub already_applied: bool,
}

pub fn apply(
    content: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<AppliedEdit, String> {
    if old_string.is_empty() {
        return Err("old_string is empty".into());
    }
    if old_string == new_string {
        return Err("No edit was applied because old_string and new_string are identical.".into());
    }
    if already_applied(content, old_string, new_string) {
        return Ok(AppliedEdit {
            content: content.to_string(),
            strategy: "already_applied",
            already_applied: true,
        });
    }
    type Strategy = (&'static str, fn(&str, &str) -> Vec<(usize, usize)>);
    let strategies: &[Strategy] = &[
        ("exact", exact_spans),
        ("line_trimmed", line_trimmed_spans),
        ("whitespace", whitespace_spans),
        ("indentation", indentation_spans),
        ("escape", escape_spans),
        ("trimmed_boundary", trimmed_boundary_spans),
        ("unicode", unicode_spans),
    ];
    for (name, strategy) in strategies {
        let spans = strategy(content, old_string);
        if spans.is_empty() {
            continue;
        }
        if spans.len() > 1 && !replace_all {
            return Err(format!(
                "search string found {} times (strategy: {name})",
                spans.len()
            ));
        }
        return Ok(AppliedEdit {
            content: replace_spans(content, &spans, new_string),
            strategy: name,
            already_applied: false,
        });
    }
    Err(format!("'{old_string}' not found"))
}

fn already_applied(content: &str, old_string: &str, new_string: &str) -> bool {
    let trimmed = new_string.trim();
    trimmed.chars().count() >= 8 && content.contains(new_string) && !content.contains(old_string)
}

fn replace_spans(content: &str, spans: &[(usize, usize)], new_string: &str) -> String {
    let mut ordered = spans.to_vec();
    ordered.sort_by_key(|span| span.0);
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0;
    for (start, end) in ordered {
        if start < cursor || end < start || end > content.len() {
            continue;
        }
        out.push_str(&content[cursor..start]);
        out.push_str(new_string);
        cursor = end;
    }
    out.push_str(&content[cursor..]);
    out
}

fn exact_spans(content: &str, pattern: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut from = 0;
    while let Some(offset) = content[from..].find(pattern) {
        let start = from + offset;
        let end = start + pattern.len();
        spans.push((start, end));
        from = end;
    }
    spans
}

fn line_windows(
    content: &str,
    pattern: &str,
    transform: fn(&str) -> String,
) -> Vec<(usize, usize)> {
    let content_lines: Vec<&str> = split_lines(content);
    let pattern_lines: Vec<String> = split_lines(pattern).into_iter().map(transform).collect();
    let count = pattern_lines.len();
    if count == 0 || content_lines.len() < count {
        return Vec::new();
    }
    let mut spans = Vec::new();
    for start_line in 0..=content_lines.len() - count {
        let matched = content_lines[start_line..start_line + count]
            .iter()
            .map(|line| transform(line))
            .eq(pattern_lines.iter().cloned());
        if matched {
            spans.push(line_span(
                content,
                &content_lines,
                start_line,
                start_line + count,
            ));
        }
    }
    spans
}

fn split_lines(text: &str) -> Vec<&str> {
    text.split('\n').collect()
}

fn line_span(content: &str, lines: &[&str], start_line: usize, end_line: usize) -> (usize, usize) {
    let start = lines[..start_line]
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>();
    let end = lines[..end_line]
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>()
        .saturating_sub(1);
    (start, end.min(content.len()))
}

fn line_trimmed_spans(content: &str, pattern: &str) -> Vec<(usize, usize)> {
    line_windows(content, pattern, |line| line.trim().to_string())
}

fn indentation_spans(content: &str, pattern: &str) -> Vec<(usize, usize)> {
    line_windows(content, pattern, |line| line.trim_start().to_string())
}

fn trimmed_boundary_spans(content: &str, pattern: &str) -> Vec<(usize, usize)> {
    boundary_windows(content, pattern)
}

fn boundary_windows(content: &str, pattern: &str) -> Vec<(usize, usize)> {
    let content_lines = split_lines(content);
    let pattern_lines = split_lines(pattern);
    let count = pattern_lines.len();
    if count == 0 || content_lines.len() < count {
        return Vec::new();
    }
    let expected = strip_boundary(&pattern_lines);
    let mut spans = Vec::new();
    for start_line in 0..=content_lines.len() - count {
        let window: Vec<&str> = content_lines[start_line..start_line + count].to_vec();
        if strip_boundary(&window) == expected {
            spans.push(line_span(
                content,
                &content_lines,
                start_line,
                start_line + count,
            ));
        }
    }
    spans
}

fn strip_boundary(lines: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = lines.iter().map(|line| (*line).to_string()).collect();
    if let Some(first) = out.first_mut() {
        *first = first.trim().to_string();
    }
    if out.len() > 1 {
        if let Some(last) = out.last_mut() {
            *last = last.trim().to_string();
        }
    }
    out
}

fn whitespace_spans(content: &str, pattern: &str) -> Vec<(usize, usize)> {
    let (normalized_content, map) = collapse_whitespace(content);
    let (normalized_pattern, _) = collapse_whitespace(pattern);
    if normalized_pattern.is_empty() {
        return Vec::new();
    }
    exact_spans(&normalized_content, &normalized_pattern)
        .into_iter()
        .filter_map(|(start, end)| {
            map_collapsed_span(content, &normalized_content, &map, start, end)
        })
        .collect()
}

fn collapse_whitespace(text: &str) -> (String, Vec<usize>) {
    let mut out = String::new();
    let mut map = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if ch == ' ' || ch == '\t' {
            map.push(index);
            out.push(' ');
            while let Some((_, next)) = chars.peek().copied() {
                if next == ' ' || next == '\t' {
                    chars.next();
                } else {
                    break;
                }
            }
        } else {
            map.push(index);
            out.push(ch);
        }
    }
    (out, map)
}

fn map_collapsed_span(
    original: &str,
    collapsed: &str,
    map: &[usize],
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    if start >= map.len() || end == 0 || end > map.len() {
        return None;
    }
    let orig_start = map[start];
    let last = map[end - 1];
    let last_char = original[last..].chars().next()?;
    let mut orig_end = last + last_char.len_utf8();
    if collapsed.as_bytes().get(end - 1) == Some(&b' ') {
        while orig_end < original.len() {
            let next = original[orig_end..].chars().next()?;
            if next == ' ' || next == '\t' {
                orig_end += next.len_utf8();
            } else {
                break;
            }
        }
    }
    Some((orig_start, orig_end.min(original.len())))
}

fn escape_spans(content: &str, pattern: &str) -> Vec<(usize, usize)> {
    let unescaped = unescape_controls(pattern);
    if unescaped == pattern {
        return Vec::new();
    }
    exact_spans(content, &unescaped)
}

fn unescape_controls(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.peek().copied() {
                Some('n') => {
                    chars.next();
                    out.push('\n');
                }
                Some('t') => {
                    chars.next();
                    out.push('\t');
                }
                Some('r') => {
                    chars.next();
                    out.push('\r');
                }
                _ => out.push(ch),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn unicode_spans(content: &str, pattern: &str) -> Vec<(usize, usize)> {
    let folded_content = fold_unicode(content);
    let folded_pattern = fold_unicode(pattern);
    if folded_content == content && folded_pattern == pattern {
        return Vec::new();
    }
    let folded_spans = if !exact_spans(&folded_content, &folded_pattern).is_empty() {
        exact_spans(&folded_content, &folded_pattern)
    } else {
        line_trimmed_spans(&folded_content, &folded_pattern)
    };
    if folded_spans.is_empty() {
        return Vec::new();
    }
    map_folded_spans(content, &folded_spans)
}

fn fold_unicode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\u{201c}' | '\u{201d}' => out.push('"'),
            '\u{2018}' | '\u{2019}' => out.push('\''),
            '\u{2014}' => out.push_str("--"),
            '\u{2013}' | '\u{2212}' => out.push('-'),
            '\u{2026}' => out.push_str("..."),
            '\u{00a0}' | '\u{2000}' | '\u{2001}' | '\u{2002}' | '\u{2003}' | '\u{2004}'
            | '\u{2005}' | '\u{2006}' | '\u{2007}' | '\u{2008}' | '\u{2009}' | '\u{200a}'
            | '\u{202f}' | '\u{205f}' | '\u{3000}' => out.push(' '),
            other => out.push(other),
        }
    }
    out
}

fn map_folded_spans(original: &str, spans: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut origins = Vec::new();
    let mut folded_pos = 0;
    for (index, ch) in original.char_indices() {
        origins.push((folded_pos, index));
        folded_pos += fold_unicode(&ch.to_string()).len();
    }
    origins.push((folded_pos, original.len()));
    spans
        .iter()
        .filter_map(|(start, end)| {
            let orig_start = origins.iter().find(|(folded, _)| folded == start)?.1;
            let orig_end = origins
                .iter()
                .find(|(folded, _)| *folded >= *end)
                .map(|(_, index)| *index)
                .unwrap_or(original.len());
            Some((orig_start, orig_end))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_and_unicode_match_and_name_the_strategy() {
        let spaced = apply(
            "let  value = 1;\n",
            "let value = 1;",
            "let value = 2;",
            false,
        )
        .unwrap();
        assert_eq!(spaced.strategy, "whitespace");
        assert_eq!(spaced.content, "let value = 2;\n");

        let quoted = apply(
            "let name = \u{201c}ca\u{201d};\n",
            "let name = \"ca\";",
            "let name = \"agent\";",
            false,
        )
        .unwrap();
        assert_eq!(quoted.strategy, "unicode");
        assert_eq!(quoted.content, "let name = \"agent\";\n");
    }

    #[test]
    fn multiple_matches_fail_unless_replace_all_was_requested() {
        let source = "alpha\nalpha\n";
        let error = apply(source, "alpha", "beta", false).unwrap_err();
        assert!(error.contains("found 2 times"), "{error}");
        assert!(error.contains("exact"), "{error}");
        let replaced = apply(source, "alpha", "beta", true).unwrap();
        assert_eq!(replaced.strategy, "exact");
        assert_eq!(replaced.content, "beta\nbeta\n");
    }

    #[test]
    fn an_already_applied_edit_is_a_no_op() {
        let source = "fn ready() {}\n";
        let edit = apply(source, "fn missing() {}", "fn ready() {}", false).unwrap();
        assert!(edit.already_applied);
        assert_eq!(edit.strategy, "already_applied");
        assert_eq!(edit.content, source);
        assert!(apply(source, "nope", "short", false).is_err());
    }
}
