//! One follow-up after a code edit when auto-review is off.
//! The follow-up asks for a check. It does not run the check.

const PROSE_EXTENSIONS: &[&str] = &[
    "md", "markdown", "mdx", "rst", "txt", "text", "adoc", "asciidoc", "org", "log", "csv", "tsv",
];

const PROSE_NAMES: &[&str] = &[
    "license",
    "licence",
    "notice",
    "authors",
    "contributors",
    "changelog",
    "codeowners",
];

pub fn is_prose_path(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    if PROSE_NAMES.iter().any(|item| *item == lower) {
        return true;
    }
    lower
        .rsplit_once('.')
        .is_some_and(|(_, extension)| PROSE_EXTENSIONS.contains(&extension))
}

/// A direct command is one whose own exit status is the command's status.
/// A pipe does not count.
pub fn command_is_direct(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return false;
    }
    let mut quote: Option<char> = None;
    for ch in trimmed.chars() {
        match (quote, ch) {
            (None, '\'' | '"') => quote = Some(ch),
            (Some(open), ch) if ch == open => quote = None,
            (None, '|') => return false,
            _ => {}
        }
    }
    true
}

pub fn follow_up(paths: &[String]) -> String {
    let shown = paths
        .iter()
        .take(8)
        .map(|path| format!("`{path}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let extra = paths.len().saturating_sub(8);
    let more = if extra > 0 {
        format!(" and {extra} more")
    } else {
        String::new()
    };
    format!(
        "The code edit is not checked yet. Run the check as the first command in the pipeline, with no pipe before it. Changed paths: {shown}{more}."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prose_paths_skip_and_a_pipe_is_not_a_direct_check() {
        assert!(is_prose_path("docs/hermes-ca-recommendations.md"));
        assert!(is_prose_path("notes.txt"));
        assert!(is_prose_path("LICENSE"));
        assert!(is_prose_path("CHANGELOG"));
        assert!(!is_prose_path("src/main.rs"));
        assert!(command_is_direct("cargo test --bin ca"));
        assert!(!command_is_direct("cargo test | tail"));
        assert!(command_is_direct("printf 'a|b'"));
        let paths = (0..10)
            .map(|index| format!("src/file{index}.rs"))
            .collect::<Vec<_>>();
        let prompt = follow_up(&paths);
        assert!(prompt.contains("`src/file0.rs`"));
        assert!(prompt.contains("`src/file7.rs`"));
        assert!(!prompt.contains("`src/file8.rs`"));
        assert!(prompt.contains("2 more"));
        assert!(prompt.contains("first command"));
    }
}
