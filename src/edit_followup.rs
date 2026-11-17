//! Bounded edit diagnostics, plus a hint when a miss looks like a confusable character.
//! The hint names the line and the ASCII lookalike. It does not rewrite the file.

pub const MAX_DIAGNOSTIC_ITEMS: usize = 8;
pub const MAX_DIAGNOSTIC_BYTES: usize = 2_048;

const SAMPLE_CHARS: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditDiagnostic<'a> {
    pub path: &'a str,
    pub line: u32,
    pub message: &'a str,
}

/// Summary of at most `max_items` notes, within `max_bytes`. Empty notes yield an empty string.
pub fn format_edit_diagnostics(
    notes: &[EditDiagnostic<'_>],
    max_items: usize,
    max_bytes: usize,
) -> String {
    if notes.is_empty() || max_items == 0 || max_bytes == 0 {
        return String::new();
    }
    let mut out = String::new();
    for (shown, note) in notes.iter().enumerate() {
        if shown >= max_items {
            break;
        }
        let line = format!("{}:{}: {}\n", note.path, note.line, note.message);
        if out.len() + line.len() > max_bytes {
            if out.is_empty() {
                out.push_str(byte_prefix(&line, max_bytes));
            }
            break;
        }
        out.push_str(&line);
    }
    out
}

/// When `missing_old` missed and `file_text` contains a smart quote, em dash, ellipsis,
/// or non-breaking space, name that line and the ASCII lookalike. Does not rewrite the file.
pub fn confusable_edit_hint(file_text: &str, missing_old: &str) -> Option<String> {
    for (index, line) in file_text.lines().enumerate() {
        let Some(confusable) = line.chars().find_map(lookalike) else {
            continue;
        };
        let line_no = index + 1;
        let sample = quote_miss(missing_old);
        return Some(format!(
            "line {line_no}: {sample:?} missed; ASCII lookalike is {confusable}"
        ));
    }
    None
}

fn lookalike(ch: char) -> Option<&'static str> {
    match ch {
        '\u{2018}' | '\u{2019}' => Some("straight quote '"),
        '\u{201C}' | '\u{201D}' => Some("straight quote \""),
        '\u{2014}' => Some("hyphen -"),
        '\u{2026}' => Some("ellipsis ..."),
        '\u{00A0}' => Some("space"),
        _ => None,
    }
}

fn quote_miss(missing_old: &str) -> String {
    let mut sample: String = missing_old.chars().take(SAMPLE_CHARS).collect();
    if missing_old.chars().count() > SAMPLE_CHARS {
        sample.push('…');
    }
    sample
}

fn byte_prefix(text: &str, max_bytes: usize) -> &str {
    if max_bytes >= text.len() {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curly_quote_on_line_three_names_the_straight_quote() {
        let file = "fn ready() {}\nlet n = 1;\nlet s = \u{201C}hi\u{201D};\n";
        let original = file.to_string();
        let hint = confusable_edit_hint(file, "let s = \"hi\";").expect("hint");
        assert!(hint.contains("line 3"), "{hint}");
        assert!(hint.contains("straight quote"), "{hint}");
        assert!(hint.contains('"'), "{hint}");
        assert_eq!(file, original);
    }

    #[test]
    fn a_plain_miss_returns_none() {
        let file = "fn ready() {}\nlet n = 1;\nlet s = \"hi\";\n";
        assert!(confusable_edit_hint(file, "let s = 'hi';").is_none());
    }

    #[test]
    fn diagnostics_respect_the_item_cap() {
        let notes = [
            EditDiagnostic {
                path: "a.rs",
                line: 1,
                message: "first",
            },
            EditDiagnostic {
                path: "b.rs",
                line: 2,
                message: "second",
            },
            EditDiagnostic {
                path: "c.rs",
                line: 4,
                message: "third",
            },
        ];
        let text = format_edit_diagnostics(&notes, 2, MAX_DIAGNOSTIC_BYTES);
        assert!(text.contains("a.rs"));
        assert!(text.contains("first"));
        assert!(text.contains("b.rs"));
        assert!(text.contains("second"));
        assert!(!text.contains("c.rs"));
        assert!(!text.contains("third"));
        assert!(text.len() <= MAX_DIAGNOSTIC_BYTES);
        let empty = format_edit_diagnostics(&[], MAX_DIAGNOSTIC_ITEMS, MAX_DIAGNOSTIC_BYTES);
        assert_eq!(empty, "");
    }
}
