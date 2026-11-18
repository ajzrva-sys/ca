//! A numbered choice with no tool call ends the turn until the user answers.

#[must_use]
pub(crate) fn awaiting_user_choice(content: &str, has_tool_calls: bool) -> bool {
    if has_tool_calls || !content.contains('?') {
        return false;
    }
    let options = content.lines().filter(|line| numbered_option(line)).count();
    options >= 2
}

fn numbered_option(line: &str) -> bool {
    let trimmed = line.trim().trim_start_matches(['*', '#', ' ']);
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_digit() {
        return false;
    }
    let rest: String = chars.collect();
    let rest = rest.trim_start_matches(|ch: char| ch.is_ascii_digit());
    rest.starts_with('.') || rest.starts_with(')')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_numbered_question_waits_and_a_finished_report_does_not() {
        let question =
            "Which do you want?\n\n1. Sync forward\n2. Replace remote\n3. Something else\n";
        assert!(awaiting_user_choice(question, false));
        assert!(!awaiting_user_choice(question, true));
        assert!(!awaiting_user_choice(
            "Done. Pushed the tree. The version is 0.1.190.",
            false
        ));
        assert!(!awaiting_user_choice("1. One option only?\n", false));
    }
}
