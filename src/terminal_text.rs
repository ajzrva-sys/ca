#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EscapeState {
    Ground,
    Escape,
    Csi,
    String,
    StringEscape,
}

pub fn sanitize_terminal_text(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    output.extend(sanitized_char_indices(input).map(|(_, character)| character));
    output
}

/// Retain source offsets so selectable views can address the original bytes.
pub(crate) fn sanitized_char_indices(input: &str) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut state = EscapeState::Ground;
    input.char_indices().filter_map(move |(offset, character)| {
        let mut visible = false;
        state = match state {
            EscapeState::Ground => match character {
                '\u{1b}' => EscapeState::Escape,
                '\n' | '\t' => {
                    visible = true;
                    EscapeState::Ground
                }
                value if value.is_control() => EscapeState::Ground,
                _ => {
                    visible = true;
                    EscapeState::Ground
                }
            },
            EscapeState::Escape => match character {
                '[' => EscapeState::Csi,
                ']' | 'P' | '_' | '^' => EscapeState::String,
                '\u{1b}' => EscapeState::Escape,
                value => {
                    visible = !value.is_control() || value == '\n' || value == '\t';
                    EscapeState::Ground
                }
            },
            EscapeState::Csi => {
                if ('@'..='~').contains(&character) {
                    EscapeState::Ground
                } else if character == '\u{1b}' {
                    EscapeState::Escape
                } else if character == '\n' {
                    visible = true;
                    EscapeState::Ground
                } else {
                    EscapeState::Csi
                }
            }
            EscapeState::String => match character {
                '\u{7}' => EscapeState::Ground,
                '\u{1b}' => EscapeState::StringEscape,
                _ => EscapeState::String,
            },
            EscapeState::StringEscape => {
                if character == '\\' {
                    EscapeState::Ground
                } else if character == '\u{1b}' {
                    EscapeState::StringEscape
                } else {
                    EscapeState::String
                }
            }
        };
        visible.then_some((offset, character))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_csi_osc_dcs_apc_bel_c0_and_malformed_sequences() {
        let input = concat!(
            "safe\u{1b}[31mred\u{1b}[0m",
            "\u{1b}]0;secret\u{7}after",
            "\u{1b}Ppayload\u{1b}\\dcs-after",
            "\u{1b}_payload\u{1b}\\apc-after",
            "\u{0}\u{7}\u{8}\tkept\n",
            "\u{1b}Xvisible",
            "\u{1b}[123;"
        );
        assert_eq!(
            sanitize_terminal_text(input),
            "saferedafterdcs-afterapc-after\tkept\nXvisible"
        );
    }
}
