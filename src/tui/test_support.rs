use crate::app::AppState;
use anyhow::Result;
use ratatui::backend::{Backend, TestBackend};
use ratatui::layout::Position;
use ratatui::Terminal;
use unicode_width::UnicodeWidthStr;

pub(crate) fn test_state() -> AppState {
    let mut state = AppState::new(crate::context::ShellHistory::in_memory());
    state.cwd = "/workspace/ca".to_string();
    state.session_time_offset_seconds = 0;
    state
}

fn composer_snapshot_cursor(state: &AppState, width: u16, height: u16) -> Option<Position> {
    let cursor_byte = state
        .input
        .floor_char_boundary(state.input_cursor.min(state.input.len()));
    let input_width = UnicodeWidthStr::width(&state.input[..cursor_byte]);
    let column = 2_usize.saturating_add(input_width);
    let inner_width = width.saturating_sub(2) as usize;
    if column >= inner_width {
        return None;
    }
    let queue_previews = state.input_queue.len().min(3) as u16;
    let input_height = state.input_height.max(3_u16.saturating_add(queue_previews));
    let y = height
        .saturating_sub(input_height)
        .saturating_add(queue_previews);
    (y < height).then_some(Position::new(1_u16.saturating_add(column as u16), y))
}

pub(crate) fn try_render_to_text(state: &AppState, width: u16, height: u16) -> Result<String> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| crate::tui::render(frame, state))?;

    let backend_cursor = terminal.backend_mut().get_cursor_position()?;
    let cursor = (backend_cursor != Position::ORIGIN)
        .then_some(backend_cursor)
        .or_else(|| composer_snapshot_cursor(state, width, height))
        .unwrap_or(Position::ORIGIN);
    let cursor_is_set = cursor != Position::ORIGIN;
    let cursor_is_at_input_end = state.input_cursor.min(state.input.len()) == state.input.len();
    let buffer = terminal.backend().buffer();
    let cursor_byte = state
        .input
        .floor_char_boundary(state.input_cursor.min(state.input.len()));
    let input_width = UnicodeWidthStr::width(&state.input[..cursor_byte]) as u16;
    let prompt_x = cursor.x.saturating_sub(2_u16.saturating_add(input_width));
    let composer_is_visible = cursor_is_set
        && prompt_x.saturating_add(1) < width
        && cursor.y < height
        && buffer[(prompt_x, cursor.y)].symbol() == "→"
        && buffer[(prompt_x + 1, cursor.y)].symbol() == " ";
    let empty_composer_prefix = if composer_is_visible && state.input.is_empty() {
        let placeholder = if state.turn_lifecycle.is_active() {
            "Add a follow-up…"
        } else {
            "Ask anything…"
        };
        let expected: Vec<String> = ["→ ", placeholder]
            .concat()
            .chars()
            .map(|character| character.to_string())
            .collect();
        let matched = expected
            .iter()
            .enumerate()
            .take_while(|(offset, expected_symbol)| {
                let x = prompt_x.saturating_add(*offset as u16);
                x < width && buffer[(x, cursor.y)].symbol() == expected_symbol.as_str()
            })
            .count();
        Some((expected, matched))
    } else {
        None
    };
    let mut lines = Vec::with_capacity(height as usize);
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            let symbol = buffer[(x, y)].symbol();
            let is_cursor_cell = cursor_is_set && cursor == Position::new(x, y);
            let partial_empty_cursor_symbol =
                empty_composer_prefix
                    .as_ref()
                    .and_then(|(expected, matched)| {
                        let is_full = *matched == expected.len();
                        let offset = x.checked_sub(prompt_x)? as usize;
                        if y != cursor.y || is_full || offset >= *matched || *matched <= 2 {
                            return None;
                        }
                        if offset == 2 {
                            Some("▌")
                        } else if offset > 2 {
                            Some(expected[offset - 1].as_str())
                        } else {
                            None
                        }
                    });
            if let Some(replacement) = partial_empty_cursor_symbol {
                line.push_str(replacement);
                continue;
            }
            let empty_composer_is_full = empty_composer_prefix
                .as_ref()
                .is_some_and(|(expected, matched)| *matched == expected.len());
            if is_cursor_cell && empty_composer_is_full {
                // Text snapshots cannot encode the white background of the
                // software block cursor. Keep the historical marker solely as
                // notation; the runtime buffer changes cell colors, not text.
                line.push('▌');
            }
            if is_cursor_cell
                && composer_is_visible
                && !state.input.is_empty()
                && cursor_is_at_input_end
                && symbol == " "
            {
                line.push('▌');
            } else {
                line.push_str(symbol);
            }
        }
        lines.push(line.trim_end().to_string());
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    Ok(lines.join("\n"))
}

pub(crate) fn render_to_text(state: &AppState, width: u16, height: u16) -> String {
    try_render_to_text(state, width, height).expect("test terminal should render")
}
