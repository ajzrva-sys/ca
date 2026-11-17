//! Model-facing grep card: a counted header, capped lines, and a cut-short footer.
//! The bytes streamed so far are always a prefix of that same card.

/// Characters kept from each hit, including a trailing ellipsis when cut.
pub const LINE_CHAR_CAP: usize = 160;

/// Cumulative card size, header and footer included.
pub const CARD_BYTE_CAP: usize = 8 * 1024;

/// Footer used only when the byte cap drops part of the search.
pub const CUT_SHORT_FOOTER: &str = "[search cut short]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrepHit<'a> {
    pub path: &'a str,
    pub line: u32,
    pub text: &'a str,
}

/// Render `hits` into one card. `line_char_cap` bounds each hit; `byte_cap` bounds the card.
pub fn card_for_query_lines(output: &str) -> String {
    let owned = output
        .lines()
        .filter_map(|line| {
            let (path, rest) = line.split_once(':')?;
            let (line_no, text) = rest.split_once(':')?;
            let line_no = line_no.trim().parse().ok()?;
            Some((path.to_string(), line_no, text.trim().to_string()))
        })
        .collect::<Vec<_>>();
    if owned.is_empty() {
        return output.to_string();
    }
    let hits = owned
        .iter()
        .map(|(path, line, text)| GrepHit {
            path,
            line: *line,
            text,
        })
        .collect::<Vec<_>>();
    let card = render_search_card(&hits, LINE_CHAR_CAP, CARD_BYTE_CAP);
    stream_prefix(&card, card.len()).to_string()
}

pub fn render_search_card(hits: &[GrepHit<'_>], line_char_cap: usize, byte_cap: usize) -> String {
    let header = format!("Found {}\n", hits.len());
    let lines: Vec<String> = hits
        .iter()
        .map(|hit| {
            let text = cap_chars(hit.text, line_char_cap);
            format!("{}:{}:{text}\n", hit.path, hit.line)
        })
        .collect();

    let mut body = String::new();
    let mut fit = 0usize;
    for line in &lines {
        if header.len() + body.len() + line.len() > byte_cap {
            break;
        }
        body.push_str(line);
        fit += 1;
    }
    if fit == lines.len() {
        let card = format!("{header}{body}");
        if card.len() <= byte_cap {
            return card;
        }
    }

    let footer = format!("{CUT_SHORT_FOOTER}\n");
    body.clear();
    for line in &lines {
        if header.len() + body.len() + line.len() + footer.len() > byte_cap {
            break;
        }
        body.push_str(line);
    }
    let mut card = format!("{header}{body}{footer}");
    if card.len() > byte_cap {
        card = prefix_string(&format!("{header}{footer}"), byte_cap);
    }
    if byte_cap > 0 && !card.contains(CUT_SHORT_FOOTER) {
        card = prefix_string(CUT_SHORT_FOOTER, byte_cap);
    }
    if card.len() > byte_cap {
        card = prefix_string(&card, byte_cap);
    }
    card
}

/// Bytes already streamed. Always `card[..prefix.len()]` on a char boundary.
pub fn stream_prefix(card: &str, sent_bytes: usize) -> &str {
    byte_prefix(card, sent_bytes)
}

fn cap_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    let mut out: String = text.chars().take(max_chars - 1).collect();
    out.push('…');
    out
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

fn prefix_string(text: &str, max_bytes: usize) -> String {
    byte_prefix(text, max_bytes).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_hits_render_found_three() {
        let hits = [
            GrepHit {
                path: "a.rs",
                line: 1,
                text: "alpha",
            },
            GrepHit {
                path: "b.rs",
                line: 2,
                text: "beta",
            },
            GrepHit {
                path: "c.rs",
                line: 3,
                text: "gamma",
            },
        ];
        let card = render_search_card(&hits, LINE_CHAR_CAP, CARD_BYTE_CAP);
        assert_eq!(card.lines().next(), Some("Found 3"));
        assert!(card.contains("alpha"));
        assert!(card.contains("beta"));
        assert!(card.contains("gamma"));
        assert!(!card.contains(CUT_SHORT_FOOTER));
    }

    #[test]
    fn a_long_line_is_capped() {
        let long = "m".repeat(LINE_CHAR_CAP + 40);
        let hits = [GrepHit {
            path: "p.rs",
            line: 9,
            text: &long,
        }];
        let card = render_search_card(&hits, LINE_CHAR_CAP, CARD_BYTE_CAP);
        assert!(!card.contains(&long));
        let rendered = card.lines().nth(1).expect("hit line");
        let text = rendered.rsplit_once(':').expect("path and text").1;
        assert!(text.chars().count() <= LINE_CHAR_CAP);
        assert!(text.chars().count() < long.chars().count());
    }

    #[test]
    fn byte_cap_sets_the_footer_and_the_prefix_matches_the_card() {
        let hits: Vec<GrepHit<'_>> = (0..20)
            .map(|index| GrepHit {
                path: "src/lib.rs",
                line: index + 1,
                text: "example hit that will not all fit",
            })
            .collect();
        let byte_cap = 80;
        let card = render_search_card(&hits, LINE_CHAR_CAP, byte_cap);
        assert!(card.contains(CUT_SHORT_FOOTER), "{card}");
        assert!(card.len() <= byte_cap, "{}", card.len());
        let prefix = stream_prefix(&card, 17);
        assert_eq!(prefix, &card[..prefix.len()]);
        assert!(card.starts_with(prefix));
        assert_eq!(stream_prefix(&card, card.len() + 8), card.as_str());
    }
}
