//! Visible approval-card text. The command stored on the approval is unchanged.
//! Invisible, format, and bidi code points render as escapes, and long space
//! runs collapse so the tail of the command stays on the card.

const BLANK_GLYPHS: &[char] = &[
    '\u{034f}', '\u{115f}', '\u{1160}', '\u{17b4}', '\u{17b5}', '\u{180b}', '\u{180c}', '\u{180d}',
    '\u{180f}', '\u{2800}', '\u{3164}', '\u{ffa0}',
];

pub fn command_preview(text: &str) -> String {
    let escaped = escape_hidden(text);
    let collapsed = collapse_blank_lines(&escaped);
    collapse_padding(&collapsed)
}

fn escape_hidden(text: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if !hidden(ch) {
            out.push(ch);
            index += 1;
            continue;
        }
        let mut count = 1;
        while index + count < chars.len() && chars[index + count] == ch {
            count += 1;
        }
        let escape = format!("\\u{{{:x}}}", u32::from(ch));
        if count >= 4 {
            out.push('⟨');
            out.push_str(&escape);
            out.push_str(&format!(" ×{count}⟩"));
        } else {
            for _ in 0..count {
                out.push_str(&escape);
            }
        }
        index += count;
    }
    out
}

fn hidden(ch: char) -> bool {
    if ch == '\t' || ch == '\n' {
        return false;
    }
    if ch.is_control() || BLANK_GLYPHS.contains(&ch) {
        return true;
    }
    matches!(
        ch,
        '\u{00ad}'
            | '\u{061c}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
    )
}

fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::new();
    let mut blanks = 0_usize;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches('\n');
        let newline = line.ends_with('\n');
        if body.trim_matches([' ', '\t']).is_empty() && newline {
            blanks += 1;
            continue;
        }
        if blanks >= 3 {
            out.push('\n');
            out.push_str(&format!("⟨+{blanks} blank lines⟩"));
            out.push('\n');
        } else {
            for _ in 0..blanks {
                out.push('\n');
            }
        }
        blanks = 0;
        out.push_str(body);
        if newline {
            out.push('\n');
        }
    }
    if blanks >= 3 {
        out.push('\n');
        out.push_str(&format!("⟨+{blanks} blank lines⟩"));
        out.push('\n');
    } else {
        for _ in 0..blanks {
            out.push('\n');
        }
    }
    out
}

fn collapse_padding(text: &str) -> String {
    let mut out = String::new();
    for (index, line) in text.split_inclusive('\n').enumerate() {
        let newline = line.ends_with('\n');
        let body = line.trim_end_matches('\n');
        let leading = body
            .chars()
            .take_while(|ch| *ch == ' ' || *ch == '\t')
            .count();
        if leading >= 65 {
            out.push_str(&format!(" ⟨+{leading} whitespace chars⟩ "));
            out.push_str(&body[leading..]);
        } else {
            out.push_str(&collapse_midline(body, index == 0));
        }
        if newline {
            out.push('\n');
        }
    }
    out
}

fn collapse_midline(body: &str, _first: bool) -> String {
    let mut out = String::new();
    let chars: Vec<char> = body.chars().collect();
    let mut index = 0;
    let mut saw_text = false;
    while index < chars.len() {
        let ch = chars[index];
        if saw_text && (ch == ' ' || ch == '\t') {
            let mut count = 0;
            while index + count < chars.len()
                && (chars[index + count] == ' ' || chars[index + count] == '\t')
            {
                count += 1;
            }
            if count >= 21 {
                out.push_str(&format!(" ⟨+{count} whitespace chars⟩ "));
            } else {
                for _ in 0..count {
                    out.push(chars[index]);
                }
            }
            index += count;
            continue;
        }
        if ch != ' ' && ch != '\t' {
            saw_text = true;
        }
        out.push(ch);
        index += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_card_shows_hidden_characters() {
        let command = "echo hello\u{200b}world\u{202e}!";
        let preview = command_preview(command);
        assert!(preview.contains("echo hello"), "{preview}");
        assert!(preview.contains("\\u{200b}"), "{preview}");
        assert!(preview.contains("\\u{202e}"), "{preview}");
        assert!(preview.contains('!'), "{preview}");
        assert!(!preview.contains('\u{200b}'));
        assert!(!preview.contains('\u{202e}'));
        let ordinary = command_preview("café — status");
        assert_eq!(ordinary, "café — status");
        let padded = format!("echo{}hidden", " ".repeat(21));
        let collapsed = command_preview(&padded);
        assert!(collapsed.contains("⟨+21 whitespace chars⟩"), "{collapsed}");
        assert!(collapsed.contains("hidden"), "{collapsed}");
        let blanks = command_preview("top\n\n\n\n\nbottom");
        assert!(blanks.contains("blank lines"), "{blanks}");
        assert!(blanks.contains("bottom"), "{blanks}");
    }
}
