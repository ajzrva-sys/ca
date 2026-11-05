//! Shared overlay chrome so modal surfaces do not each invent borders and hints.
//! Adopt incrementally — start with new or touched dialogs.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};

pub(super) fn standard_block(title: impl AsRef<str>, border: Color) -> Block<'static> {
    Block::default()
        .title(title.as_ref().to_string())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
}

/// Border-only chrome for dialogs that have no title row (keeps top border clean).
pub(super) fn plain_block(border: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
}

pub(super) fn footer_hint(text: impl AsRef<str>, muted: Color) -> Line<'static> {
    Line::from(Span::styled(
        text.as_ref().to_string(),
        Style::default().fg(muted),
    ))
}

/// Standard Esc/choice hint wording for modal footers.
pub(super) fn esc_cancel_hint(muted: Color) -> Line<'static> {
    footer_hint("Esc cancel", muted)
}
