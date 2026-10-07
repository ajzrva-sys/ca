//! Terminal theme, frame rendering, and role-header printing.
//!
//! Presentation-only helpers that draw bordered frames, colour-tagged rows,
//! and compact role invocation headers to the terminal.

use crate::config::RoleConfig;

use super::formatting::{format_count, truncate_center};

// ---------------------------------------------------------------------------
// Theme (ANSI colour wrapper)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub(crate) struct Theme {
    pub(crate) color: bool,
}

impl Theme {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            color: enabled && terminal_color_allowed(),
        }
    }

    pub(crate) fn tag(&self, text: &str) -> String {
        self.paint("36;1", &format!("[{text}]"))
    }

    pub(crate) fn section(&self, text: &str) -> String {
        let width = frame_width();
        let label = format!(" {text} ");
        let dash_count = width.saturating_sub(label.chars().count() + 2);
        self.paint("36", &format!("+{label}{}+", "-".repeat(dash_count)))
    }

    pub(crate) fn border(&self, text: &str) -> String {
        self.paint("36", text)
    }

    pub(crate) fn title(&self, text: &str) -> String {
        self.paint("35;1", text)
    }

    pub(crate) fn label(&self, text: &str) -> String {
        self.paint("33;1", text)
    }

    pub(crate) fn meta(&self, text: &str) -> String {
        self.paint("37;1", text)
    }

    pub(crate) fn command(&self, text: &str) -> String {
        self.paint("34;1", text)
    }

    pub(crate) fn status(&self, text: &str, ok: bool) -> String {
        if ok {
            self.paint("32;1", text)
        } else {
            self.paint("31;1", text)
        }
    }

    pub(crate) fn warning(&self, text: &str) -> String {
        self.paint("33;1", text)
    }

    pub(crate) fn error(&self, text: &str) -> String {
        self.paint("31;1", text)
    }

    pub(crate) fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// Frame utilities
// ---------------------------------------------------------------------------

pub(crate) fn frame_width() -> usize {
    std::env::var("CODEX_TEAM_FRAME_WIDTH")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .map(|width| width.clamp(72, 160))
        .unwrap_or(112)
}

pub(crate) fn frame_rule(width: usize) -> String {
    format!("+{}+", "-".repeat(width.saturating_sub(2)))
}

pub(crate) fn print_frame_row(theme: &Theme, width: usize, segments: &[(String, usize)]) {
    let inner_width = width.saturating_sub(4);
    let mut visible = String::new();
    let mut plain_len = 0;
    for (idx, (styled, plain_width)) in segments.iter().enumerate() {
        if idx > 0 {
            visible.push_str("  ");
            plain_len += 2;
        }
        visible.push_str(styled);
        plain_len += plain_width;
    }

    let padding = inner_width.saturating_sub(plain_len);
    println!(
        "{} {}{} {}",
        theme.border("|"),
        visible,
        " ".repeat(padding),
        theme.border("|")
    );
}

fn terminal_color_allowed() -> bool {
    std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM")
            .map(|term| term != "dumb")
            .unwrap_or(true)
}

// ---------------------------------------------------------------------------
// Role-header printing
// ---------------------------------------------------------------------------

pub(crate) fn print_role_header(
    theme: &Theme,
    role_label: &str,
    role: &RoleConfig,
    prompt_tokens: usize,
    raw_log: &str,
    timeout_seconds: u64,
) {
    let width = frame_width();
    let inner_width = width.saturating_sub(4);
    let prompt_value = format!("~{} tokens", format_count(prompt_tokens));
    let raw_prefix_width =
        "PROMPT".chars().count() + prompt_value.chars().count() + "RAW".chars().count() + 6;
    let raw_width = inner_width.saturating_sub(raw_prefix_width).max(16);
    let raw_log = truncate_center(raw_log, raw_width);
    println!("{}", theme.border(&frame_rule(width)));
    print_frame_row(
        theme,
        width,
        &[
            (theme.title("CODEX TEAM"), 10),
            (theme.meta(role_label), role_label.chars().count()),
        ],
    );
    print_frame_row(
        theme,
        width,
        &[
            (theme.label("MODEL"), 5),
            (theme.meta(&role.model), role.model.chars().count()),
            (theme.label("EFFORT"), 6),
            (theme.meta(&role.effort), role.effort.chars().count()),
            (theme.label("SANDBOX"), 7),
            (theme.meta(&role.sandbox), role.sandbox.chars().count()),
            (theme.label("TIMEOUT"), 7),
            (
                theme.meta(&format!("{timeout_seconds}s")),
                format!("{timeout_seconds}s").chars().count(),
            ),
        ],
    );
    print_frame_row(
        theme,
        width,
        &[
            (theme.label("PROMPT"), 6),
            (theme.meta(&prompt_value), prompt_value.chars().count()),
            (theme.label("RAW"), 3),
            (theme.meta(&raw_log), raw_log.chars().count()),
        ],
    );
    println!("{}", theme.border(&frame_rule(width)));
}

pub(crate) fn print_codex_role_header(
    theme: &Theme,
    role_label: &str,
    role: &RoleConfig,
    prompt_tokens: usize,
    raw_log: &str,
) {
    let raw_log = truncate_center(raw_log, 56);
    println!(
        "\n{}",
        theme.dim(&format!(
            "{role_label} · {} · {} · prompt ~{} tokens · raw {raw_log}",
            role.model,
            role.effort,
            format_count(prompt_tokens)
        ))
    );
}
