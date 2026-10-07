//! Pure string formatting and truncation helpers.
//!
//! No IO, no state mutation.  Every function takes scalar inputs and returns
//! `String` or `Option<String>`.

use std::path::Path;
use std::time::Duration;

use crate::config::Config;
use crate::paths::Paths;

// ---------------------------------------------------------------------------
// Inline / center truncation
// ---------------------------------------------------------------------------

pub(crate) fn truncate_inline(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    for _ in 0..max_chars {
        let Some(ch) = chars.next() else {
            return out;
        };
        out.push(if ch == '\n' { ' ' } else { ch });
    }
    if chars.next().is_some() {
        out.push_str("...");
    }
    out
}

pub(crate) fn truncate_center(input: &str, max_chars: usize) -> String {
    let chars = input.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return input.to_string();
    }
    if max_chars <= 1 {
        return "…".to_string();
    }
    let left_len = max_chars / 2;
    let right_len = max_chars - left_len - 1;
    let left = chars.iter().take(left_len).collect::<String>();
    let right = chars
        .iter()
        .skip(chars.len().saturating_sub(right_len))
        .collect::<String>();
    format!("{left}…{right}")
}

// ---------------------------------------------------------------------------
// Numeric / duration formatting
// ---------------------------------------------------------------------------

pub(crate) fn format_count(n: usize) -> String {
    if n >= 1_000 {
        let whole = n / 1_000;
        let tenth = (n % 1_000) / 100;
        if tenth == 0 {
            format!("{whole}k")
        } else {
            format!("{whole}.{tenth}k")
        }
    } else {
        n.to_string()
    }
}

pub(crate) fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs >= 60 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else if secs > 0 {
        format!("{secs}s")
    } else {
        format!("{}ms", duration.as_millis())
    }
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

pub(crate) fn display_path(paths: &Paths, path: &Path) -> String {
    path.strip_prefix(&paths.root)
        .map(|rel| rel.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

pub(crate) fn short_path(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

// ---------------------------------------------------------------------------
// Role label resolution
// ---------------------------------------------------------------------------

pub(crate) fn role_label(cfg: &Config, role_name: &str) -> String {
    match role_name {
        "master" => cfg.role_labels.master.clone(),
        "planner" => cfg.role_labels.planner.clone(),
        "foreman-plan" => cfg.role_labels.planner.clone(),
        "foreman-review" => format!("{} Review", cfg.role_labels.foreman),
        "worker" => cfg.role_labels.worker.clone(),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Text normalisation
// ---------------------------------------------------------------------------

pub(crate) fn sentence_case(text: &str) -> String {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_uppercase().chain(chars).collect()
}

pub(crate) fn unquote_shell_arg(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 {
        let first = s.as_bytes()[0];
        let last = s.as_bytes()[s.len() - 1];
        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            return &s[1..s.len() - 1];
        }
    }
    s
}

pub(crate) fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Terminal text wrapping
// ---------------------------------------------------------------------------

pub(crate) fn wrap_terminal_text(value: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in normalized.split(' ') {
        if word.chars().count() > width {
            if !current.is_empty() {
                lines.push(current);
                current = String::new();
            }
            let mut chunk = String::new();
            for ch in word.chars() {
                if chunk.chars().count() >= width {
                    lines.push(chunk);
                    chunk = String::new();
                }
                chunk.push(ch);
            }
            if !chunk.is_empty() {
                current = chunk;
            }
            continue;
        }
        let separator = usize::from(!current.is_empty());
        if !current.is_empty() && current.chars().count() + separator + word.chars().count() > width
        {
            lines.push(current);
            current = String::new();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
