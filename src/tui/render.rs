use crate::app::{AppState, PartialToolCall, Role, ToolCall, ToolStatus, TranscriptBlockKey};
use crate::markdown;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

#[path = "transcript_cache.rs"]
mod transcript_cache;
pub(crate) use transcript_cache::TranscriptCache;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)] // Fields used by compact mode, block styling, and scrollbar features
pub(super) struct ThemePalette {
    pub(super) accent: Color,
    pub(super) heading: Color,
    heading_text: Color,
    link: Color,
    plan: Color,
    pub(super) text: Color,
    pub(super) accent_foreground: Color,
    title_hint_background: Color,
    pub(super) surface: Color,
    code_background: Color,
    pub(super) border: Color,
    pub(super) muted: Color,
    strong: Color,
    warning: Color,
    error: Color,
    success: Color,
    // Diff colors
    diff_insert_fg: Color,
    diff_insert_bg: Color,
    diff_delete_fg: Color,
    diff_delete_bg: Color,
    diff_hunk_fg: Color,
    diff_file_header_fg: Color,
    // Paste / selection colors
    paste_fg: Color,
    paste_bg: Color,
    // Markdown heading hierarchy
    md_h1: Color,
    md_h2: Color,
    md_h3: Color,
    md_h4: Color,
    md_h5: Color,
    md_h6: Color,
    // Semantic state colors
    state_needs_input: Color,
    state_completed: Color,
    state_failed: Color,
    state_blocked: Color,
    pub(super) state_idle: Color,
    pub(super) state_working: Color,
    // Scrollbar
    scrollbar_fg: Color,
    scrollbar_bg: Color,
    // Borders
    selection_border: Color,
    hover_border: Color,
}

fn active_theme(state: &AppState) -> crate::ui_settings::UiTheme {
    state
        .theme_manager
        .as_ref()
        .and_then(crate::app::ThemeManagerState::selected_theme)
        .unwrap_or(state.ui_theme)
}

pub(super) fn palette(state: &AppState) -> ThemePalette {
    if state.theme_manager.is_none()
        && state.terminal_theme_auto
        && state.terminal_appearance == Some(crate::terminal_awareness::TerminalAppearance::Light)
    {
        return ThemePalette {
            accent: Color::Blue,
            heading: Color::Blue,
            heading_text: Color::Black,
            link: Color::Blue,
            plan: Color::Green,
            text: Color::Black,
            accent_foreground: Color::White,
            title_hint_background: Color::Gray,
            surface: Color::White,
            code_background: Color::Gray,
            border: Color::DarkGray,
            muted: Color::DarkGray,
            strong: Color::Black,
            warning: Color::Rgb(160, 96, 0),
            error: Color::Red,
            success: Color::Green,
            diff_insert_fg: Color::Rgb(0, 128, 0),
            diff_insert_bg: Color::Rgb(220, 245, 220),
            diff_delete_fg: Color::Rgb(180, 0, 0),
            diff_delete_bg: Color::Rgb(255, 220, 220),
            diff_hunk_fg: Color::Blue,
            diff_file_header_fg: Color::Blue,
            paste_fg: Color::Black,
            paste_bg: Color::Rgb(200, 220, 255),
            md_h1: Color::Blue,
            md_h2: Color::Blue,
            md_h3: Color::Rgb(0, 100, 180),
            md_h4: Color::Rgb(0, 100, 180),
            md_h5: Color::Rgb(80, 80, 80),
            md_h6: Color::Rgb(80, 80, 80),
            state_needs_input: Color::Rgb(180, 130, 0),
            state_completed: Color::Green,
            state_failed: Color::Red,
            state_blocked: Color::Rgb(200, 120, 0),
            state_idle: Color::DarkGray,
            state_working: Color::Blue,
            scrollbar_fg: Color::DarkGray,
            scrollbar_bg: Color::Reset,
            selection_border: Color::Blue,
            hover_border: Color::Rgb(100, 150, 200),
        };
    }
    match active_theme(state) {
        crate::ui_settings::UiTheme::Default => ThemePalette {
            accent: Color::Rgb(189, 147, 249),
            heading: Color::Rgb(214, 214, 214),
            heading_text: Color::Rgb(214, 214, 214),
            link: Color::Rgb(137, 183, 235),
            plan: Color::Rgb(151, 190, 122),
            text: Color::Rgb(214, 214, 214),
            accent_foreground: Color::Rgb(17, 17, 17),
            title_hint_background: Color::Rgb(38, 38, 38),
            surface: Color::Rgb(17, 17, 17),
            code_background: Color::Rgb(30, 30, 30),
            border: Color::Rgb(83, 83, 89),
            muted: Color::Rgb(139, 139, 139),
            strong: Color::Rgb(235, 235, 235),
            warning: Color::Rgb(229, 190, 111),
            error: Color::Rgb(240, 126, 126),
            success: Color::Rgb(134, 173, 100),
            diff_insert_fg: Color::Rgb(151, 190, 122),
            diff_insert_bg: Color::Rgb(24, 49, 24),
            diff_delete_fg: Color::Rgb(240, 126, 126),
            diff_delete_bg: Color::Rgb(61, 27, 27),
            diff_hunk_fg: Color::Rgb(137, 183, 235),
            diff_file_header_fg: Color::Rgb(214, 214, 214),
            paste_fg: Color::Rgb(214, 214, 214),
            paste_bg: Color::Rgb(42, 42, 57),
            md_h1: Color::Rgb(235, 235, 235),
            md_h2: Color::Rgb(214, 214, 214),
            md_h3: Color::Rgb(189, 147, 249),
            md_h4: Color::Rgb(189, 147, 249),
            md_h5: Color::Rgb(139, 139, 139),
            md_h6: Color::Rgb(139, 139, 139),
            state_needs_input: Color::Rgb(229, 190, 111),
            state_completed: Color::Rgb(134, 173, 100),
            state_failed: Color::Rgb(240, 126, 126),
            state_blocked: Color::Rgb(229, 190, 111),
            state_idle: Color::Rgb(139, 139, 139),
            state_working: Color::Rgb(139, 139, 139),
            scrollbar_fg: Color::Rgb(83, 83, 89),
            scrollbar_bg: Color::Rgb(17, 17, 17),
            selection_border: Color::Rgb(189, 147, 249),
            hover_border: Color::Rgb(139, 139, 139),
        },
        crate::ui_settings::UiTheme::Ocean => ThemePalette {
            accent: Color::Rgb(82, 165, 255),
            heading: Color::Rgb(113, 183, 255),
            heading_text: Color::Rgb(113, 183, 255),
            link: Color::Rgb(113, 183, 255),
            plan: Color::Rgb(144, 221, 181),
            text: Color::Rgb(232, 242, 250),
            accent_foreground: Color::Black,
            title_hint_background: Color::Rgb(25, 55, 82),
            surface: Color::Rgb(18, 34, 48),
            code_background: Color::Rgb(18, 34, 48),
            border: Color::Rgb(67, 96, 121),
            muted: Color::Rgb(102, 128, 153),
            strong: Color::Rgb(255, 207, 112),
            warning: Color::Rgb(255, 196, 87),
            error: Color::Rgb(255, 107, 107),
            success: Color::Rgb(91, 214, 153),
            diff_insert_fg: Color::Rgb(91, 214, 153),
            diff_insert_bg: Color::Rgb(20, 55, 40),
            diff_delete_fg: Color::Rgb(255, 107, 107),
            diff_delete_bg: Color::Rgb(65, 25, 30),
            diff_hunk_fg: Color::Rgb(113, 183, 255),
            diff_file_header_fg: Color::Rgb(113, 183, 255),
            paste_fg: Color::Rgb(232, 242, 250),
            paste_bg: Color::Rgb(30, 60, 100),
            md_h1: Color::Rgb(82, 165, 255),
            md_h2: Color::Rgb(90, 175, 255),
            md_h3: Color::Rgb(113, 183, 255),
            md_h4: Color::Rgb(113, 183, 255),
            md_h5: Color::Rgb(102, 128, 153),
            md_h6: Color::Rgb(102, 128, 153),
            state_needs_input: Color::Rgb(255, 196, 87),
            state_completed: Color::Rgb(91, 214, 153),
            state_failed: Color::Rgb(255, 107, 107),
            state_blocked: Color::Rgb(255, 165, 87),
            state_idle: Color::Rgb(102, 128, 153),
            state_working: Color::Rgb(82, 165, 255),
            scrollbar_fg: Color::Rgb(67, 96, 121),
            scrollbar_bg: Color::Rgb(18, 34, 48),
            selection_border: Color::Rgb(82, 165, 255),
            hover_border: Color::Rgb(100, 150, 200),
        },
        crate::ui_settings::UiTheme::Amber => ThemePalette {
            accent: Color::Rgb(255, 184, 77),
            heading: Color::Rgb(255, 197, 109),
            heading_text: Color::Rgb(255, 197, 109),
            link: Color::Rgb(255, 197, 109),
            plan: Color::Rgb(171, 220, 133),
            text: Color::Rgb(244, 235, 218),
            accent_foreground: Color::Black,
            title_hint_background: Color::Rgb(79, 58, 28),
            surface: Color::Rgb(38, 31, 20),
            code_background: Color::Rgb(38, 31, 20),
            border: Color::Rgb(108, 84, 49),
            muted: Color::Rgb(145, 119, 78),
            strong: Color::Rgb(255, 211, 92),
            warning: Color::Yellow,
            error: Color::Rgb(255, 99, 92),
            success: Color::Rgb(126, 211, 91),
            diff_insert_fg: Color::Rgb(126, 211, 91),
            diff_insert_bg: Color::Rgb(30, 50, 25),
            diff_delete_fg: Color::Rgb(255, 99, 92),
            diff_delete_bg: Color::Rgb(60, 25, 25),
            diff_hunk_fg: Color::Rgb(255, 197, 109),
            diff_file_header_fg: Color::Rgb(255, 197, 109),
            paste_fg: Color::Rgb(244, 235, 218),
            paste_bg: Color::Rgb(70, 55, 30),
            md_h1: Color::Rgb(255, 184, 77),
            md_h2: Color::Rgb(255, 197, 109),
            md_h3: Color::Rgb(255, 207, 130),
            md_h4: Color::Rgb(255, 207, 130),
            md_h5: Color::Rgb(145, 119, 78),
            md_h6: Color::Rgb(145, 119, 78),
            state_needs_input: Color::Yellow,
            state_completed: Color::Rgb(126, 211, 91),
            state_failed: Color::Rgb(255, 99, 92),
            state_blocked: Color::Rgb(255, 165, 77),
            state_idle: Color::Rgb(145, 119, 78),
            state_working: Color::Rgb(255, 184, 77),
            scrollbar_fg: Color::Rgb(108, 84, 49),
            scrollbar_bg: Color::Rgb(38, 31, 20),
            selection_border: Color::Rgb(255, 184, 77),
            hover_border: Color::Rgb(200, 150, 80),
        },
        crate::ui_settings::UiTheme::Mimocode => ThemePalette {
            accent: Color::Rgb(255, 106, 0),
            heading: Color::Rgb(137, 147, 255),
            heading_text: Color::Rgb(137, 147, 255),
            link: Color::Rgb(137, 147, 255),
            plan: Color::Rgb(194, 226, 164),
            text: Color::Rgb(235, 235, 235),
            accent_foreground: Color::Black,
            title_hint_background: Color::Rgb(22, 22, 22),
            surface: Color::Rgb(30, 30, 30),
            code_background: Color::Rgb(30, 30, 30),
            border: Color::Rgb(91, 91, 91),
            muted: Color::Rgb(132, 132, 132),
            strong: Color::Rgb(255, 194, 35),
            warning: Color::Rgb(255, 194, 35),
            error: Color::Rgb(255, 91, 95),
            success: Color::Rgb(194, 226, 164),
            diff_insert_fg: Color::Rgb(194, 226, 164),
            diff_insert_bg: Color::Rgb(25, 45, 25),
            diff_delete_fg: Color::Rgb(255, 91, 95),
            diff_delete_bg: Color::Rgb(55, 20, 22),
            diff_hunk_fg: Color::Rgb(137, 147, 255),
            diff_file_header_fg: Color::Rgb(137, 147, 255),
            paste_fg: Color::Rgb(235, 235, 235),
            paste_bg: Color::Rgb(50, 50, 70),
            md_h1: Color::Rgb(255, 106, 0),
            md_h2: Color::Rgb(137, 147, 255),
            md_h3: Color::Rgb(160, 170, 255),
            md_h4: Color::Rgb(160, 170, 255),
            md_h5: Color::Rgb(132, 132, 132),
            md_h6: Color::Rgb(132, 132, 132),
            state_needs_input: Color::Rgb(255, 194, 35),
            state_completed: Color::Rgb(194, 226, 164),
            state_failed: Color::Rgb(255, 91, 95),
            state_blocked: Color::Rgb(255, 150, 50),
            state_idle: Color::Rgb(132, 132, 132),
            state_working: Color::Rgb(255, 106, 0),
            scrollbar_fg: Color::Rgb(91, 91, 91),
            scrollbar_bg: Color::Rgb(30, 30, 30),
            selection_border: Color::Rgb(255, 106, 0),
            hover_border: Color::Rgb(180, 120, 60),
        },
        crate::ui_settings::UiTheme::Mono => ThemePalette {
            accent: Color::White,
            heading: Color::White,
            heading_text: Color::White,
            link: Color::White,
            plan: Color::White,
            text: Color::White,
            accent_foreground: Color::Black,
            title_hint_background: Color::DarkGray,
            surface: Color::Black,
            code_background: Color::DarkGray,
            border: Color::Gray,
            muted: Color::Gray,
            strong: Color::White,
            warning: Color::White,
            error: Color::LightRed,
            success: Color::White,
            diff_insert_fg: Color::White,
            diff_insert_bg: Color::DarkGray,
            diff_delete_fg: Color::LightRed,
            diff_delete_bg: Color::Rgb(40, 0, 0),
            diff_hunk_fg: Color::Gray,
            diff_file_header_fg: Color::White,
            paste_fg: Color::White,
            paste_bg: Color::DarkGray,
            md_h1: Color::White,
            md_h2: Color::White,
            md_h3: Color::White,
            md_h4: Color::Gray,
            md_h5: Color::Gray,
            md_h6: Color::Gray,
            state_needs_input: Color::White,
            state_completed: Color::White,
            state_failed: Color::LightRed,
            state_blocked: Color::White,
            state_idle: Color::Gray,
            state_working: Color::White,
            scrollbar_fg: Color::Gray,
            scrollbar_bg: Color::Black,
            selection_border: Color::White,
            hover_border: Color::Gray,
        },
        crate::ui_settings::UiTheme::RosePine => ThemePalette {
            accent: Color::Rgb(196, 167, 231),
            heading: Color::Rgb(196, 167, 231),
            heading_text: Color::Rgb(224, 222, 244),
            link: Color::Rgb(156, 207, 216),
            plan: Color::Rgb(166, 218, 149),
            text: Color::Rgb(224, 222, 244),
            accent_foreground: Color::Black,
            title_hint_background: Color::Rgb(38, 36, 53),
            surface: Color::Rgb(34, 33, 48),
            code_background: Color::Rgb(38, 36, 53),
            border: Color::Rgb(87, 82, 121),
            muted: Color::Rgb(124, 118, 157),
            strong: Color::Rgb(235, 188, 186),
            warning: Color::Rgb(246, 193, 119),
            error: Color::Rgb(235, 111, 146),
            success: Color::Rgb(166, 218, 149),
            diff_insert_fg: Color::Rgb(166, 218, 149),
            diff_insert_bg: Color::Rgb(25, 45, 30),
            diff_delete_fg: Color::Rgb(235, 111, 146),
            diff_delete_bg: Color::Rgb(55, 20, 30),
            diff_hunk_fg: Color::Rgb(196, 167, 231),
            diff_file_header_fg: Color::Rgb(196, 167, 231),
            paste_fg: Color::Rgb(224, 222, 244),
            paste_bg: Color::Rgb(50, 45, 70),
            md_h1: Color::Rgb(196, 167, 231),
            md_h2: Color::Rgb(196, 167, 231),
            md_h3: Color::Rgb(210, 190, 240),
            md_h4: Color::Rgb(210, 190, 240),
            md_h5: Color::Rgb(124, 118, 157),
            md_h6: Color::Rgb(124, 118, 157),
            state_needs_input: Color::Rgb(246, 193, 119),
            state_completed: Color::Rgb(166, 218, 149),
            state_failed: Color::Rgb(235, 111, 146),
            state_blocked: Color::Rgb(246, 193, 119),
            state_idle: Color::Rgb(124, 118, 157),
            state_working: Color::Rgb(196, 167, 231),
            scrollbar_fg: Color::Rgb(87, 82, 121),
            scrollbar_bg: Color::Rgb(34, 33, 48),
            selection_border: Color::Rgb(196, 167, 231),
            hover_border: Color::Rgb(140, 120, 180),
        },
    }
}

fn markdown_palette(theme: ThemePalette) -> markdown::MarkdownPalette {
    markdown::MarkdownPalette {
        text: theme.text,
        heading: theme.heading,
        heading_text: theme.heading_text,
        accent: theme.accent,
        link: theme.link,
        strong: theme.strong,
        muted: theme.muted,
        border: theme.border,
        code_background: theme.code_background,
        md_h1: theme.md_h1,
        md_h2: theme.md_h2,
        md_h3: theme.md_h3,
        md_h4: theme.md_h4,
        md_h5: theme.md_h5,
        md_h6: theme.md_h6,
    }
}

/// Returns the RGB components of the current theme's accent color.
pub fn theme_accent_rgb(state: &AppState) -> (u8, u8, u8) {
    let accent = palette(state).accent;
    match accent {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Cyan => (0, 255, 255),
        Color::Blue => (0, 0, 255),
        Color::White => (255, 255, 255),
        _ => (0, 255, 255),
    }
}

pub(crate) const FILTER_BANNER_LABEL: &str = "FILTER ON";
pub(crate) const PROVIDER_PANE_LABEL: &str = "TO PROVIDER";
/// Cyan from the confidential classification bar, with black text.
pub(crate) const FILTER_BANNER_BG: Color = Color::Rgb(92, 227, 254);
pub(crate) const FILTER_BANNER_FG: Color = Color::Black;

pub fn render(frame: &mut ratatui::Frame, state: &AppState) {
    render_with_version_label(frame, state, super::VERSION_LABEL);
}

pub(super) fn render_with_version_label(
    frame: &mut ratatui::Frame,
    state: &AppState,
    version_label: &str,
) {
    let (banner, summary, below) = split_filter_banner(frame.area(), state);
    if banner.is_some() && summary.is_none() {
        paint_filter_status_line(frame, below, state);
        paint_filter_banner(frame, banner);
        return;
    }
    let (area, provider) = split_provider_pane(below, state);
    let Some(deck) = state.pane_deck.as_ref().filter(|deck| !deck.is_single()) else {
        render_pane(frame, area, state, true, version_label);
        paint_provider_pane(frame, provider, state);
        paint_filter_summary(frame, summary, state);
        paint_filter_banner(frame, banner);
        return;
    };

    for (id, region) in deck.layout().regions(area) {
        let Some(pane) = deck.pane(state, id) else {
            continue;
        };
        let focused = deck.focused() == id;
        let activity = if pane.interaction.is_some() {
            "approval"
        } else if pane.streaming.is_some() || pane.turn_lifecycle.is_active() {
            "working"
        } else if !pane.pending_job_completions.is_empty() {
            "done"
        } else {
            "idle"
        };
        let title = format!(
            " {} pane {} · {} ",
            if focused { "●" } else { "○" },
            id,
            activity
        );
        frame.render_widget(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if focused {
                    palette(pane).accent
                } else {
                    palette(pane).border
                })),
            region,
        );
        let inner = region.inner(Margin {
            horizontal: 1,
            vertical: 1,
        });
        render_pane(frame, inner, pane, focused, version_label);
    }
    paint_provider_pane(frame, provider, state);
    paint_filter_summary(frame, summary, state);
    paint_filter_banner(frame, banner);
}

fn split_filter_banner(area: Rect, state: &AppState) -> (Option<Rect>, Option<Rect>, Rect) {
    if !state
        .filter_banner
        .load(std::sync::atomic::Ordering::Relaxed)
        || area.height == 0
        || area.width == 0
    {
        return (None, None, area);
    }
    let rows = if area.height >= 3 { 2 } else { 1 };
    let banner = Some(Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    });
    let summary = if rows == 2 {
        Some(Rect {
            x: area.x,
            y: area.y.saturating_add(1),
            width: area.width,
            height: 1,
        })
    } else {
        None
    };
    let rest = Rect {
        x: area.x,
        y: area.y.saturating_add(rows),
        width: area.width,
        height: area.height.saturating_sub(rows),
    };
    (banner, summary, rest)
}

fn paint_filter_banner(frame: &mut ratatui::Frame, banner: Option<Rect>) {
    let Some(area) = banner else {
        return;
    };
    paint_classified_row(frame, area, FILTER_BANNER_LABEL);
}

fn paint_filter_summary(frame: &mut ratatui::Frame, summary: Option<Rect>, state: &AppState) {
    let Some(area) = summary else {
        return;
    };
    paint_left_row(frame, area, &filter_summary_text(state), classified_style());
}

fn paint_filter_status_line(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let style = Style::default().fg(palette(state).warning);
    paint_left_row(frame, area, &filter_summary_text(state), style);
}

fn paint_left_row(frame: &mut ratatui::Frame, area: Rect, text: &str, style: Style) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let width = area.width as usize;
    let mut line: String = text.chars().take(width).collect();
    if line.len() < width {
        line.push_str(&" ".repeat(width - line.len()));
    }
    frame.buffer_mut().set_string(area.x, area.y, line, style);
}

fn filter_summary_text(state: &AppState) -> String {
    state
        .filter_summary
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

fn classified_style() -> Style {
    Style::default()
        .fg(FILTER_BANNER_FG)
        .bg(FILTER_BANNER_BG)
        .add_modifier(Modifier::BOLD)
}

fn paint_classified_row(frame: &mut ratatui::Frame, area: Rect, label: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.buffer_mut().set_string(
        area.x,
        area.y,
        classified_banner_text(label, area.width as usize),
        classified_style(),
    );
}

/// One centered label on a solid bar. The label is the only text.
fn classified_banner_text(label: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let label: String = label.chars().take(width).collect();
    let pad = width - label.len();
    let left = pad / 2;
    let mut line = " ".repeat(left);
    line.push_str(&label);
    line.push_str(&" ".repeat(pad - left));
    line
}

fn split_provider_pane(area: Rect, state: &AppState) -> (Rect, Option<Rect>) {
    let width = provider_pane_width(area, state);
    if width == 0 {
        return (area, None);
    }
    (
        Rect {
            x: area.x,
            y: area.y,
            width: area.width.saturating_sub(width),
            height: area.height,
        },
        Some(Rect {
            x: area.x.saturating_add(area.width.saturating_sub(width)),
            y: area.y,
            width,
            height: area.height,
        }),
    )
}

fn provider_pane_width(area: Rect, state: &AppState) -> u16 {
    let summary_empty = filter_summary_text(state).is_empty();
    if !state
        .filter_banner
        .load(std::sync::atomic::Ordering::Relaxed)
        || (state.provider_pane.is_empty() && summary_empty)
        || area.width < 80
        || area.height < 8
    {
        return 0;
    }
    let pane = u16::try_from(u32::from(area.width) * 2 / 5).unwrap_or(area.width);
    let pane = pane.clamp(28, area.width / 2);
    if area.width.saturating_sub(pane) < 40 {
        return 0;
    }
    pane
}

fn paint_provider_pane(frame: &mut ratatui::Frame, pane: Option<Rect>, state: &AppState) {
    let Some(pane) = pane else {
        state.provider_pane.clear_viewport();
        return;
    };
    if pane.width < 12 || pane.height < 3 {
        state.provider_pane.clear_viewport();
        return;
    }
    let style = classified_style();
    let top = Rect {
        x: pane.x,
        y: pane.y,
        width: pane.width,
        height: 1,
    };
    let bottom = Rect {
        x: pane.x,
        y: pane.y.saturating_add(pane.height.saturating_sub(1)),
        width: pane.width,
        height: 1,
    };
    paint_classified_row(frame, top, PROVIDER_PANE_LABEL);
    paint_classified_row(frame, bottom, PROVIDER_PANE_LABEL);
    let content = Rect {
        x: pane.x.saturating_add(1),
        y: pane.y.saturating_add(1),
        width: pane.width.saturating_sub(1),
        height: pane.height.saturating_sub(2),
    };
    for row in content.y..content.y.saturating_add(content.height) {
        frame.buffer_mut().set_string(pane.x, row, " ", style);
    }
    let text = provider_copy_text(state);
    let theme = palette(state);
    let paragraph = Paragraph::new(text.as_str())
        .style(Style::default().fg(theme.text).bg(theme.surface))
        .wrap(Wrap { trim: false });
    let line_count = paragraph.line_count(content.width);
    state
        .provider_pane
        .set_viewport(pane, content.height, line_count);
    frame.render_widget(paragraph.scroll((state.provider_pane.scroll(), 0)), content);
}

fn provider_copy_text(state: &AppState) -> String {
    let summary = filter_summary_text(state);
    let body = state.provider_pane.text();
    if summary.is_empty() {
        body.to_string()
    } else if body.is_empty() {
        summary
    } else {
        format!("{summary}\n{body}")
    }
}

fn render_pane(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &AppState,
    focused: bool,
    version_label: &str,
) {
    if let Some(editor) = state.personality_editor.as_ref() {
        crate::tui::personality::render_with_version_label(frame, area, editor, version_label);
        return;
    }
    let theme = palette(state);
    frame.render_widget(
        Block::default().style(Style::default().fg(theme.text).bg(theme.surface)),
        area,
    );
    if state.dashboard.open {
        crate::tui::dashboard::render_with_version_label(frame, area, state, version_label);
        if state.session_picker_open {
            render_session_picker(frame, area, state);
        }
        if state.pager_open {
            render_pager(frame, area, state);
        }
        if state.slash_palette_open {
            render_slash_palette(frame, area, state);
        }
        return;
    }
    let composer_width = area.width.saturating_sub(2) as usize;
    let composer_layout = crate::tui::composer::ComposerLayout::new(
        &state.input,
        state.input_cursor,
        composer_width,
        2,
    );
    let composer_rows = composer_layout
        .visible_window(state.composer_ui.viewport_top)
        .end
        .saturating_sub(
            composer_layout
                .visible_window(state.composer_ui.viewport_top)
                .top,
        ) as u16;
    let queue_rows = state.input_queue.len().min(3) as u16 + u16::from(state.input_queue.len() > 3);
    let attachment_rows = u16::from(!state.pasted_images.is_empty() || state.image_attachment_busy);
    let paste_preview_rows = u16::from(
        state
            .composer_ui
            .active_large_paste_index(state.input_cursor)
            .is_some(),
    ) * (crate::tui::composer::PASTE_PREVIEW_ROWS as u16 + 1);
    let input_height = state
        .input_height
        .max(2 + composer_rows + queue_rows + attachment_rows + paste_preview_rows);
    let compact = state.ui_compact_mode;
    let title_rows: u16 = 1;
    let status_rows: u16 = if compact { 0 } else { 1 };
    let activity_rows = background_activity_rows(
        state,
        area.height
            .saturating_sub(title_rows + input_height + status_rows + 1),
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(title_rows),    // title bar
            Constraint::Min(1),                // conversation
            Constraint::Length(activity_rows), // activity strip
            Constraint::Length(input_height),  // input box (dynamic)
            Constraint::Length(status_rows),   // status bar
        ])
        .split(area);

    render_title(frame, chunks[0], state, version_label);
    let transcript_top = render_conversation(frame, chunks[1], state, focused);
    render_background_activity(frame, chunks[2], state);
    render_input(frame, chunks[3], state, focused);
    render_status(frame, chunks[4], state);
    state.transcript_scroll.capture_selectable_frame_at(
        frame.buffer_mut(),
        chunks[1],
        u32::from(transcript_top),
    );
    state.transcript_scroll.render_selection(frame.buffer_mut());

    if !focused {
        state.transcript_scroll.clear_activity_hit_targets();
        state.transcript_scroll.clear_user_prompt_hit_targets();
        return;
    }

    if state.plan_workflow_graph.is_some() {
        render_plan_workflow_graph(frame, area, state);
    }
    if state.model_picker_open {
        render_model_picker(frame, area, state);
    }
    if state.config_hub.is_some() {
        render_config_hub(frame, area, state);
    }
    if state.sandbox_hub.is_some() {
        render_sandbox_hub(frame, area, state);
    }
    if state.context_limit_editor.is_some() {
        render_context_limit_editor(frame, area, state);
    }
    if state.provider_manager.is_some() {
        render_provider_manager(frame, area, state);
    }
    if state.provider_discovery.is_some() {
        render_provider_discovery(frame, area, state);
    }
    if state.provider_editor.is_some() {
        render_provider_editor(frame, area, state);
    }
    if state.model_prompt_cache_editor.is_some() {
        render_model_prompt_cache_editor(frame, area, state);
    }
    if state.provider_auth.is_some() {
        render_provider_auth(frame, area, state);
    }
    if state.slash_palette_open {
        render_slash_palette(frame, area, state);
    }
    if state.session_picker_open {
        render_session_picker(frame, area, state);
    }
    if state.session_portability.is_some() {
        render_session_portability(frame, area, state);
    }
    if state.session_retention.is_some() {
        render_session_retention(frame, area, state);
    }
    if state.timeline_open {
        render_timeline(frame, area, state);
    }
    if state.queue_view_open {
        render_queue_view(frame, area, state);
    }
    if state.pager_open {
        render_pager(frame, area, state);
    }
    if state.usage_modal.is_some() {
        crate::tui::dashboard::render_usage_modal(frame, area, state);
    }
    if state.file_picker_open {
        render_file_picker(frame, area, state);
    }
    if state.composer_ui.path_completion.open {
        render_path_completion(frame, area, state);
    }
    if state.change_review.open {
        render_change_review(frame, area, state);
    }
    if state.change_walkthrough.is_some() {
        render_change_walkthrough(frame, area, state);
    }
    if state.terminal_manager.is_some() {
        render_terminal_manager(frame, area, state);
    }
    if state.theme_manager.is_some() {
        render_theme_manager(frame, area, state);
    }
    if state.copy_picker.is_some() {
        render_copy_picker(frame, area, state);
    }
    if state.notification_picker.is_some() {
        render_notification_picker(frame, area, state);
    }
    if state.command_output_picker.is_some() {
        render_command_output_picker(frame, area, state);
    }
    if state.output_full_confirmation.is_some() {
        render_output_full_confirmation(frame, area, state);
    }
    if state.scratch_delete_confirmation.is_some() {
        render_scratch_delete_confirmation(frame, area, state);
    }
    if state.route_preset_picker.is_some() {
        render_route_preset_picker(frame, area, state);
    }
    if state.reasoning_picker.is_some() {
        render_reasoning_picker(frame, area, state);
    }
    if state.route_preset_overwrite.is_some() {
        render_route_preset_overwrite(frame, area, state);
    }
    if state.statusline_manager.is_some() {
        render_statusline_manager(frame, area, state);
    }
    if state.jobs_view_open {
        render_jobs_view(frame, area, state);
    }
    if state.btw.is_some() {
        render_btw(frame, area, state);
    }
    if state.voice.is_some() {
        render_voice(frame, area, state);
    }
    if state.history_search.is_some() {
        render_history_search(frame, area, state);
    }
    if state.prompt_stash.is_some() {
        render_prompt_stash(frame, area, state);
    }
    if state.hook_manager.is_some() {
        render_hook_manager(frame, area, state);
    }
    if state.permission_manager.is_some() {
        render_permission_manager(frame, area, state);
    }
    if state.memory_manager.is_some() {
        render_memory_manager(frame, area, state);
    }
    if state.multi_run_manager.is_some() {
        render_multi_run_manager(frame, area, state);
    }
    if state.task_graph_manager.is_some() {
        render_task_graph_manager(frame, area, state);
    }
    if state.keymap_hub.is_some() {
        render_keymap_hub(frame, area, state);
    }
    if state.workflow_preview.is_some() {
        render_workflow_preview(frame, area, state);
    }
    if state.workflow_clear_confirmation.is_some() {
        render_workflow_clear_confirmation(frame, area, state);
    }
    if state.worktree_cleanup_confirmation.is_some() {
        render_worktree_cleanup_confirmation(frame, area, state);
    }
    if state.worktree_setup_confirmation.is_some() {
        render_worktree_setup_confirmation(frame, area, state);
    }
    if state.support_bundle_confirmation.is_some() {
        render_support_bundle_confirmation(frame, area, state);
    }
    if state.setup_import_confirmation.is_some() {
        render_setup_import_confirmation(frame, area, state);
    }
    if state.interaction.is_some() {
        render_interaction(frame, area, state);
    }
    if state.workspace_trust_prompt {
        render_workspace_trust_prompt(frame, area, state);
    }
    if let Some(confirmation) = &state.trace_delete_confirmation {
        render_trace_delete_confirmation(frame, area, confirmation);
    }
    if let Some(init) = &state.project_init {
        render_project_init(frame, area, init);
    }
    if state.loop_guard_interaction.is_some() {
        render_loop_guard_interaction(frame, area, state);
    }
    if transcript_activity_clicks_blocked(state) {
        state.transcript_scroll.clear_activity_hit_targets();
    }
    render_toast(frame, area, state);
    if state.debug_render_overlay {
        render_runtime_debug_overlay(frame, area, state);
    }
}

fn render_runtime_debug_overlay(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if area.width < 24 || area.height < 8 {
        return;
    }
    let snapshot = state.runtime_diagnostics.render_snapshot();
    let width = 36.min(area.width.saturating_sub(2));
    let height = 11.min(area.height.saturating_sub(1));
    let popup = Rect::new(
        area.right().saturating_sub(width + 1),
        area.y.saturating_add(1),
        width,
        height,
    );
    let theme = palette(state);
    let (builds, blocks, bytes, pending) = state.transcript_cache.debug_stats();
    let border = if snapshot.repaint_highlight {
        Color::Magenta
    } else {
        theme.accent
    };
    if snapshot.repaint_highlight {
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta)),
            area,
        );
    }
    let lines = vec![
        Line::from(format!("Redraws          {}", snapshot.redraw_count)),
        Line::from(format!("Dirty regions    {}", snapshot.dirty_regions)),
        Line::from(format!(
            "Last / max frame {} / {} ms",
            snapshot.last_frame_ms, snapshot.max_frame_ms
        )),
        Line::from(format!("Render failures  {}", snapshot.render_failures)),
        Line::from(format!("Repaint rate     {}/s", snapshot.repaint_rate)),
        Line::from(format!(
            "Highlight        {}",
            if snapshot.repaint_highlight {
                "on"
            } else {
                "off"
            }
        )),
        Line::from(format!("Cache layouts    {builds} · {blocks} blocks")),
        Line::from(format!(
            "Cached payload   {} · {}",
            bytes.map_or_else(|| "busy".into(), |bytes| format!("{} KiB", bytes / 1024)),
            if pending { "building" } else { "ready" }
        )),
    ];
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(theme.text).bg(theme.surface))
            .block(
                Block::default()
                    .title(" Render diagnostics ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border)),
            ),
        popup,
    );
}

fn render_toast(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if area.width < 4 || area.height < 3 {
        return;
    }
    let Some(toast) = state.toast.as_ref().filter(|toast| toast.is_visible()) else {
        return;
    };
    let theme = palette(state);
    let text = crate::terminal_text::sanitize_terminal_text(&toast.message);
    let text_width = unicode_width::UnicodeWidthStr::width(text.as_str()) as u16;
    let width = text_width
        .saturating_add(10)
        .min(area.width.saturating_sub(4))
        .max(18.min(area.width));
    let popup = centered_rect(width, 3.min(area.height), area);
    let color = match toast.kind {
        crate::app::ToastKind::Success => theme.accent,
        crate::app::ToastKind::Error => theme.error,
    };
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(""),
            Line::from(Span::styled(text, Style::default().fg(theme.text))),
        ]))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(color))
                .style(Style::default().bg(theme.surface)),
        ),
        popup,
    );
}

fn render_loop_guard_interaction(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(interaction) = state.loop_guard_interaction.as_ref() else {
        return;
    };
    let width = 72.min(area.width.saturating_sub(2));
    let height = if interaction.cancel_confirmation {
        13
    } else {
        15
    }
    .min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Low-yield loop paused ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let evidence = crate::terminal_text::sanitize_terminal_text(&interaction.incident.evidence);
    let mut lines = vec![
        Line::from(Span::styled(evidence, Style::default().fg(Color::Yellow))),
        Line::from(""),
    ];
    if interaction.cancel_confirmation {
        lines.push(Line::from(
            "Cancel the current turn? Completed tool changes remain.",
        ));
        lines.push(Line::from(""));
        let keep_style = if interaction.cancel_confirm_selected {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Black).bg(Color::White)
        };
        let cancel_style = if interaction.cancel_confirm_selected {
            Style::default().fg(Color::Black).bg(Color::Red)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::styled(" Keep paused ", keep_style),
            Span::raw("  "),
            Span::styled(" Cancel turn ", cancel_style),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "←/→ choose · Enter confirm · Esc back",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from("Choose how to continue:"));
        lines.push(Line::from(""));
        let choices = [
            (
                crate::loop_guard::LoopGuardDecision::Replan,
                " Replan ",
                "require a materially different next approach",
            ),
            (
                crate::loop_guard::LoopGuardDecision::Continue,
                " Continue ",
                "suppress this guard for the rest of the turn",
            ),
            (
                crate::loop_guard::LoopGuardDecision::Cancel,
                " Cancel ",
                "open a safe confirmation",
            ),
        ];
        for (decision, label, description) in choices {
            let style = if interaction.selected == decision {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![
                Span::styled(label, style),
                Span::raw(format!("  {description}")),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "←/→ choose · Enter select · r replan · o continue · c cancel",
            Style::default().fg(Color::DarkGray),
        )));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_project_init(
    frame: &mut ratatui::Frame,
    area: Rect,
    init: &crate::app::ProjectInitState,
) {
    use crate::app::ProjectInitPhase;
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Initialize project instructions ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let footer_height = if init.phase == ProjectInitPhase::Preview {
        2
    } else {
        1
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(footer_height)])
        .split(inner);
    let mut lines = vec![
        Line::from(format!("Target: {}", init.root.join("AGENTS.md").display())),
        Line::from(""),
    ];
    match init.phase {
        ProjectInitPhase::Preparing => {
            lines.push(Line::from(Span::styled(
                "Collecting bounded evidence and drafting with one tool-free request…",
                Style::default().fg(Color::Cyan),
            )));
            lines.push(Line::from(
                "Esc or Ctrl+C cancels without changing the workspace.",
            ));
        }
        ProjectInitPhase::Writing => {
            lines.push(Line::from(Span::styled(
                "Revalidating evidence and creating AGENTS.md safely…",
                Style::default().fg(Color::Cyan),
            )));
        }
        ProjectInitPhase::Preview => {
            lines.push(Line::from(Span::styled(
                "Preview — no file has been written",
                Style::default().fg(Color::Yellow),
            )));
            lines.push(Line::from(""));
            if let Some(draft) = &init.draft {
                lines.extend(
                    crate::terminal_text::sanitize_terminal_text(&draft.content)
                        .lines()
                        .map(|line| Line::from(line.to_string())),
                );
            }
        }
    }
    if let Some(error) = &init.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(error),
            Style::default().fg(Color::Red),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((init.scroll, 0))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );
    if init.phase == ProjectInitPhase::Preview {
        let cancel = if init.create_selected {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        };
        let create = if init.create_selected {
            Style::default().fg(Color::Black).bg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(" Cancel ", cancel),
                    Span::raw("  "),
                    Span::styled(" Create AGENTS.md ", create),
                ]),
                Line::from(Span::styled(
                    "←/→ choose · ↑↓/Pg scroll · Enter select · Esc cancel",
                    Style::default().fg(Color::DarkGray),
                )),
            ]),
            chunks[1],
        );
    } else {
        frame.render_widget(
            Paragraph::new(if init.phase == ProjectInitPhase::Preparing {
                "Esc cancel"
            } else {
                "Writing is completing"
            })
            .style(Style::default().fg(Color::DarkGray)),
            chunks[1],
        );
    }
}

fn render_trace_delete_confirmation(
    frame: &mut ratatui::Frame,
    area: Rect,
    confirmation: &crate::app::TraceDeleteConfirmation,
) {
    let width = 64.min(area.width.saturating_sub(4));
    let height =
        if confirmation.error.is_some() { 12 } else { 10 }.min(area.height.saturating_sub(4));
    let modal = centered_rect(width, height, area);
    let size = if confirmation.bytes >= 1024 * 1024 {
        format!("{:.1} MiB", confirmation.bytes as f64 / (1024.0 * 1024.0))
    } else if confirmation.bytes >= 1024 {
        format!("{:.1} KiB", confirmation.bytes as f64 / 1024.0)
    } else {
        format!("{} B", confirmation.bytes)
    };
    let mut lines = vec![
        Line::from(Span::styled(
            "Delete provider trace?",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!("{} · {size}", confirmation.trace_id)),
        Line::from("This permanently removes exactly this trace file."),
        Line::from("Provider requests, sessions, and other traces are unchanged."),
        Line::from(""),
    ];
    if let Some(error) = &confirmation.error {
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(error),
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(""));
    }
    let (cancel_style, delete_style) = if confirmation.delete_selected {
        (
            Style::default().fg(Color::White),
            Style::default().fg(Color::Black).bg(Color::Red),
        )
    } else {
        (
            Style::default().fg(Color::Black).bg(Color::Cyan),
            Style::default().fg(Color::White),
        )
    };
    lines.push(Line::from(vec![
        Span::styled(" Cancel ", cancel_style),
        Span::raw("   "),
        Span::styled(" Delete ", delete_style),
    ]));
    lines.push(Line::from(Span::styled(
        "←/→ choose · Enter confirm · Esc cancel",
        Style::default().fg(Color::DarkGray),
    )));

    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red)),
            )
            .wrap(Wrap { trim: false }),
        modal,
    );
}

fn render_multi_run_manager(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(manager) = state.multi_run_manager.as_ref() else {
        return;
    };
    let width = 100.min(area.width.saturating_sub(2));
    let height = 31.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Multi-run · isolated candidates ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let mut lines = Vec::new();
    if manager.editing {
        lines.extend([
            Line::from(Span::styled(
                "One request, independent worktrees, explicit judging",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(format!(
                "Candidates: {} (↑↓ change; maximum {})",
                manager.candidate_count,
                crate::multi_run::MAX_CANDIDATES
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("> ", Style::default().fg(Color::Cyan)),
                Span::raw(if manager.draft_request.is_empty() {
                    "describe the task…"
                } else {
                    manager.draft_request.as_str()
                }),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Enter preview · Esc close · no worktrees or provider calls yet",
                Style::default().fg(Color::DarkGray),
            )),
        ]);
    } else if let Some(run) = manager.run.as_ref() {
        lines.push(Line::from(vec![
            Span::styled(
                run.status.label(),
                Style::default().fg(match run.status {
                    crate::multi_run::MultiRunStatus::Failed => Color::Red,
                    crate::multi_run::MultiRunStatus::ReadyToJudge
                    | crate::multi_run::MultiRunStatus::Judged
                    | crate::multi_run::MultiRunStatus::Complete => Color::Green,
                    crate::multi_run::MultiRunStatus::Running
                    | crate::multi_run::MultiRunStatus::CreatingWorktrees
                    | crate::multi_run::MultiRunStatus::Judging
                    | crate::multi_run::MultiRunStatus::Fusing => Color::Yellow,
                    _ => Color::White,
                }),
            ),
            Span::raw(format!(
                " · {} candidates · cost estimate shown per route when configured",
                run.candidates.len()
            )),
        ]));
        lines.push(Line::from(Span::styled(
            "Candidate worktrees and branches remain user-owned; nothing is merged or attached.",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
        let available = inner.height.saturating_sub(12) as usize;
        let start = run.selected_idx.saturating_sub(available.saturating_sub(1));
        for (index, candidate) in run
            .candidates
            .iter()
            .enumerate()
            .skip(start)
            .take(available)
        {
            let selected = index == run.selected_idx;
            let worktree = candidate
                .worktree_id
                .map(|id| id.to_string()[..8].to_string())
                .unwrap_or_else(|| "planned".into());
            let evidence = candidate.evidence.as_ref().map_or_else(
                || "evidence pending".into(),
                |evidence| {
                    format!(
                        "{} changes · {} omitted",
                        evidence.changed_entries, evidence.omitted_bytes
                    )
                },
            );
            let line = Line::from(format!(
                "{} {} · {} · {} · {}",
                if selected { "▶" } else { " " },
                candidate.definition.id.label(),
                candidate.definition.route.label(),
                candidate.status.label(),
                candidate.estimated_cost
            ));
            lines.push(if selected {
                line.style(Style::default().bg(Color::Cyan).fg(Color::Black))
            } else {
                line
            });
            lines.push(Line::from(format!("  worktree {worktree} · {evidence}")));
        }
        if let Some(judgment) = &run.judgment {
            lines.push(Line::from(""));
            lines.push(Line::from(format!(
                "Judge selected {} · {}",
                judgment.winner.label(),
                crate::terminal_text::sanitize_terminal_text(&judgment.rationale)
            )));
        }
        if let Some(fusion) = &run.fusion {
            lines.push(Line::from(format!(
                "Fusion proposal: {} / worktree {} · {}",
                fusion.source.label(),
                &fusion.worktree_id.to_string()[..8],
                crate::terminal_text::sanitize_terminal_text(&fusion.plan)
            )));
        }
        lines.push(Line::from(""));
        let controls = match run.status {
            crate::multi_run::MultiRunStatus::Preview => {
                "Enter route · Shift+S start · Esc close · ↑↓ navigate"
            }
            crate::multi_run::MultiRunStatus::CreatingWorktrees
            | crate::multi_run::MultiRunStatus::Running => {
                "Shift+A/D approve/deny waiting candidate · Shift+C cancel · Esc close"
            }
            crate::multi_run::MultiRunStatus::ReadyToJudge => {
                "Shift+R refresh evidence · Shift+J judge · Esc close"
            }
            crate::multi_run::MultiRunStatus::Judged => {
                "Shift+R refresh · Shift+F fusion proposal · Esc close"
            }
            _ => "Shift+R refresh evidence · Esc close",
        };
        lines.push(Line::from(Span::styled(
            controls,
            Style::default().fg(Color::DarkGray),
        )));
    }
    if manager.busy {
        lines.push(Line::from(Span::styled(
            "Working… controls are locked",
            Style::default().fg(Color::Yellow),
        )));
    } else if let Some(error) = &manager.error {
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(error),
            Style::default().fg(Color::Red),
        )));
    } else if let Some(notice) = &manager.notice {
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(notice),
            Style::default().fg(Color::Green),
        )));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);

    if let Some(confirmation) = &manager.confirmation {
        let confirm_area = centered_rect(70.min(area.width.saturating_sub(4)), 10, area);
        frame.render_widget(Clear, confirm_area);
        let verb = match confirmation.action {
            crate::app::MultiRunConfirmationAction::Start => "Start candidates",
            crate::app::MultiRunConfirmationAction::Cancel => "Cancel live candidates",
            crate::app::MultiRunConfirmationAction::Judge => "Run read-only judge",
            crate::app::MultiRunConfirmationAction::Fuse => "Create fusion proposal",
        };
        let body = vec![
            Line::from(verb),
            Line::from("This may use provider tokens."),
            Line::from("Files are never merged automatically."),
            Line::from(""),
            Line::from(if confirmation.confirm_selected {
                "  Cancel     [ Confirm ]"
            } else {
                "[ Cancel ]     Confirm"
            }),
            Line::from("Arrow/Tab choose · Enter confirm · Esc cancel"),
        ];
        frame.render_widget(
            Paragraph::new(body).block(
                Block::default()
                    .title(" Confirm multi-run action ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            ),
            confirm_area,
        );
    }
}

fn render_workflow_preview(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(preview) = state.workflow_preview.as_ref() else {
        return;
    };
    let width = 92.min(area.width.saturating_sub(2));
    let height = 26.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let title = if preview.resume_existing {
        " Resume workflow "
    } else {
        " Run workflow "
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let mut lines = crate::workflow::render_preview(preview)
        .lines()
        .map(|line| Line::from(crate::terminal_text::sanitize_terminal_text(line)))
        .collect::<Vec<_>>();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            if preview.run_selected {
                "  Cancel  "
            } else {
                "▶ Cancel  "
            },
            if preview.run_selected {
                Style::default()
            } else {
                Style::default().bg(Color::Cyan).fg(Color::Black)
            },
        ),
        Span::raw("   "),
        Span::styled(
            if preview.run_selected {
                if preview.resume_existing {
                    "▶ Resume  "
                } else {
                    "▶ Run  "
                }
            } else if preview.resume_existing {
                "  Resume  "
            } else {
                "  Run  "
            },
            if preview.run_selected {
                Style::default().bg(Color::Cyan).fg(Color::Black)
            } else {
                Style::default()
            },
        ),
    ]));
    lines.push(Line::from(Span::styled(
        "Arrow/Tab choose · Enter confirm · Esc close",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_plan_workflow_graph(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(graph) = state.plan_workflow_graph.as_ref() else {
        return;
    };
    let theme = palette(state);
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Plan workflow ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.plan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(4),
        ])
        .split(inner);
    let work = state
        .work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let Some(workflow) = work.plan_workflow.as_ref() else {
        frame.render_widget(
            Paragraph::new(
                "No plan workflow yet.\nUse /plan run <request>, or create a plan and /plan send.",
            )
            .wrap(Wrap { trim: false }),
            chunks[0],
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(crate::terminal_text::sanitize_terminal_text(
                    state.status_message.as_deref().unwrap_or_default(),
                )),
                Line::from("S Send plan · E Execute plan"),
                Line::from("Esc close"),
            ])
            .style(Style::default().fg(theme.plan)),
            chunks[2],
        );
        return;
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                crate::terminal_text::sanitize_terminal_text(&workflow.goal),
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(format!(
                "{:?} · {} stages · {} agents · {}",
                workflow.status,
                workflow.steps.len(),
                workflow.participants.len(),
                if graph.participants {
                    "Agents"
                } else {
                    "Handoffs"
                }
            )),
        ])
        .wrap(Wrap { trim: false }),
        chunks[0],
    );
    let mut lines = Vec::new();
    let count = if graph.participants {
        workflow.participants.len()
    } else {
        workflow.steps.len()
    };
    let selected = graph.selected_idx.min(count.saturating_sub(1));
    let visible = usize::from(chunks[1].height / 4).max(1);
    let start = selected
        .saturating_sub(visible / 2)
        .min(count.saturating_sub(visible));
    let width = usize::from(chunks[1].width.saturating_sub(4));
    if graph.participants {
        for (index, participant) in workflow
            .participants
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
        {
            let prefix = if index == selected { "▶" } else { " " };
            let style = if index == selected {
                Style::default().bg(theme.plan).fg(theme.accent_foreground)
            } else {
                Style::default().fg(theme.text)
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "{prefix} {}",
                    crate::terminal_text::sanitize_terminal_text(&participant.id)
                ),
                style,
            )));
            lines.push(Line::from(format!(
                "  {}",
                crate::tui::selection::truncate_cells(
                    &crate::terminal_text::sanitize_terminal_text(&format!(
                        "{}/{}",
                        participant.route.provider, participant.route.model
                    )),
                    width
                )
            )));
            lines.push(Line::from("  Enter drafts a handoff to this agent"));
            lines.push(Line::from(""));
        }
    } else {
        for (index, stage) in workflow.steps.iter().enumerate().skip(start).take(visible) {
            let prefix = if index == selected { "▶" } else { " " };
            let style = if index == selected {
                Style::default().bg(theme.plan).fg(theme.accent_foreground)
            } else {
                Style::default().fg(theme.text)
            };
            let label = format!(
                "{prefix} {}. {:?} · {} · {:?}",
                index + 1,
                stage.role,
                stage.participant,
                stage.status
            );
            lines.push(Line::from(Span::styled(
                crate::tui::selection::truncate_cells(
                    &crate::terminal_text::sanitize_terminal_text(&label),
                    usize::from(chunks[1].width),
                ),
                style,
            )));
            lines.push(Line::from(format!(
                "  {}",
                crate::tui::selection::truncate_cells(
                    &crate::terminal_text::sanitize_terminal_text(&format!(
                        "{}/{}",
                        stage.route.provider, stage.route.model
                    )),
                    width
                )
            )));
            let result = stage
                .result_snapshot
                .and_then(|snapshot| workflow.snapshots.get(snapshot))
                .map(|snapshot| format!("Plan revision {}", snapshot.plan.revision))
                .or_else(|| {
                    stage.accepted.map(|accepted| {
                        if accepted {
                            "Accepted".into()
                        } else {
                            "Changes requested".into()
                        }
                    })
                })
                .unwrap_or_else(|| "Enter opens plan and feedback".into());
            lines.push(Line::from(format!("  {result}")));
            lines.push(Line::from(if index + 1 < count { "  ↓" } else { "" }));
        }
    }
    frame.render_widget(Paragraph::new(lines), chunks[1]);
    let notice = state
        .status_message
        .as_deref()
        .or(workflow.notice.as_deref())
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(crate::tui::selection::truncate_cells(
                &crate::terminal_text::sanitize_terminal_text(notice),
                usize::from(chunks[2].width),
            )),
            Line::from(if graph.participants {
                "↑↓ select · Enter draft handoff · Tab handoffs"
            } else {
                "↑↓ select · Enter inspect · Tab agents/handoffs"
            }),
            Line::from("S Send · E Execute · P Pause/Resume · C Cancel"),
            Line::from(format!(
                "Esc close · {}/{}",
                if count == 0 { 0 } else { selected + 1 },
                count
            )),
        ])
        .style(Style::default().fg(theme.plan)),
        chunks[2],
    );
}

fn render_task_graph_manager(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(manager) = state.task_graph_manager.as_ref() else {
        return;
    };
    let width = 96.min(area.width.saturating_sub(2));
    let height = 30.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Tasks and agent graph ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let work = state
        .work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone();
    let mut lines = Vec::new();
    let Some(plan) = work.plan.as_ref() else {
        lines.push(Line::from("No plan tasks. Use /plan to propose one."));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Esc close",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
        return;
    };
    lines.push(Line::from(vec![
        Span::styled(
            crate::terminal_text::sanitize_terminal_text(&plan.title),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            " · plan {:?} · graph {:?} · revision {}",
            plan.status, work.agent_graph.status, plan.revision
        )),
    ]));
    if state.agent_graph_busy {
        lines.push(Line::from(Span::styled(
            "Updating agent graph… controls are temporarily locked",
            Style::default().fg(Color::Yellow),
        )));
    }
    lines.push(Line::from(format!("> {}", manager.filter)));
    lines.push(Line::from(""));
    let filter = manager.filter.trim().to_ascii_lowercase();
    let visible = plan
        .tasks
        .iter()
        .filter(|task| {
            filter.is_empty()
                || task.title.to_ascii_lowercase().contains(&filter)
                || task.id.to_string().contains(&filter)
        })
        .collect::<Vec<_>>();
    if visible.is_empty() {
        lines.push(Line::from("No matching tasks"));
    } else {
        let selected = manager.selected_idx.min(visible.len().saturating_sub(1));
        let start = selected
            .saturating_sub(4)
            .min(visible.len().saturating_sub(8));
        for (index, task) in visible.iter().enumerate().skip(start).take(8) {
            let selected_row = index == selected;
            let prefix = if selected_row { "▶" } else { " " };
            let graph_state = task
                .agent
                .as_ref()
                .map(|_| format!("{:?}", task.agent_runtime.state))
                .unwrap_or_else(|| format!("{:?}", task.status));
            let text = format!(
                "{prefix} {:<18} {:<46} attempt {}/{}",
                graph_state,
                crate::terminal_text::sanitize_terminal_text(&task.title),
                task.agent_runtime.attempts.len(),
                task.agent.as_ref().map_or(0, |agent| agent.max_attempts)
            );
            lines.push(Line::from(Span::styled(
                text,
                if selected_row {
                    Style::default().bg(Color::Cyan).fg(Color::Black)
                } else {
                    Style::default()
                },
            )));
        }
        let task = visible[selected];
        lines.push(Line::from(""));
        lines.push(Line::from(format!("Task ID: {}", task.id)));
        if let Some(agent) = &task.agent {
            let dependencies = if agent.depends_on.is_empty() {
                "none".to_string()
            } else {
                agent
                    .depends_on
                    .iter()
                    .map(|id| id.to_string().chars().take(8).collect::<String>())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let worktree = agent
                .worktree_id
                .map(|id| id.to_string().chars().take(8).collect::<String>())
                .unwrap_or_else(|| "current workspace".into());
            lines.push(Line::from(format!(
                "Dependencies: {dependencies} · Worktree: {worktree}"
            )));
            lines.push(Line::from(format!(
                "Grants: tools [{}] · {} path(s) · {} command(s) · network {}",
                agent.grant.tools.join(", "),
                agent.grant.paths.len(),
                agent.grant.commands.len(),
                if agent.grant.network {
                    "requested"
                } else {
                    "denied"
                }
            )));
            let evidence = agent
                .evidence
                .iter()
                .map(|gate| match gate {
                    crate::agent_graph::AgentEvidenceGate::AssistantOutput => {
                        "assistant output".into()
                    }
                    crate::agent_graph::AgentEvidenceGate::ToolSuccess { tool } => {
                        format!("{tool} success")
                    }
                    crate::agent_graph::AgentEvidenceGate::Artifact(predicate) => {
                        format!("artifact {}", predicate.path)
                    }
                    crate::agent_graph::AgentEvidenceGate::ClosedCommand(_) => {
                        "closed command".into()
                    }
                    crate::agent_graph::AgentEvidenceGate::HumanReview => "human review".into(),
                })
                .collect::<Vec<String>>()
                .join(", ");
            lines.push(Line::from(format!("Evidence: {evidence}")));
            if let Some(receipt) = task
                .agent_runtime
                .attempts
                .last()
                .and_then(|attempt| attempt.verification_receipt.as_ref())
            {
                let attempt = task
                    .agent_runtime
                    .attempts
                    .last()
                    .expect("receipt came from the last attempt");
                let receipt_valid = if receipt.disposition
                    == crate::agent_graph::AgentVerificationDisposition::Pass
                {
                    crate::agent_graph::task_completion_verified(
                        work.agent_graph.verification_binding(),
                        task,
                    )
                } else {
                    crate::agent_graph::verification_receipt_valid(
                        work.agent_graph.verification_binding(),
                        task.id,
                        agent,
                        attempt,
                    )
                };
                if !receipt_valid {
                    lines.push(Line::from(Span::styled(
                        "Verification: stale/invalid receipt",
                        Style::default().fg(Color::Yellow),
                    )));
                } else {
                    let color = match receipt.disposition {
                        crate::agent_graph::AgentVerificationDisposition::Pass => Color::Green,
                        crate::agent_graph::AgentVerificationDisposition::Fail => Color::Red,
                        crate::agent_graph::AgentVerificationDisposition::Inconclusive => {
                            Color::Yellow
                        }
                    };
                    lines.push(Line::from(Span::styled(
                        format!(
                            "Verification: {:?} · receipt {}",
                            receipt.disposition,
                            receipt.receipt_sha256.get(..12).unwrap_or("invalid")
                        ),
                        Style::default().fg(color),
                    )));
                }
            } else if task.agent_runtime.state == crate::agent_graph::AgentTaskState::Completed
                && agent.verification_contract.is_none()
            {
                lines.push(Line::from(Span::styled(
                    "Verification: legacy-unverified",
                    Style::default().fg(Color::Yellow),
                )));
            }
        } else {
            lines.push(Line::from(format!(
                "Ordinary task · status {:?} · owner {}",
                task.status,
                task.owner.as_deref().unwrap_or("unassigned")
            )));
            lines.push(Line::from(
                "Configure dependency and grant metadata in plan mode to run it as an agent graph.",
            ));
        }
        if !task.evidence.is_empty() {
            lines.push(Line::from(format!(
                "Recorded evidence: {}",
                task.evidence.len()
            )));
            for evidence in task.evidence.iter().take(2) {
                lines.push(Line::from(format!(
                    "- {}: {}",
                    evidence.event_id,
                    crate::terminal_text::sanitize_terminal_text(&evidence.note)
                )));
            }
        }
        if let Some(failure) = &task.agent_runtime.failure {
            lines.push(Line::from(Span::styled(
                format!(
                    "Failure: {}",
                    crate::terminal_text::sanitize_terminal_text(failure)
                ),
                Style::default().fg(Color::Red),
            )));
        }
    }
    if let Some(error) = &manager.error {
        lines.push(Line::from(Span::styled(
            format!(
                "Error: {}",
                crate::terminal_text::sanitize_terminal_text(error)
            ),
            Style::default().fg(Color::Red),
        )));
    } else if let Some(notice) = &manager.notice {
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(notice),
            Style::default().fg(Color::Green),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Shift+S start/resume · Shift+P pause · Shift+R retry · Shift+C cancel · Esc close",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);

    if let Some(confirmation) = &manager.confirmation {
        let confirm_area = centered_rect(
            72.min(area.width.saturating_sub(4)),
            10.min(area.height.saturating_sub(4)),
            area,
        );
        frame.render_widget(Clear, confirm_area);
        let title = match confirmation.action {
            crate::app::TaskGraphConfirmationAction::Start => " Start agent graph ",
            crate::app::TaskGraphConfirmationAction::Cancel => " Cancel agent graph ",
            crate::app::TaskGraphConfirmationAction::Retry(_) => " Retry agent task ",
            crate::app::TaskGraphConfirmationAction::Review(_) => " Record human review ",
        };
        let text = match confirmation.action {
            crate::app::TaskGraphConfirmationAction::Start => {
                "Start dependency-ready child agents? Existing approval, sandbox, tool, and workspace policy still apply."
            }
            crate::app::TaskGraphConfirmationAction::Cancel => {
                "Cancel graph scheduling? Child changes are not rolled back and worktrees/branches are preserved."
            }
            crate::app::TaskGraphConfirmationAction::Retry(_) => {
                "Approve one new attempt? Failed attempts remain recorded and descendants stay blocked until this task succeeds."
            }
            crate::app::TaskGraphConfirmationAction::Review(_) => {
                "Record an explicit human pass for this task's human-review gate? Other artifact and command gates must already pass."
            }
        };
        let cancel_style = if confirmation.confirm_selected {
            Style::default()
        } else {
            Style::default().bg(Color::Cyan).fg(Color::Black)
        };
        let confirm_style = if confirmation.confirm_selected {
            Style::default().bg(Color::Cyan).fg(Color::Black)
        } else {
            Style::default()
        };
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        let inner = block.inner(confirm_area);
        frame.render_widget(block, confirm_area);
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(text),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        if confirmation.confirm_selected {
                            "  Cancel  "
                        } else {
                            "▶ Cancel  "
                        },
                        cancel_style,
                    ),
                    Span::raw("   "),
                    Span::styled(
                        if confirmation.confirm_selected {
                            "▶ Confirm  "
                        } else {
                            "  Confirm  "
                        },
                        confirm_style,
                    ),
                ]),
                Line::from(Span::styled(
                    "Arrow/Tab choose · Enter confirm · Esc close",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .wrap(Wrap { trim: false }),
            inner,
        );
    }
}

fn render_workflow_clear_confirmation(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(confirm_selected) = state.workflow_clear_confirmation else {
        return;
    };
    let popup = centered_rect(62.min(area.width.saturating_sub(2)), 10, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Clear workflow state? ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let workflow = state
        .workflow_run
        .as_ref()
        .map(|run| run.definition_id.as_str())
        .unwrap_or("workflow");
    let lines = vec![
        Line::from(format!(
            "Clear {workflow} state? Definition and transcript remain unchanged."
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                if confirm_selected {
                    "  Cancel  "
                } else {
                    "▶ Cancel  "
                },
                if confirm_selected {
                    Style::default()
                } else {
                    Style::default().bg(Color::Cyan).fg(Color::Black)
                },
            ),
            Span::raw("   "),
            Span::styled(
                if confirm_selected {
                    "▶ Clear  "
                } else {
                    "  Clear  "
                },
                if confirm_selected {
                    Style::default().bg(Color::Red).fg(Color::White)
                } else {
                    Style::default()
                },
            ),
        ]),
        Line::from(Span::styled(
            "Arrow/Tab choose · Enter confirm · Esc close",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_worktree_cleanup_confirmation(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(confirmation) = state.worktree_cleanup_confirmation.as_ref() else {
        return;
    };
    let popup = centered_rect(
        70.min(area.width.saturating_sub(2)),
        if confirmation.error.is_some() { 14 } else { 12 },
        area,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Clean up worktree? ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let mut lines = vec![
        Line::from(format!(
            "Remove the clean worktree directory for {}?",
            confirmation.label
        )),
        Line::from(format!("Branch {} will be preserved.", confirmation.branch)),
        Line::from("No merge, commit, stash, or branch deletion will run."),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                if confirmation.remove_selected {
                    "  Cancel  "
                } else {
                    "▶ Cancel  "
                },
                if confirmation.remove_selected {
                    Style::default()
                } else {
                    Style::default().bg(Color::Cyan).fg(Color::Black)
                },
            ),
            Span::raw("   "),
            Span::styled(
                if confirmation.busy {
                    "  Removing…  "
                } else if confirmation.remove_selected {
                    "▶ Remove  "
                } else {
                    "  Remove  "
                },
                if confirmation.remove_selected {
                    Style::default().bg(Color::Red).fg(Color::White)
                } else {
                    Style::default()
                },
            ),
        ]),
        Line::from(Span::styled(
            if confirmation.busy {
                "Cleanup is running; controls are locked"
            } else {
                "Arrow/Tab choose · Enter confirm · Esc close"
            },
            Style::default().fg(Color::DarkGray),
        )),
    ];
    if let Some(error) = &confirmation.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(error),
            Style::default().fg(Color::Red),
        )));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_worktree_setup_confirmation(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(confirmation) = state.worktree_setup_confirmation.as_ref() else {
        return;
    };
    let step_lines = confirmation.preview.steps.len().saturating_mul(2);
    let popup_height = (16usize + step_lines + usize::from(confirmation.error.is_some()))
        .min(area.height.saturating_sub(2) as usize) as u16;
    let popup = centered_rect(
        86.min(area.width.saturating_sub(2)),
        popup_height.max(12),
        area,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Worktree setup preview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let preview = &confirmation.preview;
    let mut lines = vec![
        Line::from(format!(
            "Worktree: {} · base {}",
            preview.label, preview.base_ref
        )),
        Line::from(format!(
            "Source: {} · SHA-256 {}",
            preview.source_kind.label(),
            &preview.source_sha256[..12]
        )),
        Line::from(format!("Approval digest: {}", preview.digest)),
        Line::from("Setup is local, bounded, and is not run until you choose Run."),
        Line::from(""),
    ];
    for (index, step) in preview.steps.iter().enumerate() {
        lines.push(Line::from(format!(
            "{}. {} · cwd={} · sandbox={} · network={}",
            index + 1,
            step.name,
            step.cwd.display(),
            step.sandbox,
            if step.network { "allowed" } else { "denied" }
        )));
        lines.push(Line::from(format!(
            "   {} {}",
            step.program,
            step.args.join(" ")
        )));
    }
    if let Some(error) = confirmation.error.as_deref() {
        lines.push(Line::from(Span::styled(
            format!("Setup failed; worktree retained: {error}"),
            Style::default().fg(Color::Red),
        )));
    }
    lines.extend([
        Line::from(""),
        Line::from(vec![
            Span::styled(
                if confirmation.run_selected {
                    "  Skip  "
                } else {
                    "▶ Skip  "
                },
                if confirmation.run_selected {
                    Style::default()
                } else {
                    Style::default().bg(Color::Cyan).fg(Color::Black)
                },
            ),
            Span::raw("   "),
            Span::styled(
                if confirmation.busy {
                    "  Running…  "
                } else if confirmation.run_selected {
                    "▶ Run  "
                } else {
                    "  Run  "
                },
                if confirmation.run_selected {
                    Style::default().bg(Color::Yellow).fg(Color::Black)
                } else {
                    Style::default()
                },
            ),
        ]),
        Line::from(Span::styled(
            "Arrow/Tab choose · Enter confirm · Esc skip",
            Style::default().fg(Color::DarkGray),
        )),
    ]);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_support_bundle_confirmation(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(confirmation) = state.support_bundle_confirmation.as_ref() else {
        return;
    };
    let preview = confirmation.plan.preview();
    let visible_entries = preview.entries.len().min(8);
    let popup_height = (14 + visible_entries + usize::from(confirmation.error.is_some()))
        .min(area.height.saturating_sub(2) as usize) as u16;
    let popup = centered_rect(
        86.min(area.width.saturating_sub(2)),
        popup_height.max(12),
        area,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Support bundle preview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let mut lines = vec![
        Line::from(format!(
            "Profile {} · {} entries · {} bytes uncompressed",
            preview.profile,
            preview.entries.len(),
            preview.total_uncompressed_bytes
        )),
        Line::from("Contains typed doctor/audit/health/provenance metadata and safe runtime logs."),
        Line::from("Excludes prompts, transcripts, tool content, provider payloads, raw paths, URLs, environment values, and credentials."),
        Line::from(""),
    ];
    for entry in preview.entries.iter().take(visible_entries) {
        lines.push(Line::from(format!(
            "{} · {} bytes · {}",
            entry.name,
            entry.bytes,
            &entry.sha256[..12]
        )));
    }
    if preview.entries.len() > visible_entries {
        lines.push(Line::from(format!(
            "… {} more entries",
            preview.entries.len() - visible_entries
        )));
    }
    if let Some(error) = confirmation.error.as_deref() {
        lines.push(Line::from(Span::styled(
            format!("Creation failed: {error}"),
            Style::default().fg(Color::Red),
        )));
    }
    lines.extend([
        Line::from(""),
        Line::from(vec![
            Span::styled(
                if confirmation.create_selected {
                    "  Cancel  "
                } else {
                    "▶ Cancel  "
                },
                if confirmation.create_selected {
                    Style::default()
                } else {
                    Style::default().bg(Color::Cyan).fg(Color::Black)
                },
            ),
            Span::raw("   "),
            Span::styled(
                if confirmation.busy {
                    "  Creating…  "
                } else if confirmation.create_selected {
                    "▶ Create  "
                } else {
                    "  Create  "
                },
                if confirmation.create_selected {
                    Style::default().bg(Color::Yellow).fg(Color::Black)
                } else {
                    Style::default()
                },
            ),
        ]),
        Line::from(Span::styled(
            "Arrow/Tab choose · Enter confirm · Esc cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ]);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_setup_import_confirmation(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(confirmation) = state.setup_import_confirmation.as_ref() else {
        return;
    };
    let preview = &confirmation.preview;
    let visible_files = preview.files.len().min(6);
    let popup_height = (15
        + visible_files
        + usize::from(!preview.skipped.is_empty())
        + usize::from(!preview.collisions.is_empty())
        + usize::from(confirmation.error.is_some()))
    .min(area.height.saturating_sub(2) as usize) as u16;
    let popup = centered_rect(
        90.min(area.width.saturating_sub(2)),
        popup_height.max(13),
        area,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Claude setup import preview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let mut lines = vec![
        Line::from(format!(
            "{} scope · {} files · {} input bytes",
            preview.scope.label(),
            preview.files.len(),
            preview.total_source_bytes
        )),
        Line::from(format!("Source: {}", preview.source_root)),
        Line::from(format!("Target: {}", preview.target_root)),
        Line::from(format!("Digest: {}", preview.digest)),
        Line::from("Imports inert instructions, conditional rules, prompt commands, and valid skills only."),
        Line::from("Settings, credentials, hooks, MCP, permissions, shell, and account data remain excluded."),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                if confirmation.apply_selected {
                    "  Cancel  "
                } else {
                    "▶ Cancel  "
                },
                if confirmation.apply_selected {
                    Style::default()
                } else {
                    Style::default().bg(Color::Yellow).fg(Color::Black)
                },
            ),
            Span::raw("  "),
            Span::styled(
                if confirmation.apply_selected {
                    "▶ Apply exact preview  "
                } else {
                    "  Apply exact preview  "
                },
                if confirmation.apply_selected && preview.collisions.is_empty() {
                    Style::default().bg(Color::Yellow).fg(Color::Black)
                } else if confirmation.apply_selected {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default()
                },
            ),
        ]),
        Line::from(Span::styled(
            "Left/Right/Tab choose · Up/Down details · Enter confirm · Esc cancel",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
    ];
    for file in preview
        .files
        .iter()
        .skip(confirmation.scroll)
        .take(visible_files)
    {
        lines.push(Line::from(format!(
            "{} · {} · {}",
            file.kind,
            file.target,
            &file.target_sha256[..12]
        )));
    }
    if preview.files.len().saturating_sub(confirmation.scroll) > visible_files {
        lines.push(Line::from(format!(
            "… {} more files",
            preview.files.len() - confirmation.scroll - visible_files
        )));
    }
    if !preview.skipped.is_empty() {
        lines.push(Line::from(format!(
            "{} excluded file(s) listed in the preview",
            preview.skipped.len()
        )));
    }
    if !preview.collisions.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("Blocked: {} target collision(s)", preview.collisions.len()),
            Style::default().fg(Color::Red),
        )));
    }
    if let Some(error) = confirmation.error.as_deref() {
        lines.push(Line::from(Span::styled(
            format!("Apply failed: {error}"),
            Style::default().fg(Color::Red),
        )));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_hook_manager(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(manager) = state.hook_manager.as_ref() else {
        return;
    };
    let width = 88.min(area.width.saturating_sub(2));
    let height = 24.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Hooks ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let filter = manager.filter.trim().to_ascii_lowercase();
    let visible = manager
        .entries
        .iter()
        .filter(|entry| {
            filter.is_empty()
                || entry.event.as_str().contains(&filter)
                || (entry.enabled && "enabled".contains(&filter))
                || (!entry.enabled && "disabled".contains(&filter))
        })
        .collect::<Vec<_>>();
    let mut lines = vec![
        Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(if manager.filter.is_empty() {
                "type to filter…"
            } else {
                manager.filter.as_str()
            }),
        ]),
        Line::from(format!(
            "{} hooks · {}",
            manager.entries.len(),
            manager.network_enforcement
        )),
        Line::from(format!(
            "Config: {}",
            crate::terminal_text::sanitize_terminal_text(&manager.config_path)
        )),
        Line::from(""),
    ];
    if manager.busy {
        lines.push(Line::from(Span::styled(
            if manager.testing {
                "Testing hook… Esc or Ctrl+C cancels"
            } else {
                "Updating hooks…"
            },
            Style::default().fg(Color::Yellow),
        )));
    } else if visible.is_empty() {
        lines.push(Line::from(Span::styled(
            "No matching hooks",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let available_rows = inner.height.saturating_sub(10) as usize;
        let start = manager
            .selected_idx
            .saturating_sub(available_rows.saturating_sub(1));
        for (visible_index, entry) in visible.iter().enumerate().skip(start).take(available_rows) {
            let selected = visible_index == manager.selected_idx;
            let result = entry.last_result.as_ref().map_or_else(
                || "not tested".into(),
                |result| format!("{} · {} ms", result.status.label(), result.duration_ms),
            );
            let text = format!(
                "{} {:<24} {:<8} {:>3}s  {:<11} {}",
                if selected { "▶" } else { " " },
                entry.event.as_str(),
                if entry.enabled { "enabled" } else { "disabled" },
                entry.timeout_secs,
                if entry.fail_closed {
                    "fail-closed"
                } else {
                    "fail-open"
                },
                result,
            );
            lines.push(Line::from(Span::styled(
                text,
                if selected {
                    Style::default().bg(Color::Cyan).fg(Color::Black)
                } else if entry.enabled {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            )));
        }
    }
    if let Some(notice) = &manager.notice {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(notice),
            Style::default().fg(Color::Green),
        )));
    }
    if let Some(error) = &manager.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(error),
            Style::default().fg(Color::Red),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Space toggle · ⇧T test · ⇧R reload · Esc close",
        Style::default().fg(Color::DarkGray),
    )));
    if !manager.writes_enabled {
        lines.push(Line::from(Span::styled(
            "Bare profile: view only; commands stay in config.toml",
            Style::default().fg(Color::Yellow),
        )));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_permission_manager(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(manager) = state.permission_manager.as_ref() else {
        return;
    };
    let width = 88.min(area.width.saturating_sub(2));
    let height = 22.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Permissions ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let filter = manager.filter.trim().to_ascii_lowercase();
    let visible = manager
        .entries
        .iter()
        .filter(|entry| {
            filter.is_empty()
                || format!("{:?} {}", entry.decision, entry.matcher)
                    .to_ascii_lowercase()
                    .contains(&filter)
        })
        .collect::<Vec<_>>();
    let mut lines = vec![
        Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(if manager.filter.is_empty() {
                "type to filter…"
            } else {
                manager.filter.as_str()
            }),
        ]),
        Line::from(format!(
            "{} rules · persistent changes {}",
            manager.entries.len(),
            if manager.writes_enabled {
                "enabled"
            } else {
                "disabled"
            }
        )),
        Line::from(""),
    ];
    if visible.is_empty() {
        lines.push(Line::from(Span::styled(
            "No matching permission rules",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let narrow_rows = popup.width < 70;
        let available_rows = inner.height.saturating_sub(8) as usize;
        let row_height = if narrow_rows { 2 } else { 1 };
        let capacity = (available_rows / row_height).max(1);
        let range = crate::tui::selection::ListViewport::default().visible(
            manager.selected_idx,
            visible.len(),
            capacity,
        );
        for (visible_index, entry) in visible
            .iter()
            .enumerate()
            .skip(range.start)
            .take(range.len())
        {
            let selected = visible_index == manager.selected_idx;
            let source = match entry.source {
                crate::app::PermissionEntrySource::Legacy(_) => "legacy",
                crate::app::PermissionEntrySource::Rich(_) => "config",
                crate::app::PermissionEntrySource::Session(_) => "session",
            };
            let decision = format!("{:?}", entry.decision).to_ascii_lowercase();
            let label = format!(
                "{} {:<7} {:<7}",
                if selected { "▶" } else { " " },
                decision,
                source,
            );
            let row = crate::tui::selection::layout_row(
                &label,
                &entry.matcher,
                inner.width as usize,
                narrow_rows,
            );
            let style = if selected {
                Style::default().bg(Color::Cyan).fg(Color::Black)
            } else {
                Style::default()
            };
            if row.stacked {
                lines.push(Line::from(row.label).style(style));
                lines.push(Line::from(row.description).style(style));
            } else {
                lines.push(
                    Line::from(format!("{}{}{}", row.label, row.gap, row.description)).style(style),
                );
            }
        }
    }
    lines.push(Line::from(""));
    if manager.busy {
        lines.push(Line::from(Span::styled(
            "Removing permission rule…",
            Style::default().fg(Color::Yellow),
        )));
    } else if let Some(error) = &manager.error {
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(error),
            Style::default().fg(Color::Red),
        )));
    } else if let Some(notice) = &manager.notice {
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(notice),
            Style::default().fg(Color::Green),
        )));
    }
    lines.push(Line::from("Shift+D remove · Esc close · ↑↓ navigate"));
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);

    if let Some(confirmation) = &manager.confirmation {
        let confirm = centered_rect(
            72.min(area.width.saturating_sub(4)),
            if manager.error.is_some() { 11 } else { 9 },
            area,
        );
        frame.render_widget(Clear, confirm);
        let block = Block::default()
            .title(" Remove permission rule? ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));
        let mut body = vec![Line::from(crate::terminal_text::sanitize_terminal_text(
            &confirmation.label,
        ))];
        if let Some(error) = &manager.error {
            body.push(Line::from(Span::styled(
                crate::terminal_text::sanitize_terminal_text(error),
                Style::default().fg(Color::Red),
            )));
        }
        body.extend([
            Line::from(""),
            Line::from(if confirmation.delete_selected {
                "  Cancel     [ Remove ]"
            } else {
                "[ Cancel ]     Remove"
            }),
            Line::from("Enter confirm · Esc cancel · ←→ choose"),
        ]);
        frame.render_widget(
            Paragraph::new(body).block(block).wrap(Wrap { trim: false }),
            confirm,
        );
    }
}

fn render_memory_manager(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(manager) = state.memory_manager.as_ref() else {
        return;
    };
    let width = 94.min(area.width.saturating_sub(2));
    let height = 25.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Memory ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let view_label = match manager.view {
        crate::app::MemoryManagerView::User => "User",
        crate::app::MemoryManagerView::Workspace => "Workspace",
        crate::app::MemoryManagerView::Proposals => "Proposals",
    };
    let (recall, generation) = match manager.view {
        crate::app::MemoryManagerView::User => (manager.user_recall, manager.user_generation),
        crate::app::MemoryManagerView::Workspace => {
            (manager.workspace_recall, manager.workspace_generation)
        }
        crate::app::MemoryManagerView::Proposals => (false, false),
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled("[", Style::default().fg(Color::DarkGray)),
            Span::styled(view_label, Style::default().fg(Color::Cyan)),
            Span::styled("]", Style::default().fg(Color::DarkGray)),
            Span::raw("  Tab switch scope"),
        ]),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(if manager.filter.is_empty() {
                "type to filter…"
            } else {
                manager.filter.as_str()
            }),
        ]),
    ];
    if manager.view != crate::app::MemoryManagerView::Proposals {
        lines.push(Line::from(format!(
            "Recall {} · proposals {} · writes {}",
            if recall { "on" } else { "off" },
            if generation { "on" } else { "off" },
            if manager.writes_enabled {
                "enabled"
            } else {
                "disabled"
            }
        )));
    } else {
        lines.push(Line::from(format!(
            "{} pending · never persisted before acceptance",
            state.memory_proposals.len()
        )));
    }
    lines.push(Line::from(""));

    let filter = manager.filter.trim().to_ascii_lowercase();
    let available = inner.height.saturating_sub(10) as usize;
    if manager.view == crate::app::MemoryManagerView::Proposals {
        let visible = state
            .memory_proposals
            .iter()
            .filter(|proposal| {
                filter.is_empty()
                    || format!("{:?} {}", proposal.kind, proposal.text)
                        .to_ascii_lowercase()
                        .contains(&filter)
            })
            .collect::<Vec<_>>();
        if visible.is_empty() {
            lines.push(Line::from(Span::styled(
                "No pending proposals",
                Style::default().fg(Color::DarkGray),
            )));
        }
        for (index, proposal) in visible.iter().enumerate().take(available) {
            let text = crate::terminal_text::sanitize_terminal_text(&proposal.text)
                .replace(['\n', '\r'], " ");
            let line = Line::from(format!(
                "{} {:<22} {}",
                if index == manager.selected_idx {
                    "▶"
                } else {
                    " "
                },
                proposal.kind.label(),
                text
            ));
            lines.push(if index == manager.selected_idx {
                line.style(Style::default().bg(Color::Cyan).fg(Color::Black))
            } else {
                line
            });
        }
    } else {
        let records = match manager.view {
            crate::app::MemoryManagerView::User => &state.user_memory_records,
            crate::app::MemoryManagerView::Workspace => &state.workspace_memory_records,
            crate::app::MemoryManagerView::Proposals => unreachable!(),
        };
        let visible = records
            .iter()
            .filter(|record| {
                filter.is_empty()
                    || format!("{:?} {}", record.kind, record.text)
                        .to_ascii_lowercase()
                        .contains(&filter)
            })
            .collect::<Vec<_>>();
        if visible.is_empty() {
            lines.push(Line::from(Span::styled(
                "No matching memory entries",
                Style::default().fg(Color::DarkGray),
            )));
        }
        let start = manager
            .selected_idx
            .saturating_sub(available.saturating_sub(1));
        for (index, record) in visible.iter().enumerate().skip(start).take(available) {
            let text = crate::terminal_text::sanitize_terminal_text(&record.text)
                .replace(['\n', '\r'], " ");
            let line = Line::from(format!(
                "{} {:<22} {}",
                if index == manager.selected_idx {
                    "▶"
                } else {
                    " "
                },
                record.kind.label(),
                text
            ));
            lines.push(if index == manager.selected_idx {
                line.style(Style::default().bg(Color::Cyan).fg(Color::Black))
            } else {
                line
            });
        }
    }
    lines.push(Line::from(""));
    if let Some(editor) = &manager.editor {
        lines.push(Line::from(Span::styled(
            format!("{} · Tab changes kind", editor.kind.label()),
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(format!("> {}", editor.text)));
        lines.push(Line::from("Enter save · Esc cancel"));
    } else if let Some(confirmation) = &manager.confirmation {
        let action = match confirmation.action {
            crate::app::MemoryConfirmationAction::Delete { .. } => "Delete memory entry?",
            crate::app::MemoryConfirmationAction::AcceptProposal { .. } => {
                "Accept proposal and persist it?"
            }
        };
        lines.push(Line::from(Span::styled(
            action,
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(if confirmation.confirm_selected {
            "  Cancel     [Confirm]"
        } else {
            " [Cancel]     Confirm"
        }));
    } else if manager.busy {
        lines.push(Line::from(Span::styled(
            "Updating memory…",
            Style::default().fg(Color::Yellow),
        )));
    } else if let Some(error) = &manager.error {
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(error),
            Style::default().fg(Color::Red),
        )));
    } else if let Some(notice) = &manager.notice {
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(notice),
            Style::default().fg(Color::Green),
        )));
    } else {
        lines.push(Line::from(
            "Shift+A add · Shift+E edit · Shift+D delete/reject · Shift+R recall · Shift+G proposals · Shift+X export · Esc close",
        ));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_keymap_hub(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(hub) = state.keymap_hub.as_ref() else {
        return;
    };
    let width = 84.min(area.width.saturating_sub(2));
    let height = 23.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    let filter = hub.filter.trim().to_ascii_lowercase();
    let visible = crate::keymap::KeyAction::ALL
        .into_iter()
        .filter(|action| {
            filter.is_empty()
                || action.label().to_ascii_lowercase().contains(&filter)
                || action.config_key().contains(&filter)
                || state
                    .keymap
                    .display_binding(*action)
                    .to_ascii_lowercase()
                    .contains(&filter)
        })
        .collect::<Vec<_>>();
    let selected = hub.selected_idx.min(visible.len().saturating_sub(1));
    let mut items = vec![
        ListItem::new(Line::from(Span::styled(
            " Keyboard shortcuts",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))),
        ListItem::new(Line::from(vec![
            Span::styled(" > ", Style::default().fg(Color::Cyan)),
            Span::styled(
                if hub.filter.is_empty() {
                    "type to filter…"
                } else {
                    &hub.filter
                },
                Style::default().fg(Color::White),
            ),
        ])),
        ListItem::new(""),
    ];
    if visible.is_empty() {
        items.push(ListItem::new(Span::styled(
            "  No matching shortcuts",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let capacity = height.saturating_sub(11).max(1) as usize;
        let start = selected.saturating_add(1).saturating_sub(capacity);
        for (index, action) in visible.iter().enumerate().skip(start).take(capacity) {
            let active = index == selected;
            let marker = if active { "▸" } else { " " };
            let customized = if state.keymap_customized.contains(action) {
                "  custom"
            } else {
                ""
            };
            let line = if width < 70 {
                format!(
                    " {marker} {} · {}{}",
                    action.label(),
                    state.keymap.display_binding(*action),
                    customized
                )
            } else {
                format!(
                    " {marker} {:<25} {:<24}{customized}",
                    action.label(),
                    state.keymap.display_binding(*action)
                )
            };
            items.push(ListItem::new(Span::styled(
                pad_right(&line, width.saturating_sub(2) as usize),
                if active {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                },
            )));
        }
    }
    items.push(ListItem::new(""));
    if width < 70 {
        items.push(ListItem::new(Span::styled(
            "  Fixed: Ctrl+R Diff · Ctrl+C cancel/quit",
            Style::default().fg(Color::DarkGray),
        )));
        items.push(ListItem::new(Span::styled(
            "  Enter/Esc and modal decisions are fixed",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        items.push(ListItem::new(Span::styled(
            "  Fixed: Ctrl+R Diff · Ctrl+C cancel/quit · Enter/Esc and modal decisions",
            Style::default().fg(Color::DarkGray),
        )));
    }
    if let Some(capture) = &hub.capture {
        let instruction = capture.first.map_or_else(
            || {
                format!(
                    "Press the first key for {} · Esc cancel",
                    capture.action.label()
                )
            },
            |first| {
                if width < 70 {
                    format!("{first} captured · Enter one stroke · next key two")
                } else {
                    format!(
                        "Captured {first} · Enter saves one stroke · press a second key · Esc cancel"
                    )
                }
            },
        );
        items.push(ListItem::new(Span::styled(
            instruction,
            Style::default().fg(Color::Yellow),
        )));
    } else if hub.saving {
        items.push(ListItem::new(Span::styled(
            "Saving…",
            Style::default().fg(Color::Yellow),
        )));
    } else if hub.writes_enabled {
        items.push(ListItem::new(Span::styled(
            if width < 70 {
                "Enter capture · ⇧R reset · Esc close · ↑↓ move"
            } else {
                "Enter capture · ⇧R reset selected · Esc close · ↑↓ navigate"
            },
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        items.push(ListItem::new(Span::styled(
            "View only in bare mode · Esc close · ↑↓ navigate",
            Style::default().fg(Color::DarkGray),
        )));
    }
    if let Some(notice) = &hub.notice {
        items.push(ListItem::new(Span::styled(
            crate::terminal_text::sanitize_terminal_text(notice),
            Style::default().fg(Color::Green),
        )));
    }
    if let Some(error) = &hub.error {
        items.push(ListItem::new(Span::styled(
            format!(
                "Error: {}",
                crate::terminal_text::sanitize_terminal_text(error)
            ),
            Style::default().fg(Color::Red),
        )));
    }
    frame.render_widget(Clear, popup);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .title(" Keymap ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        popup,
    );
}

fn render_history_search(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(history) = state.history_search.as_ref() else {
        return;
    };
    let width = 84.min(area.width.saturating_sub(2));
    let height = 21.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    let filter = history.filter.to_ascii_lowercase();
    let visible = state
        .input_history
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, entry)| filter.is_empty() || entry.to_ascii_lowercase().contains(&filter))
        .collect::<Vec<_>>();
    let selected = history.selected_idx.min(visible.len().saturating_sub(1));
    let mut items = vec![
        ListItem::new(Line::from(Span::styled(
            " Input history · newest first",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))),
        ListItem::new(Line::from(vec![
            Span::styled(" > ", Style::default().fg(Color::Cyan)),
            Span::styled(
                if history.filter.is_empty() {
                    "type to filter…"
                } else {
                    &history.filter
                },
                Style::default().fg(Color::White),
            ),
        ])),
        ListItem::new(Span::styled(
            format!("  {} matching · process only", visible.len()),
            Style::default().fg(Color::DarkGray),
        )),
        ListItem::new(""),
    ];
    if visible.is_empty() {
        items.push(ListItem::new(Span::styled(
            "  No matching input history",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let capacity = height.saturating_sub(8).max(1) as usize;
        let start = selected.saturating_add(1).saturating_sub(capacity);
        for (index, (_, entry)) in visible.iter().enumerate().skip(start).take(capacity) {
            let active = index == selected;
            let marker = if active { "▸" } else { " " };
            let safe = crate::terminal_text::sanitize_terminal_text(entry).replace('\n', " ↵ ");
            let max = width.saturating_sub(6) as usize;
            let mut shown = safe.chars().take(max).collect::<String>();
            if safe.chars().count() > max {
                shown.push('…');
            }
            items.push(ListItem::new(Span::styled(
                pad_right(
                    &format!(" {marker} {shown}"),
                    width.saturating_sub(2) as usize,
                ),
                if active {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                },
            )));
        }
    }
    items.push(ListItem::new(""));
    items.push(ListItem::new(Span::styled(
        if width < 70 {
            "Enter restore · Esc close · ↑↓ move · PgUp/PgDn"
        } else {
            "Enter restore as draft · Esc close · ↑↓ navigate · PgUp/PgDn page"
        },
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .title(" History ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        popup,
    );
}

fn prompt_stash_age(
    created_at: chrono::DateTime<chrono::Utc>,
    snapshot_at: chrono::DateTime<chrono::Utc>,
) -> String {
    let seconds = snapshot_at
        .signed_duration_since(created_at)
        .num_seconds()
        .max(0);
    match seconds {
        0..=59 => "now".into(),
        60..=3_599 => format!("{}m", seconds / 60),
        3_600..=86_399 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

fn prompt_stash_preview(text: &str, maximum: usize) -> String {
    let safe = crate::terminal_text::sanitize_terminal_text(text);
    let collapsed = safe.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview = collapsed.chars().take(maximum).collect::<String>();
    if collapsed.chars().count() > maximum {
        preview.push('…');
    }
    preview
}

fn render_prompt_stash(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(picker) = state.prompt_stash.as_ref() else {
        return;
    };
    let width = 86.min(area.width.saturating_sub(2));
    let height = 22.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    let selected = picker
        .selected_idx
        .min(picker.entries.len().saturating_sub(1));
    let mut items = vec![
        ListItem::new(Line::from(Span::styled(
            " Saved composer drafts · newest first",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))),
        ListItem::new(Span::styled(
            format!(
                "  {} of {} entries · 64 KiB each · 1 MiB total",
                picker.entries.len(),
                crate::prompt_stash::MAX_ENTRIES
            ),
            Style::default().fg(Color::DarkGray),
        )),
        ListItem::new(""),
    ];
    if picker.entries.is_empty() {
        items.push(ListItem::new(Span::styled(
            if picker.writes_enabled {
                "  No saved drafts · use /stash save or Ctrl+X S"
            } else {
                "  Persistent drafts are unavailable in bare mode"
            },
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let capacity = height.saturating_sub(11).max(2) as usize;
        let start = selected.saturating_add(1).saturating_sub(capacity);
        for (index, entry) in picker.entries.iter().enumerate().skip(start).take(capacity) {
            let active = index == selected;
            let marker = if active { "▸" } else { " " };
            let age = prompt_stash_age(entry.created_at, picker.snapshot_at);
            let lines = entry.line_count();
            let reserved = if width < 54 { 22 } else { 28 };
            let preview =
                prompt_stash_preview(&entry.text, width.saturating_sub(reserved) as usize);
            let line_label = if lines == 1 { "line" } else { "lines" };
            let row = format!(" {marker} {age:>4} · {lines:>3} {line_label:<5} · {preview}");
            items.push(ListItem::new(Span::styled(
                pad_right(&row, width.saturating_sub(2) as usize),
                if active {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                },
            )));
        }
        if let Some(entry) = picker.selected() {
            items.push(ListItem::new(""));
            items.push(ListItem::new(Span::styled(
                "  Preview",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )));
            let safe = crate::terminal_text::sanitize_terminal_text(&entry.text);
            for line in safe.lines().take(3) {
                items.push(ListItem::new(Span::styled(
                    format!(
                        "  {}",
                        prompt_stash_preview(line, width.saturating_sub(6) as usize)
                    ),
                    Style::default().fg(Color::Gray),
                )));
            }
        }
    }
    items.push(ListItem::new(""));
    let controls = if picker.busy {
        "Working…"
    } else if picker.writes_enabled {
        "Enter restore/swap · d twice delete · Esc close · ↑↓ navigate"
    } else {
        "View only in bare mode · Esc close"
    };
    items.push(ListItem::new(Span::styled(
        format!("  {controls}"),
        Style::default().fg(Color::DarkGray),
    )));
    if let Some(notice) = &picker.notice {
        items.push(ListItem::new(Span::styled(
            format!("  {}", crate::terminal_text::sanitize_terminal_text(notice)),
            Style::default().fg(Color::Green),
        )));
    }
    if let Some(error) = &picker.error {
        items.push(ListItem::new(Span::styled(
            format!(
                "  Error: {}",
                crate::terminal_text::sanitize_terminal_text(error)
            ),
            Style::default().fg(Color::Red),
        )));
    }
    frame.render_widget(Clear, popup);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .title(" Prompt Stash ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        popup,
    );
}

fn render_btw(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(btw) = state.btw.as_ref() else {
        return;
    };
    let width = 88.min(area.width.saturating_sub(2));
    let height = 22.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Model  ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{}/{}", btw.provider, btw.model)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Question",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
    ];
    if btw.status == crate::app::BtwStatus::Editing {
        lines.push(Line::from(vec![
            Span::raw("> "),
            Span::raw(btw.question[..btw.cursor].to_string()),
            Span::styled("▏", Style::default().fg(Color::Cyan)),
            Span::raw(btw.question[btw.cursor..].to_string()),
        ]));
    } else {
        lines.push(Line::from(format!("> {}", btw.question)));
    }
    lines.push(Line::from(""));
    let (status, style) = match &btw.status {
        crate::app::BtwStatus::Editing => ("Ready", Style::default().fg(Color::DarkGray)),
        crate::app::BtwStatus::Running => ("Answering…", Style::default().fg(Color::Yellow)),
        crate::app::BtwStatus::Complete => ("Answer", Style::default().fg(Color::Green)),
        crate::app::BtwStatus::Failed(_) => ("Could not answer", Style::default().fg(Color::Red)),
        crate::app::BtwStatus::Cancelled => ("Cancelled", Style::default().fg(Color::Yellow)),
    };
    lines.push(Line::from(Span::styled(
        status,
        style.add_modifier(Modifier::BOLD),
    )));
    match &btw.status {
        crate::app::BtwStatus::Editing => lines.push(Line::from(Span::styled(
            "This answer stays out of the conversation and saved session.",
            Style::default().fg(Color::DarkGray),
        ))),
        crate::app::BtwStatus::Running if btw.answer.is_empty() => {
            lines.push(Line::from(Span::styled(
                "Waiting for the provider…",
                Style::default().fg(Color::DarkGray),
            )))
        }
        crate::app::BtwStatus::Failed(error) => {
            if !btw.answer.is_empty() {
                lines.extend(btw.answer.lines().map(|line| Line::from(line.to_string())));
                lines.push(Line::from(""));
            }
            lines.push(Line::from(Span::styled(
                error.clone(),
                Style::default().fg(Color::Red),
            )));
        }
        crate::app::BtwStatus::Cancelled if btw.answer.is_empty() => {
            lines.push(Line::from(Span::styled(
                "No side answer was kept.",
                Style::default().fg(Color::DarkGray),
            )))
        }
        _ => lines.extend(btw.answer.lines().map(|line| Line::from(line.to_string()))),
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        match btw.status {
            crate::app::BtwStatus::Editing => "Enter ask · Esc close",
            crate::app::BtwStatus::Running => "Esc cancel side question · ↑↓ scroll",
            _ => "Enter/Esc close · ↑↓ scroll",
        },
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .title(" BTW · ephemeral ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .scroll((btw.scroll, 0))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_voice(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(voice) = state.voice.as_ref() else {
        return;
    };
    let width = 68.min(area.width.saturating_sub(2));
    let height = if voice.error.is_some() { 12 } else { 10 };
    let popup = centered_rect(width, height.min(area.height.saturating_sub(2)), area);
    frame.render_widget(Clear, popup);
    let theme = palette(state);
    let block = Block::default()
        .title(" Voice input ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let (status, style, detail, controls) = match voice.phase {
        crate::voice::VoicePhase::Ready => (
            "Ready",
            Style::default().fg(theme.muted),
            "No microphone audio is captured until you press Space.",
            "Space start recording · Esc close",
        ),
        crate::voice::VoicePhase::Recording => (
            "Recording…",
            Style::default().fg(theme.warning),
            "Audio remains process-only and is discarded after transcription.",
            "Space/Enter stop and transcribe · Esc discard",
        ),
        crate::voice::VoicePhase::Transcribing => (
            "Transcribing…",
            Style::default().fg(theme.warning),
            "The result becomes an editable draft and is never sent automatically.",
            "Esc cancel and discard",
        ),
        crate::voice::VoicePhase::Failed => (
            "Could not transcribe",
            Style::default().fg(theme.error),
            "Nothing was added to the conversation or saved session.",
            "Space retry · Esc close",
        ),
    };
    let mut lines = vec![
        Line::from(Span::styled(status, style)),
        Line::from(""),
        Line::from(detail),
    ];
    if let Some(error) = &voice.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(error),
            Style::default().fg(theme.error),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        controls,
        Style::default().fg(theme.muted),
    )));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_provider_discovery(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(discovery) = state.provider_discovery.as_ref() else {
        return;
    };
    let width = 88.min(area.width.saturating_sub(4));
    let height = 22.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    let mut lines = Vec::new();
    lines.push(Line::from(""));
    if discovery.scanning {
        lines.push(Line::from(Span::styled(
            "Scanning approved 127.0.0.1 ports…",
            Style::default().fg(Color::Yellow),
        )));
    } else if let Some(error) = discovery.error.as_ref() {
        lines.push(Line::from(Span::styled(
            format!("Discovery failed: {error}"),
            Style::default().fg(Color::Red),
        )));
    } else if discovery.candidates.is_empty() {
        lines.push(Line::from("No compatible local providers found."));
        lines.push(Line::from(Span::styled(
            "Start a local OpenAI-compatible server, then rescan.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!(
                "{} compatible local {}",
                discovery.candidates.len(),
                if discovery.candidates.len() == 1 {
                    "provider"
                } else {
                    "providers"
                }
            ),
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
        for (index, candidate) in discovery.candidates.iter().enumerate() {
            let models = candidate
                .models
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let more = if candidate.models.len() > 2 {
                format!(" +{}", candidate.models.len() - 2)
            } else {
                String::new()
            };
            let row = format!(
                "{} {} · {} · {models}{more}",
                if index == discovery.selected_idx {
                    "▶"
                } else {
                    " "
                },
                candidate.provider_id,
                model_count_label(candidate.models.len())
            );
            let style = if index == discovery.selected_idx {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(row, style)));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        if discovery.scanning {
            "Esc cancel and close"
        } else {
            "Enter add · ⇧R rescan · Esc close · ↑↓ navigate"
        },
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .title(" Local provider discovery ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_provider_manager(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(manager) = state.provider_manager.as_ref() else {
        return;
    };
    match &manager.view {
        crate::app::ProviderManagerView::Providers => {
            render_provider_list(frame, area, state, manager)
        }
        crate::app::ProviderManagerView::Models { provider } => {
            render_provider_models(frame, area, state, manager, provider)
        }
        crate::app::ProviderManagerView::Templates { return_to } => {
            render_provider_templates(frame, area, manager, *return_to)
        }
        crate::app::ProviderManagerView::SetupProviders => {
            render_provider_setup_list(frame, area, state, manager)
        }
        crate::app::ProviderManagerView::SetupProvider { provider } => {
            render_provider_setup_provider(frame, area, state, manager, provider)
        }
        crate::app::ProviderManagerView::SetupFinish => {
            render_provider_setup_finish(frame, area, state, manager)
        }
    }
    if let Some(confirmation) = manager.delete_confirmation.as_ref() {
        render_provider_delete_confirmation(frame, area, manager, confirmation);
    }
}

fn render_provider_auth(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(auth) = state.provider_auth.as_ref() else {
        return;
    };
    let shows_mimo_discount = auth.provider == crate::provider_connect::FirstClassProvider::Mimo
        && matches!(
            auth.stage,
            crate::app::ProviderAuthStage::MethodPicker
                | crate::app::ProviderAuthStage::PlanPicker
                | crate::app::ProviderAuthStage::TermPicker
                | crate::app::ProviderAuthStage::PlanUsdInput
                | crate::app::ProviderAuthStage::PriceConfirmation
        );
    let width = 86.min(area.width.saturating_sub(2));
    let height = (if shows_mimo_discount { 22 } else { 20 }).min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("Connect {}", auth.provider.display_name()),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(crate::terminal_text::sanitize_terminal_text(&auth.prompt)),
    ];
    if auth.bare {
        lines.push(Line::from(Span::styled(
            "Bare profile: this credential is memory-only and cannot become the saved default.",
            Style::default().fg(Color::Yellow),
        )));
    }
    if shows_mimo_discount {
        lines.push(Line::from(Span::styled(
            format!("Token Plan: {}", crate::pricing::MIMO_OFF_PEAK_COPY),
            Style::default().fg(Color::DarkGray),
        )));
    }
    if auth.url.is_some() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Authorization URL ready.",
            Style::default().fg(Color::Cyan),
        )));
        lines.push(Line::from(
            "Ctrl+Y copies it and opens a borderless selectable handoff.",
        ));
    }
    let picker_items: Option<Vec<String>> = match auth.stage {
        crate::app::ProviderAuthStage::MethodPicker => Some(
            auth.mimo_method_options()
                .into_iter()
                .map(|method| match method {
                    crate::app::MimoSetupMethod::SavedTokenPlan => format!(
                        "Use saved Token Plan credential ({}; no sign-in)",
                        auth.saved_mimo_region
                            .map_or("unknown region", crate::pricing::MimoRegion::label)
                    ),
                    crate::app::MimoSetupMethod::Browser => {
                        "Browser authorization (API or Token Plan)".into()
                    }
                    crate::app::MimoSetupMethod::ManualTokenPlan => {
                        "Manual Token Plan key (delegation eligible)".into()
                    }
                })
                .collect(),
        ),
        crate::app::ProviderAuthStage::RegionPicker => Some(
            crate::pricing::MimoRegion::ALL
                .iter()
                .map(|region| format!("{} · {}", region.label(), region.token_plan_url()))
                .collect(),
        ),
        crate::app::ProviderAuthStage::PlanPicker => Some(
            crate::pricing::MimoPlanTier::ALL
                .iter()
                .map(|tier| tier.label().to_string())
                .collect(),
        ),
        crate::app::ProviderAuthStage::TermPicker => Some(
            crate::pricing::MimoPlanTerm::ALL
                .iter()
                .map(|term| term.label().to_string())
                .collect(),
        ),
        _ => None,
    };
    if let Some(items) = picker_items {
        lines.push(Line::from(""));
        for (index, item) in items.iter().enumerate() {
            let style = if index == auth.selected_index {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "{} {item}",
                    if index == auth.selected_index {
                        "▸"
                    } else {
                        " "
                    }
                ),
                style,
            )));
        }
    }
    if let Some(code) = auth.code.as_deref() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("One-time code: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                crate::terminal_text::sanitize_terminal_text(code),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    if auth.stage == crate::app::ProviderAuthStage::SecretInput {
        let masked = "•".repeat(auth.input.len().min(width.saturating_sub(8) as usize));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::styled(masked, Style::default().fg(Color::White)),
        ]));
    }
    if auth.stage == crate::app::ProviderAuthStage::PlanUsdInput {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("$ ", Style::default().fg(Color::Cyan)),
            Span::styled(
                crate::terminal_text::sanitize_terminal_text(&auth.metadata_input),
                Style::default().fg(Color::White),
            ),
        ]));
    }
    if auth.stage == crate::app::ProviderAuthStage::PriceConfirmation {
        lines.push(Line::from(""));
        let cancel = if auth.selected_index == 0 {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        let confirm = if auth.selected_index == 1 {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(vec![
            Span::styled(
                if auth.selected_index == 0 {
                    "▸ Cancel "
                } else {
                    "  Cancel "
                },
                cancel,
            ),
            Span::raw("   "),
            Span::styled(
                if auth.selected_index == 1 {
                    "▸ Confirm actual USD "
                } else {
                    "  Confirm actual USD "
                },
                confirm,
            ),
        ]));
    }
    if let Some(error) = auth.error.as_deref() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(error),
            Style::default().fg(Color::Red),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        match auth.stage {
            crate::app::ProviderAuthStage::MethodPicker
            | crate::app::ProviderAuthStage::RegionPicker
            | crate::app::ProviderAuthStage::PlanPicker
            | crate::app::ProviderAuthStage::TermPicker => {
                "↑↓ choose · Enter continue · Esc cancel"
            }
            crate::app::ProviderAuthStage::SecretInput if auth.url.is_some() => {
                "Ctrl+Y copy/show URL · Enter continue · Esc cancel"
            }
            crate::app::ProviderAuthStage::SecretInput => "Enter continue · Esc cancel",
            crate::app::ProviderAuthStage::PlanUsdInput => {
                "Enter continue · decimal USD only · Esc cancel"
            }
            crate::app::ProviderAuthStage::PriceConfirmation => {
                "←→ choose · Enter confirm · Esc cancel"
            }
            crate::app::ProviderAuthStage::Failed => "Enter retry · Esc close",
            crate::app::ProviderAuthStage::Probing => "Testing one isolated request… · Esc cancel",
            crate::app::ProviderAuthStage::Starting if auth.bare => {
                "Enter continue without saving · Esc cancel"
            }
            _ if auth.url.is_some() => {
                "Ctrl+Y copy/show URL · Waiting for authorization… · Esc cancel"
            }
            _ => "Waiting for authorization… · Esc cancel",
        },
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .title(" Provider authentication ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_provider_setup_list(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &AppState,
    manager: &crate::app::ProviderManagerState,
) {
    let pw = 82.min(area.width.saturating_sub(4));
    let ph = 22.min(area.height.saturating_sub(2));
    let pa = centered_rect(pw, ph, area);
    let filter = manager.provider_filter.to_ascii_lowercase();
    let visible = manager
        .providers
        .iter()
        .filter(|entry| crate::tui::input::provider_matches_filter(entry, &filter))
        .collect::<Vec<_>>();
    let selected = manager
        .selected_provider_idx
        .min(visible.len().saturating_sub(1));
    let working = manager
        .providers
        .iter()
        .filter(|entry| {
            state
                .provider_checks
                .get(&entry.id)
                .is_some_and(|status| status.working_model().is_some())
        })
        .count();
    let mut items = vec![
        ListItem::new(Span::styled(
            " Provider setup",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        ListItem::new(Span::styled(
            " Tests send one tiny isolated request; provider usage may apply.",
            Style::default().fg(Color::Yellow),
        )),
        ListItem::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::styled(
                if manager.provider_filter.is_empty() {
                    "type to filter providers…"
                } else {
                    &manager.provider_filter
                },
                Style::default().fg(Color::White),
            ),
        ])),
        ListItem::new(Span::styled(
            format!("  {working} of {} working", manager.providers.len()),
            Style::default().fg(Color::DarkGray),
        )),
        ListItem::new(""),
    ];
    let capacity = ph.saturating_sub(12).max(1) as usize;
    let start = selected.saturating_add(1).saturating_sub(capacity);
    for (index, entry) in visible.iter().enumerate().skip(start).take(capacity) {
        let is_selected = index == selected;
        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        let (check, _) = provider_check_label(state.provider_checks.get(&entry.id));
        let label = format!(
            "{}{} · {} · {}",
            if is_selected { "▸ " } else { "  " },
            crate::terminal_text::sanitize_terminal_text(&entry.id),
            provider_credential_label(&entry.api_key_source),
            check
        );
        items.push(ListItem::new(Span::styled(
            pad_right(&label, pw.saturating_sub(2) as usize),
            style,
        )));
    }
    if visible.is_empty() {
        items.push(ListItem::new(Span::styled(
            if manager.providers.is_empty() {
                "  No providers configured. ⇧P templates · ⇧A custom"
            } else {
                "  No matching providers"
            },
            Style::default().fg(Color::DarkGray),
        )));
    }
    items.push(ListItem::new(""));
    if let Some(error) = manager.error.as_deref() {
        items.push(ListItem::new(Span::styled(
            error,
            Style::default().fg(Color::Red),
        )));
    }
    items.push(ListItem::new(Span::styled(
        "Enter details · ⇧T test · ⇧E edit · ⇧F finish",
        Style::default().fg(Color::DarkGray),
    )));
    items.push(ListItem::new(Span::styled(
        "⇧P templates · ⇧O OpenAI · ⇧A custom",
        Style::default().fg(Color::DarkGray),
    )));
    items.push(ListItem::new(Span::styled(
        "Esc close · ↑↓ move · PgUp/PgDn · Home/End",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Clear, pa);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        pa,
    );
}

fn render_provider_setup_provider(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &AppState,
    manager: &crate::app::ProviderManagerState,
    provider: &str,
) {
    let pw = 76.min(area.width.saturating_sub(4));
    let ph = 16.min(area.height.saturating_sub(4));
    let pa = centered_rect(pw, ph, area);
    let entry = manager.providers.iter().find(|entry| entry.id == provider);
    let (status, color) = provider_check_label(state.provider_checks.get(provider));
    let test_model = state
        .provider_checks
        .get(provider)
        .and_then(crate::provider_probe::ProviderCheckStatus::working_model)
        .map(str::to_owned)
        .or_else(|| {
            state
                .current_model
                .as_ref()
                .filter(|model| model.provider == provider)
                .map(|model| model.id.clone())
        })
        .or_else(|| entry.and_then(|entry| entry.configured_models.first().cloned()))
        .unwrap_or_else(|| "none configured".into());
    let endpoint = entry.map_or("", |entry| entry.base_url.as_str());
    let credential = entry.map_or_else(
        || "no credential".into(),
        |entry| provider_credential_label(&entry.api_key_source),
    );
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "Provider setup: {}",
                crate::terminal_text::sanitize_terminal_text(provider)
            ),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "Endpoint: {}",
            truncate_middle_chars(
                &crate::terminal_text::sanitize_terminal_text(endpoint),
                pw.saturating_sub(12) as usize,
            )
        )),
        Line::from(format!("Credential: {credential}")),
        Line::from(format!(
            "Test model: {}",
            crate::terminal_text::sanitize_terminal_text(&test_model)
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("Status: {status}"),
            Style::default().fg(color),
        )),
        Line::from(""),
        Line::from(Span::styled(
            if state.provider_check_running.as_deref() == Some(provider) {
                "Testing… Esc cancel"
            } else {
                "⇧T test provider · ⇧E edit settings/key · ⇧A add model"
            },
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "⇧R refresh models · Esc checklist",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    frame.render_widget(Clear, pa);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        pa,
    );
}

fn render_provider_setup_finish(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &AppState,
    manager: &crate::app::ProviderManagerState,
) {
    let pw = 72.min(area.width.saturating_sub(4));
    let ph = 16.min(area.height.saturating_sub(4));
    let pa = centered_rect(pw, ph, area);
    let passing = manager
        .providers
        .iter()
        .filter_map(|entry| {
            state
                .provider_checks
                .get(&entry.id)
                .and_then(crate::provider_probe::ProviderCheckStatus::working_model)
                .map(|model| (entry.id.as_str(), model))
        })
        .collect::<Vec<_>>();
    let selected = manager
        .selected_model_idx
        .min(passing.len().saturating_sub(1));
    let mut items = vec![
        ListItem::new(Span::styled(
            " Choose a working default",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        ListItem::new(""),
    ];
    for (index, (provider, model)) in passing.iter().enumerate() {
        let is_selected = index == selected;
        let label = format!(
            "{}{} / {}",
            if is_selected { "▸ " } else { "  " },
            crate::terminal_text::sanitize_terminal_text(provider),
            crate::terminal_text::sanitize_terminal_text(model)
        );
        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::Green)
        };
        items.push(ListItem::new(Span::styled(
            pad_right(&label, pw.saturating_sub(2) as usize),
            style,
        )));
    }
    items.push(ListItem::new(""));
    if manager.busy {
        items.push(ListItem::new(Span::styled(
            "Saving default…",
            Style::default().fg(Color::Yellow),
        )));
    } else if let Some(error) = manager.error.as_deref() {
        items.push(ListItem::new(Span::styled(
            error,
            Style::default().fg(Color::Red),
        )));
    }
    items.push(ListItem::new(Span::styled(
        "Enter select and finish · Esc checklist · ↑↓ move",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Clear, pa);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        pa,
    );
}

fn provider_check_label(
    status: Option<&crate::provider_probe::ProviderCheckStatus>,
) -> (String, Color) {
    use crate::provider_probe::ProviderCheckStatus;
    match status {
        None => ("not checked".into(), Color::DarkGray),
        Some(ProviderCheckStatus::Checking) => ("testing…".into(), Color::Yellow),
        Some(ProviderCheckStatus::Working { .. }) => ("working".into(), Color::Green),
        Some(ProviderCheckStatus::MissingEnvironment { variable }) => {
            (format!("env {variable} missing"), Color::Yellow)
        }
        Some(ProviderCheckStatus::CredentialRejected) => {
            ("credential rejected (401)".into(), Color::Red)
        }
        Some(ProviderCheckStatus::AccessDenied) => ("access denied (HTTP 403)".into(), Color::Red),
        Some(ProviderCheckStatus::TimedOut) => ("timed out".into(), Color::Yellow),
        Some(ProviderCheckStatus::CannotConnect) => ("cannot connect".into(), Color::Red),
        Some(ProviderCheckStatus::EndpointNotFound) => {
            ("endpoint not found (HTTP 404)".into(), Color::Red)
        }
        Some(ProviderCheckStatus::RateLimited) => ("rate limited (HTTP 429)".into(), Color::Yellow),
        Some(ProviderCheckStatus::ProviderError {
            status: Some(status),
        }) => (format!("provider error (HTTP {status})"), Color::Red),
        Some(ProviderCheckStatus::ProviderError { status: None }) => {
            ("provider request failed".into(), Color::Red)
        }
        Some(ProviderCheckStatus::InvalidResponse) => {
            ("invalid or incomplete response".into(), Color::Red)
        }
        Some(ProviderCheckStatus::FilterStopped) => {
            ("provider filter stopped".into(), Color::Yellow)
        }
        Some(ProviderCheckStatus::NeedsModel) => ("needs a model".into(), Color::Yellow),
        Some(ProviderCheckStatus::AuthenticationRequired) => {
            ("authentication required".into(), Color::Yellow)
        }
    }
}

fn render_provider_list(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &AppState,
    manager: &crate::app::ProviderManagerState,
) {
    let pw = 74.min(area.width.saturating_sub(4));
    let ph = 20.min(area.height.saturating_sub(4));
    let pa = centered_rect(pw, ph, area);
    let filter = manager.provider_filter.to_ascii_lowercase();
    let visible = manager
        .providers
        .iter()
        .filter(|entry| crate::tui::input::provider_matches_filter(entry, &filter))
        .collect::<Vec<_>>();
    let selected = manager
        .selected_provider_idx
        .min(visible.len().saturating_sub(1));
    let configured_count = manager
        .providers
        .iter()
        .filter(|entry| entry.configured)
        .count();
    let mut items = vec![
        ListItem::new(Span::styled(
            " Providers",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        ListItem::new(""),
        ListItem::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::styled(
                if manager.provider_filter.is_empty() {
                    "type to filter…"
                } else {
                    &manager.provider_filter
                },
                Style::default().fg(Color::White),
            ),
        ])),
        ListItem::new(Span::styled(
            format!(
                "  {}/{} matches · 4 quick-connect routes · {} configured provider{}",
                if visible.is_empty() { 0 } else { selected + 1 },
                visible.len(),
                configured_count,
                if configured_count == 1 { "" } else { "s" }
            ),
            Style::default().fg(Color::DarkGray),
        )),
        ListItem::new(""),
    ];
    let narrow_rows = pw < 64;
    let available_rows = ph.saturating_sub(12).max(1) as usize;
    let row_height = if narrow_rows { 2 } else { 1 };
    let capacity = (available_rows / row_height).max(1);
    let range =
        crate::tui::selection::ListViewport::default().visible(selected, visible.len(), capacity);
    for (index, entry) in visible
        .iter()
        .enumerate()
        .skip(range.start)
        .take(range.len())
    {
        let is_selected = index == selected;
        let is_current = state
            .current_model
            .as_ref()
            .is_some_and(|model| model.provider == entry.id);
        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        let provider_name = crate::provider_connect::FirstClassProvider::for_id(&entry.id)
            .map_or(entry.id.as_str(), |provider| provider.display_name());
        let connection = if entry.connected {
            "connected"
        } else {
            "disconnected"
        };
        let label = format!(
            "{}{}",
            if is_selected { "▸ " } else { "  " },
            crate::terminal_text::sanitize_terminal_text(provider_name),
        );
        let description = format!(
            "{} · {}{}",
            connection,
            if entry.configured {
                model_count_label(entry.configured_models.len())
            } else {
                "not configured".into()
            },
            if is_current { " · current" } else { "" }
        );
        let row_width = pw.saturating_sub(2) as usize;
        let row = crate::tui::selection::layout_row(
            &label,
            &description,
            row_width.saturating_sub(1),
            narrow_rows,
        );
        if row.stacked {
            items.push(ListItem::new(Span::styled(
                pad_right(&row.label, row_width),
                style,
            )));
            items.push(ListItem::new(Span::styled(
                pad_right(&row.description, row_width),
                style,
            )));
        } else {
            items.push(ListItem::new(Span::styled(
                pad_right(
                    &format!("{}{}{}", row.label, row.gap, row.description),
                    row_width,
                ),
                style,
            )));
        }
    }
    if visible.is_empty() {
        items.push(ListItem::new(Span::styled(
            if manager.providers.is_empty() {
                "  No providers configured. ⇧P templates · ⇧A custom"
            } else {
                "  No matching providers"
            },
            Style::default().fg(Color::DarkGray),
        )));
    }
    items.push(ListItem::new(""));
    if manager.busy {
        items.push(ListItem::new(Span::styled(
            "Saving provider changes…",
            Style::default().fg(Color::Yellow),
        )));
    } else if let Some(error) = manager.error.as_deref() {
        items.push(ListItem::new(Span::styled(
            error,
            Style::default().fg(Color::Red),
        )));
    }
    items.push(ListItem::new(Span::styled(
        "Enter connect/models · ⇧L reconnect · ⇧P templates",
        Style::default().fg(Color::DarkGray),
    )));
    items.push(ListItem::new(Span::styled(
        "⇧A custom · ⇧E edit · ⇧D delete · ⇧R refresh",
        Style::default().fg(Color::DarkGray),
    )));
    items.push(ListItem::new(Span::styled(
        "Esc close · ↑↓ wrap · PgUp/PgDn · Home/End",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Clear, pa);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        pa,
    );
}

fn render_provider_templates(
    frame: &mut ratatui::Frame,
    area: Rect,
    manager: &crate::app::ProviderManagerState,
    return_to: crate::app::ProviderTemplateReturn,
) {
    let pw = 82.min(area.width.saturating_sub(4));
    let ph = 22.min(area.height.saturating_sub(2));
    let pa = centered_rect(pw, ph, area);
    let visible = crate::tui::input::filtered_provider_templates(manager);
    let selected = manager
        .selected_template_idx
        .min(visible.len().saturating_sub(1));
    let ready = crate::provider_templates::ALL
        .iter()
        .filter(|template| {
            matches!(
                template.availability,
                crate::provider_templates::Availability::Ready(_)
            )
        })
        .count();
    let mut items = vec![
        ListItem::new(Span::styled(
            " Provider templates",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        ListItem::new(Span::styled(
            " Select a compatible template to review before saving.",
            Style::default().fg(Color::DarkGray),
        )),
        ListItem::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::styled(
                if manager.template_filter.is_empty() {
                    "type to filter names, aliases, or key env…"
                } else {
                    &manager.template_filter
                },
                Style::default().fg(Color::White),
            ),
        ])),
        ListItem::new(Span::styled(
            format!(
                "  {ready} ready · {} need integration",
                crate::provider_templates::ALL.len().saturating_sub(ready)
            ),
            Style::default().fg(Color::DarkGray),
        )),
        ListItem::new(""),
    ];
    let capacity = ph.saturating_sub(11).max(1) as usize;
    let start = selected.saturating_add(1).saturating_sub(capacity);
    for (index, template) in visible.iter().enumerate().skip(start).take(capacity) {
        let is_selected = index == selected;
        let configured = manager
            .providers
            .iter()
            .any(|entry| entry.id == template.provider_id);
        let status = if configured {
            "configured"
        } else {
            template.availability.label()
        };
        let raw = format!(
            "{}{} · {}",
            if is_selected { "▸ " } else { "  " },
            crate::terminal_text::sanitize_terminal_text(template.display_name),
            status
        );
        let label = truncate_middle_chars(&raw, pw.saturating_sub(2) as usize);
        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else if configured {
            Style::default().fg(Color::Green)
        } else if template.is_installable() {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Yellow)
        };
        items.push(ListItem::new(Span::styled(
            pad_right(&label, pw.saturating_sub(2) as usize),
            style,
        )));
    }
    if visible.is_empty() {
        items.push(ListItem::new(Span::styled(
            "  No matching provider templates",
            Style::default().fg(Color::DarkGray),
        )));
    }
    items.push(ListItem::new(""));
    if let Some(error) = manager.error.as_deref() {
        items.push(ListItem::new(Span::styled(
            truncate_middle_chars(error, pw.saturating_sub(2) as usize),
            Style::default().fg(Color::Red),
        )));
    }
    items.push(ListItem::new(Span::styled(
        "Enter review · ↑↓ move · PgUp/PgDn · Home/End",
        Style::default().fg(Color::DarkGray),
    )));
    items.push(ListItem::new(Span::styled(
        match return_to {
            crate::app::ProviderTemplateReturn::Providers => {
                "Esc providers · unavailable rows show requirements"
            }
            crate::app::ProviderTemplateReturn::SetupProviders => {
                "Esc setup · unavailable rows show requirements"
            }
        },
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Clear, pa);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        pa,
    );
}

fn render_provider_models(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &AppState,
    manager: &crate::app::ProviderManagerState,
    provider: &str,
) {
    let pw = 76.min(area.width.saturating_sub(4));
    let ph = 22.min(area.height.saturating_sub(2));
    let pa = centered_rect(pw, ph, area);
    let entry = manager.providers.iter().find(|entry| entry.id == provider);
    let filter = manager.model_filter.to_ascii_lowercase();
    let visible = state
        .model_list
        .iter()
        .filter(|model| model.provider == provider)
        .filter(|model| crate::tui::input::model_matches_filter(model, &filter))
        .collect::<Vec<_>>();
    let selected = manager
        .selected_model_idx
        .min(visible.len().saturating_sub(1));
    let endpoint = entry.map_or("", |entry| entry.base_url.as_str());
    let credential = entry.map_or("none".into(), |entry| {
        provider_credential_label(&entry.api_key_source)
    });
    let catalog = state
        .model_catalog_provider_status
        .get(provider)
        .map(String::as_str)
        .unwrap_or(if state.model_catalog_refreshing {
            "refreshing"
        } else {
            "configured only"
        });
    let mut items = vec![
        ListItem::new(Span::styled(
            format!(
                " Provider: {}",
                crate::terminal_text::sanitize_terminal_text(provider)
            ),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        ListItem::new(Span::styled(
            format!(
                " Endpoint: {}",
                truncate_middle_chars(
                    &crate::terminal_text::sanitize_terminal_text(endpoint),
                    pw.saturating_sub(13) as usize
                )
            ),
            Style::default().fg(Color::DarkGray),
        )),
        ListItem::new(Span::styled(
            format!(
                " Credential: {credential} · {}",
                crate::terminal_text::sanitize_terminal_text(catalog)
            ),
            Style::default().fg(Color::DarkGray),
        )),
        ListItem::new(""),
        ListItem::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::styled(
                if manager.model_filter.is_empty() {
                    "type to filter models…"
                } else {
                    &manager.model_filter
                },
                Style::default().fg(Color::White),
            ),
        ])),
        ListItem::new(""),
    ];
    let capacity = ph.saturating_sub(12).max(1) as usize;
    let start = selected.saturating_add(1).saturating_sub(capacity);
    for (index, model) in visible.iter().enumerate().skip(start).take(capacity) {
        let is_selected = index == selected;
        let is_current = state.current_model.as_ref() == Some(*model);
        let configured = entry.is_some_and(|entry| entry.configured_models.contains(&model.id));
        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        let label = format!(
            "{}{}{}{}",
            if is_selected { "▸ " } else { "  " },
            crate::terminal_text::sanitize_terminal_text(&model.display_name),
            if state.model_is_configured_only(model) {
                " · configured only"
            } else if configured {
                " · configured"
            } else {
                " · available"
            },
            if is_current { " · current" } else { "" }
        );
        items.push(ListItem::new(Span::styled(
            pad_right(&label, pw.saturating_sub(2) as usize),
            style,
        )));
    }
    if visible.is_empty() {
        items.push(ListItem::new(Span::styled(
            "  No matching models. ⇧A add model",
            Style::default().fg(Color::DarkGray),
        )));
    }
    items.push(ListItem::new(""));
    if manager.busy {
        items.push(ListItem::new(Span::styled(
            "Saving provider changes…",
            Style::default().fg(Color::Yellow),
        )));
    } else if let Some(error) = manager.error.as_deref() {
        items.push(ListItem::new(Span::styled(
            error,
            Style::default().fg(Color::Red),
        )));
    }
    items.push(ListItem::new(Span::styled(
        "Enter select · ⇧A add model · ⇧E edit/add · ⇧C cache",
        Style::default().fg(Color::DarkGray),
    )));
    items.push(ListItem::new(Span::styled(
        "⇧S settings · ⇧R refresh · ⇧D delete provider · Esc providers",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Clear, pa);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        pa,
    );
}

fn render_provider_delete_confirmation(
    frame: &mut ratatui::Frame,
    area: Rect,
    manager: &crate::app::ProviderManagerState,
    confirmation: &crate::app::ProviderDeleteConfirmation,
) {
    let pw = 68.min(area.width.saturating_sub(4));
    let ph = 12.min(area.height.saturating_sub(4));
    let pa = centered_rect(pw, ph, area);
    let replacement = confirmation.replacement.as_ref().map_or_else(
        || "Current model will not change".to_string(),
        |model| format!("Switch to {}/{}", model.provider, model.id),
    );
    let key_line = if confirmation.provider == "openai-codex" {
        "ca will attempt OpenAI token revocation, then delete the local Codex cache."
    } else if matches!(confirmation.provider.as_str(), "mimo" | "deepseek") {
        "The saved credential is deleted locally; this does not revoke its upstream API key."
    } else if confirmation.removes_keychain {
        "The saved credential will also be deleted."
    } else {
        "No saved credential is attached to this provider."
    };
    let cancel_style = if !confirmation.confirm_selected {
        Style::default().fg(Color::Black).bg(Color::Cyan)
    } else {
        Style::default().fg(Color::White)
    };
    let delete_style = if confirmation.confirm_selected {
        Style::default().fg(Color::Black).bg(Color::Red)
    } else {
        Style::default().fg(Color::Red)
    };
    let configured_models = match confirmation.configured_model_count {
        1 => "1 configured model".to_string(),
        count => format!("{count} configured models"),
    };
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("Delete provider {}?", confirmation.provider),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "Removes {configured_models} and provider-scoped settings."
        )),
        Line::from(key_line),
        Line::from(replacement),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Cancel  ", cancel_style),
            Span::raw("    "),
            Span::styled("  Delete  ", delete_style),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            if manager.busy {
                "Deleting provider…"
            } else {
                "←→ choose · Enter confirm · Esc cancel"
            },
            Style::default().fg(Color::DarkGray),
        )),
    ];
    frame.render_widget(Clear, pa);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(
            Block::default()
                .title(" Delete provider ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red)),
        ),
        pa,
    );
}

fn provider_credential_label(source: &crate::config::ProviderApiKeySource) -> String {
    match source {
        crate::config::ProviderApiKeySource::ApplicationDefaultCredentials => {
            "Application Default Credentials".into()
        }
        crate::config::ProviderApiKeySource::None => "no credential".into(),
        crate::config::ProviderApiKeySource::Environment(environment) => {
            format!("env {environment}")
        }
        crate::config::ProviderApiKeySource::Keychain => "saved credential".into(),
        crate::config::ProviderApiKeySource::Hidden => "credential configured".into(),
    }
}

fn model_count_label(count: usize) -> String {
    format!("{count} {}", if count == 1 { "model" } else { "models" })
}

fn compact_token_count(tokens: u64) -> String {
    if tokens >= 1_000 {
        format!("{}K", tokens / 1_000)
    } else {
        tokens.to_string()
    }
}

fn render_title(frame: &mut ratatui::Frame, area: Rect, state: &AppState, version_label: &str) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let theme = palette(state);
    let context = if let Some(window) = state.current_context_window_tokens {
        format!(
            "{} / {}",
            compact_token_count(state.current_context_tokens),
            compact_token_count(window)
        )
    } else if state.current_context_tokens > 0 {
        format!(
            "{} context",
            compact_token_count(state.current_context_tokens)
        )
    } else {
        String::new()
    };
    let mut right = if context.is_empty() {
        "[Dashboard]".to_string()
    } else {
        format!("{context} │ [Dashboard]")
    };
    let identity = format!(" {version_label}");
    let identity_width = unicode_width::UnicodeWidthStr::width(identity.as_str());
    let remaining_width = (area.width as usize).saturating_sub(identity_width + 2);
    if unicode_width::UnicodeWidthStr::width(right.as_str()) > remaining_width {
        right = if remaining_width >= "[Dashboard]".len() {
            "[Dashboard]".to_string()
        } else {
            String::new()
        };
    }
    let right_width = unicode_width::UnicodeWidthStr::width(right.as_str());
    let workspace_width = (area.width as usize).saturating_sub(identity_width + right_width + 5);
    let workspace = crate::terminal_text::sanitize_terminal_text(&state.cwd);
    let workspace = crate::tui::selection::truncate_cells(&workspace, workspace_width);
    let left = if workspace.is_empty() {
        identity
    } else {
        format!("{identity} │ {workspace}")
    };
    frame.render_widget(
        Paragraph::new(Span::styled(left, Style::default().fg(theme.text))),
        area,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(right, Style::default().fg(theme.muted)))
            .alignment(Alignment::Right),
        Rect::new(
            area.right().saturating_sub(right_width as u16 + 1),
            area.y,
            right_width as u16,
            1,
        ),
    );
}

fn composer_metadata(state: &AppState) -> String {
    let mode_str = match state.mode {
        crate::app::AgentMode::Agent => "agent",
        crate::app::AgentMode::Plan => "plan",
        crate::app::AgentMode::Ask => "ask",
        crate::app::AgentMode::Debug => "debug",
        crate::app::AgentMode::Custom(ref name) => name.as_str(),
    };
    let model = state
        .current_model
        .as_ref()
        .map(|m| m.display_name.as_str())
        .unwrap_or("auto");
    let mut details = Vec::new();
    if !state
        .statusline_fields
        .contains(&crate::ui_settings::StatusField::Mode)
    {
        details.push(mode_str.to_string());
    }
    if !state
        .statusline_fields
        .contains(&crate::ui_settings::StatusField::Model)
    {
        details.push(
            state
                .reasoning_effort
                .map_or_else(|| model.to_string(), |effort| format!("{model} ({effort})")),
        );
    }
    if !state
        .statusline_fields
        .contains(&crate::ui_settings::StatusField::Profile)
    {
        match &state.active_profile {
            crate::runtime::ActiveProfile::Default => {}
            profile => details.push(format!("profile {}", profile.label())),
        }
    }
    match state.approval {
        crate::app::ApprovalMode::Allowlist => {}
        crate::app::ApprovalMode::AutoReview => details.push("auto-review".into()),
        crate::app::ApprovalMode::RunEverything => details.push("always-approve".into()),
    }
    if !state.run_capabilities.session_persistence
        && !matches!(state.active_profile, crate::runtime::ActiveProfile::Bare)
    {
        details.push("no session".into());
    }
    if let Some(goal) = state.goal_state.as_ref() {
        let status = if state.goal_evaluating {
            "evaluating"
        } else {
            goal.status.label()
        };
        details.push(format!(
            "goal {} {}/{}",
            status, goal.continuations, goal.limits.max_continuations
        ));
    }
    if let Some(run) = state
        .workflow_run
        .as_ref()
        .filter(|run| !run.status.is_terminal())
    {
        let step = run
            .current_step
            .as_deref()
            .map_or_else(String::new, |step| format!(" · {step}"));
        details.push(format!("workflow {}{step}", run.status.label()));
    }
    if let Some(record) = state.attached_worktree.as_ref() {
        details.push(format!("wt {}", record.label));
    }
    if let Some(run) = state
        .multi_run_manager
        .as_ref()
        .and_then(|manager| manager.run.as_ref())
        .or(state.multi_run_stashed.as_ref())
        .filter(|run| run.status != crate::multi_run::MultiRunStatus::Complete)
    {
        details.push(format!("multi-run {}", run.status.label()));
    }

    crate::terminal_text::sanitize_terminal_text(&details.join(" · "))
}

#[derive(Clone, Copy)]
enum ExplorationCallRef<'a> {
    Complete(&'a ToolCall),
    Partial(&'a PartialToolCall),
}

impl<'a> ExplorationCallRef<'a> {
    fn id(self) -> Option<&'a str> {
        match self {
            Self::Complete(call) => Some(&call.id),
            Self::Partial(call) => call.id.as_deref(),
        }
    }

    fn name(self) -> Option<&'a str> {
        match self {
            Self::Complete(call) => Some(&call.name),
            Self::Partial(call) => call.name.as_deref(),
        }
    }

    fn arguments(self) -> &'a str {
        match self {
            Self::Complete(call) => &call.arguments,
            Self::Partial(call) => &call.arguments,
        }
    }

    fn is_active(self) -> bool {
        match self {
            Self::Complete(call) => {
                matches!(call.status, ToolStatus::Pending | ToolStatus::Running)
            }
            Self::Partial(_) => true,
        }
    }

    fn is_error(self) -> bool {
        matches!(self, Self::Complete(call) if call.status == ToolStatus::Error)
    }
}

pub(crate) struct ExplorationGroup<'a> {
    pub(crate) key: TranscriptBlockKey,
    calls: Vec<ExplorationCallRef<'a>>,
}

impl ExplorationGroup<'_> {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.calls.len()
    }
}

fn is_exploration_tool(name: &str) -> bool {
    crate::agent::registry::ToolRegistry::builtins().presentation(name)
        == crate::agent::registry::ToolPresentation::Exploration
}

fn activity_tool_kind(name: &str) -> Option<crate::agent::registry::ToolActivityKind> {
    match crate::agent::registry::ToolRegistry::builtins().presentation(name) {
        crate::agent::registry::ToolPresentation::Activity(kind) => Some(kind),
        crate::agent::registry::ToolPresentation::Ordinary
        | crate::agent::registry::ToolPresentation::Exploration => None,
    }
}

#[derive(Clone, Copy)]
enum ActivityCallRef<'a> {
    Complete(&'a ToolCall),
    Partial(&'a PartialToolCall),
}

impl<'a> ActivityCallRef<'a> {
    fn id(self) -> Option<&'a str> {
        match self {
            Self::Complete(call) => Some(&call.id),
            Self::Partial(call) => call.id.as_deref(),
        }
    }

    fn name(self) -> Option<&'a str> {
        match self {
            Self::Complete(call) => Some(&call.name),
            Self::Partial(call) => call.name.as_deref(),
        }
    }

    fn kind(self) -> Option<crate::agent::registry::ToolActivityKind> {
        self.name().and_then(activity_tool_kind)
    }

    fn is_active(self) -> bool {
        match self {
            Self::Complete(call) => {
                matches!(call.status, ToolStatus::Pending | ToolStatus::Running)
            }
            Self::Partial(_) => true,
        }
    }

    fn is_error(self) -> bool {
        matches!(self, Self::Complete(call) if call.status == ToolStatus::Error)
    }
}

pub(crate) struct ActivityGroup<'a> {
    pub(crate) key: TranscriptBlockKey,
    calls: Vec<ActivityCallRef<'a>>,
}

impl ActivityGroup<'_> {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.calls.len()
    }
}

fn finish_activity_group<'a>(
    current: &mut Vec<ActivityCallRef<'a>>,
    groups: &mut Vec<ActivityGroup<'a>>,
) {
    let Some(first_id) = current.first().and_then(|call| call.id()) else {
        current.clear();
        return;
    };
    if current.len() >= 2 {
        if let Some(key) = TranscriptBlockKey::tool_calls(first_id) {
            groups.push(ActivityGroup {
                key,
                calls: std::mem::take(current),
            });
            return;
        }
    }
    current.clear();
}

fn collect_activity_call<'a>(
    call: ActivityCallRef<'a>,
    current: &mut Vec<ActivityCallRef<'a>>,
    groups: &mut Vec<ActivityGroup<'a>>,
) {
    if call.id().is_some_and(|id| !id.is_empty()) && call.kind().is_some() {
        current.push(call);
    } else {
        finish_activity_group(current, groups);
    }
}

pub(crate) fn activity_groups(state: &AppState) -> Vec<ActivityGroup<'_>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();
    let mut current_turn = None;
    let mut annotations = std::collections::HashMap::new();
    for annotation in &state.transcript_annotations {
        annotations
            .entry(annotation.message_timestamp)
            .or_insert(annotation.turn_id);
    }
    for message in &state.messages {
        if current.is_empty() {
            current_turn = None;
        }
        let message_turn = annotations.get(&message.timestamp).copied();
        if !current.is_empty()
            && current_turn
                .zip(message_turn)
                .is_some_and(|(current, next)| current != next)
        {
            finish_activity_group(&mut current, &mut groups);
            current_turn = None;
        }
        match message.role {
            Role::User => {
                finish_activity_group(&mut current, &mut groups);
                current_turn = None;
            }
            Role::Assistant => {
                if !message.content.trim().is_empty()
                    || message
                        .thinking
                        .as_deref()
                        .is_some_and(|thinking| !thinking.trim().is_empty())
                {
                    finish_activity_group(&mut current, &mut groups);
                    current_turn = None;
                }
                current_turn = current_turn.or(message_turn);
                if let Some(calls) = &message.tool_calls {
                    for call in calls {
                        collect_activity_call(
                            ActivityCallRef::Complete(call),
                            &mut current,
                            &mut groups,
                        );
                    }
                }
            }
            Role::System | Role::Tool => {}
        }
    }
    if let Some(streaming) = &state.streaming {
        if !current.is_empty()
            && current_turn
                .zip(state.active_agent_turn_id)
                .is_some_and(|(current, next)| current != next)
        {
            finish_activity_group(&mut current, &mut groups);
        }
        if !streaming.content.trim().is_empty()
            || streaming
                .thinking
                .as_deref()
                .is_some_and(|thinking| !thinking.trim().is_empty())
        {
            finish_activity_group(&mut current, &mut groups);
        }
        for call in &streaming.tool_calls {
            collect_activity_call(ActivityCallRef::Partial(call), &mut current, &mut groups);
        }
    }
    finish_activity_group(&mut current, &mut groups);
    groups
}

fn activity_group_starting_with<'a>(
    groups: &'a [ActivityGroup<'a>],
    id: &str,
) -> Option<&'a ActivityGroup<'a>> {
    groups.iter().find(|group| {
        matches!(
            &group.key,
            TranscriptBlockKey::ToolCalls { first_call_id } if first_call_id == id
        )
    })
}

fn activity_call_belongs_to_group(groups: &[ActivityGroup<'_>], id: &str) -> bool {
    groups.iter().any(|group| {
        group
            .calls
            .iter()
            .any(|call| call.id().is_some_and(|call_id| call_id == id))
    })
}

fn finish_exploration_group<'a>(
    current: &mut Vec<ExplorationCallRef<'a>>,
    groups: &mut Vec<ExplorationGroup<'a>>,
) {
    let Some(first_id) = current.first().and_then(|call| call.id()) else {
        current.clear();
        return;
    };
    if let Some(key) = TranscriptBlockKey::tool_calls(first_id) {
        groups.push(ExplorationGroup {
            key,
            calls: std::mem::take(current),
        });
    } else {
        current.clear();
    }
}

fn collect_exploration_call<'a>(
    call: ExplorationCallRef<'a>,
    current: &mut Vec<ExplorationCallRef<'a>>,
    groups: &mut Vec<ExplorationGroup<'a>>,
) {
    if call.id().is_some_and(|id| !id.is_empty()) && call.name().is_some_and(is_exploration_tool) {
        current.push(call);
    } else {
        finish_exploration_group(current, groups);
    }
}

pub(crate) fn exploration_groups(state: &AppState) -> Vec<ExplorationGroup<'_>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();
    for message in &state.messages {
        match message.role {
            Role::User => finish_exploration_group(&mut current, &mut groups),
            Role::Assistant => {
                if !message.content.trim().is_empty()
                    || message
                        .thinking
                        .as_deref()
                        .is_some_and(|thinking| !thinking.trim().is_empty())
                {
                    finish_exploration_group(&mut current, &mut groups);
                }
                if let Some(calls) = &message.tool_calls {
                    for call in calls {
                        collect_exploration_call(
                            ExplorationCallRef::Complete(call),
                            &mut current,
                            &mut groups,
                        );
                    }
                }
            }
            Role::System | Role::Tool => {}
        }
    }
    if let Some(streaming) = &state.streaming {
        if !streaming.content.trim().is_empty()
            || streaming
                .thinking
                .as_deref()
                .is_some_and(|thinking| !thinking.trim().is_empty())
        {
            finish_exploration_group(&mut current, &mut groups);
        }
        for call in &streaming.tool_calls {
            collect_exploration_call(ExplorationCallRef::Partial(call), &mut current, &mut groups);
        }
    }
    finish_exploration_group(&mut current, &mut groups);
    groups
}

fn group_starting_with<'a>(
    groups: &'a [ExplorationGroup<'a>],
    id: &str,
) -> Option<&'a ExplorationGroup<'a>> {
    groups.iter().find(|group| {
        matches!(
            &group.key,
            TranscriptBlockKey::ToolCalls { first_call_id } if first_call_id == id
        )
    })
}

fn ordinary_tool_block_key(call: &ToolCall) -> Option<TranscriptBlockKey> {
    let output = call.output.as_deref()?;
    let safe_output = crate::terminal_text::sanitize_terminal_text(output);
    if (call.name == "send_message" && safe_output.starts_with("[SEND_MESSAGE] "))
        || safe_output.starts_with("@@ ")
    {
        None
    } else {
        TranscriptBlockKey::tool_calls(&call.id)
    }
}

fn identical_spawn_failure_run(calls: &[ToolCall], start: usize) -> Option<&[ToolCall]> {
    let first = calls.get(start)?;
    if first.name != "spawn_agent" || first.status != ToolStatus::Error {
        return None;
    }
    let output = first.output.as_deref()?;
    let length = calls[start..]
        .iter()
        .take_while(|call| {
            call.name == "spawn_agent"
                && call.status == ToolStatus::Error
                && call.output.as_deref() == Some(output)
        })
        .count();
    (length > 1).then(|| &calls[start..start + length])
}

fn spawn_failure_task_summary(call: &ToolCall) -> String {
    let safe_arguments = crate::terminal_text::sanitize_terminal_text(&call.arguments);
    serde_json::from_str::<serde_json::Value>(&safe_arguments)
        .ok()
        .and_then(|value| value.get("task")?.as_str().map(str::to_string))
        .filter(|task| !task.is_empty())
        .map_or_else(|| truncate(&safe_arguments, 72), |task| truncate(&task, 72))
}

pub(crate) fn latest_expandable_block_key(state: &AppState) -> Option<TranscriptBlockKey> {
    let groups = exploration_groups(state);
    let activity = activity_groups(state);
    let mut latest = None;
    for message in &state.messages {
        if let Some(calls) = &message.tool_calls {
            let mut index = 0;
            while index < calls.len() {
                if let Some(group) = identical_spawn_failure_run(calls, index) {
                    latest = TranscriptBlockKey::tool_calls(&group[0].id);
                    index += group.len();
                    continue;
                }
                let call = &calls[index];
                if let Some(group) = activity_group_starting_with(&activity, &call.id) {
                    latest = Some(group.key.clone());
                } else if activity_call_belongs_to_group(&activity, &call.id) {
                    // The group's stable key is owned by its first call.
                } else if let Some(group) = group_starting_with(&groups, &call.id) {
                    latest = Some(group.key.clone());
                } else if !is_exploration_tool(&call.name) {
                    if let Some(key) = ordinary_tool_block_key(call) {
                        latest = Some(key);
                    }
                }
                index += 1;
            }
        }
    }
    if let Some(streaming) = &state.streaming {
        for call in &streaming.tool_calls {
            if let Some(id) = call.id.as_deref() {
                if let Some(group) = activity_group_starting_with(&activity, id) {
                    latest = Some(group.key.clone());
                } else if activity_call_belongs_to_group(&activity, id) {
                    // The group's stable key is owned by its first call.
                } else if let Some(group) = group_starting_with(&groups, id) {
                    latest = Some(group.key.clone());
                }
            }
        }
    }
    latest
}

fn exploration_action_summary(call: ExplorationCallRef<'_>) -> String {
    let name = crate::terminal_text::sanitize_terminal_text(call.name().unwrap_or("tool"));
    let arguments = crate::terminal_text::sanitize_terminal_text(call.arguments());
    let details = serde_json::from_str::<serde_json::Value>(&arguments)
        .ok()
        .and_then(|value| {
            let object = value.as_object()?;
            let mut selected = Vec::new();
            for field in ["path", "relative_path", "query", "pattern", "url", "server"] {
                if let Some(value) = object.get(field).and_then(serde_json::Value::as_str) {
                    if !value.is_empty() && !selected.contains(&value) {
                        selected.push(value);
                    }
                }
            }
            (!selected.is_empty()).then(|| selected.join(" · "))
        })
        .unwrap_or(arguments);
    if details.is_empty() || details == "{}" {
        name
    } else {
        format!("{name} {}", truncate(&details, 72))
    }
}

fn render_exploration_group(
    lines: &mut Vec<Line<'static>>,
    group: &ExplorationGroup<'_>,
    width: u16,
    expanded: bool,
    theme: &ThemePalette,
) {
    let (label, icon, style) = if group.calls.iter().any(|call| call.is_active()) {
        ("Exploring…", "⟳", Style::default().fg(theme.state_working))
    } else if group.calls.iter().any(|call| call.is_error()) {
        (
            "Explored with errors",
            "✗",
            Style::default().fg(theme.error),
        )
    } else {
        ("Explored", "✓", Style::default().fg(theme.success))
    };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("{icon} {label}"),
            style.add_modifier(Modifier::BOLD),
        ),
    ]));
    if expanded {
        for call in &group.calls {
            match call {
                ExplorationCallRef::Complete(call) => {
                    render_tool_call(lines, call, width, true, None, theme)
                }
                ExplorationCallRef::Partial(call) => render_partial_tool_call(lines, call, theme),
            }
        }
    } else {
        for call in group.calls.iter().take(12) {
            lines.push(Line::from(Span::styled(
                format!("    {}", exploration_action_summary(*call)),
                Style::default().fg(theme.muted),
            )));
        }
        if group.calls.len() > 12 {
            lines.push(Line::from(Span::styled(
                format!("    +{} more", group.calls.len() - 12),
                Style::default()
                    .fg(theme.muted)
                    .add_modifier(Modifier::ITALIC),
            )));
        }
    }
}

#[derive(Debug, Clone)]
struct ActivityHeaderLine {
    line_index: usize,
    key: TranscriptBlockKey,
    left: u16,
    width: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivityFileKind {
    Created,
    Edited,
    Deleted,
    Changed,
    CreatedDirectory,
}

fn activity_group_summary(group: &ActivityGroup<'_>) -> String {
    let file_calls = group
        .calls
        .iter()
        .filter(|call| call.kind() == Some(crate::agent::registry::ToolActivityKind::File));
    let files = file_calls.clone().count();
    let file_active = file_calls.clone().any(|call| call.is_active());
    let command_calls = group
        .calls
        .iter()
        .filter(|call| call.kind() == Some(crate::agent::registry::ToolActivityKind::Command));
    let commands = command_calls.clone().count();
    let command_active = command_calls.clone().any(|call| call.is_active());
    let active = group.calls.iter().any(|call| call.is_active());
    let error = group.calls.iter().any(|call| call.is_error());
    let file_phrase = (files > 0).then(|| {
        if file_active {
            if files == 1 {
                "Editing a file".to_string()
            } else {
                format!("Editing {files} files")
            }
        } else if files == 1 {
            "Edited 1 file".to_string()
        } else {
            format!("Edited {files} files")
        }
    });
    let command_phrase = (commands > 0).then(|| {
        if command_active {
            if commands == 1 {
                "running a command".to_string()
            } else {
                format!("running {commands} commands")
            }
        } else if commands == 1 {
            "ran a command".to_string()
        } else {
            format!("ran {commands} commands")
        }
    });
    let mut summary = match (file_phrase, command_phrase) {
        (Some(files), Some(commands)) => format!("{files}, {commands}"),
        (Some(files), None) => files,
        (None, Some(commands)) => {
            let mut characters = commands.chars();
            characters
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
                .unwrap_or_default()
        }
        (None, None) => "Activity".into(),
    };
    if error {
        summary.push_str(" with errors");
    }
    if active {
        summary.push('…');
    }
    summary
}

fn activity_header_style(group: &ActivityGroup<'_>, palette: ThemePalette) -> Style {
    let color = if group.calls.iter().any(|call| call.is_error()) {
        palette.error
    } else if group.calls.iter().any(|call| call.is_active()) {
        palette.warning
    } else {
        palette.accent
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn activity_file_path_from_arguments(arguments: &str) -> Option<std::path::PathBuf> {
    serde_json::from_str::<serde_json::Value>(arguments)
        .ok()?
        .get("path")?
        .as_str()
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from)
}

fn restored_activity_file_kind(call: &ToolCall) -> ActivityFileKind {
    match call.name.as_str() {
        "edit_file" | "hashline_edit" | "apply_patch" => ActivityFileKind::Edited,
        "delete_file" => ActivityFileKind::Deleted,
        "create_directory" => ActivityFileKind::CreatedDirectory,
        "write_file" => {
            let output = call
                .output
                .as_deref()
                .map(crate::terminal_text::sanitize_terminal_text)
                .unwrap_or_default();
            if output.contains("(Created,") {
                ActivityFileKind::Created
            } else if output.contains("(Modified,") {
                ActivityFileKind::Edited
            } else {
                ActivityFileKind::Changed
            }
        }
        _ => ActivityFileKind::Changed,
    }
}

fn activity_file_details(
    state: &AppState,
    call: ActivityCallRef<'_>,
) -> (ActivityFileKind, std::path::PathBuf) {
    match call {
        ActivityCallRef::Complete(call) => {
            if let Some(presentation) = state.file_activity_presentations.get(&call.id) {
                let kind = match presentation.kind {
                    crate::app::FileActivityKind::Created => ActivityFileKind::Created,
                    crate::app::FileActivityKind::Edited => ActivityFileKind::Edited,
                    crate::app::FileActivityKind::Deleted => ActivityFileKind::Deleted,
                    crate::app::FileActivityKind::Changed => ActivityFileKind::Changed,
                };
                return (kind, presentation.path.clone());
            }
            (
                restored_activity_file_kind(call),
                activity_file_path_from_arguments(&call.arguments).unwrap_or_default(),
            )
        }
        ActivityCallRef::Partial(call) => {
            let kind = match call.name.as_deref() {
                Some("edit_file" | "hashline_edit" | "apply_patch") => ActivityFileKind::Edited,
                Some("delete_file") => ActivityFileKind::Deleted,
                Some("create_directory") => ActivityFileKind::CreatedDirectory,
                _ => ActivityFileKind::Changed,
            };
            (
                kind,
                activity_file_path_from_arguments(&call.arguments).unwrap_or_default(),
            )
        }
    }
}

fn activity_display_path(state: &AppState, path: &std::path::Path) -> String {
    let displayed = if path.is_absolute() {
        path.strip_prefix(std::path::Path::new(&state.cwd))
            .ok()
            .filter(|relative| !relative.as_os_str().is_empty())
            .unwrap_or(path)
    } else {
        path
    };
    let text = displayed.to_string_lossy();
    let text = if text.is_empty() {
        "unknown path"
    } else {
        &text
    };
    crate::terminal_text::sanitize_terminal_text(text)
}

fn activity_file_verb(kind: ActivityFileKind, active: bool, error: bool) -> &'static str {
    match (error, active, kind) {
        (true, _, ActivityFileKind::Created) => "Failed to create",
        (true, _, ActivityFileKind::Edited) => "Failed to edit",
        (true, _, ActivityFileKind::Deleted) => "Failed to delete",
        (true, _, ActivityFileKind::CreatedDirectory) => "Failed to create directory",
        (true, _, ActivityFileKind::Changed) => "Failed to change",
        (false, true, ActivityFileKind::Created) => "Creating",
        (false, true, ActivityFileKind::Edited) => "Editing",
        (false, true, ActivityFileKind::Deleted) => "Deleting",
        (false, true, ActivityFileKind::CreatedDirectory) => "Creating directory",
        (false, true, ActivityFileKind::Changed) => "Changing",
        (false, false, ActivityFileKind::Created) => "Created",
        (false, false, ActivityFileKind::Edited) => "Edited",
        (false, false, ActivityFileKind::Deleted) => "Deleted",
        (false, false, ActivityFileKind::CreatedDirectory) => "Created directory",
        (false, false, ActivityFileKind::Changed) => "Changed",
    }
}

fn push_activity_file_rows(
    lines: &mut Vec<Line<'static>>,
    state: &AppState,
    call: ActivityCallRef<'_>,
    width: u16,
) {
    let (kind, path) = activity_file_details(state, call);
    let verb = activity_file_verb(kind, call.is_active(), call.is_error());
    let path = activity_display_path(state, &path);
    let prefix = format!("    ✎ {verb} ");
    let prefix_width = unicode_width::UnicodeWidthStr::width(prefix.as_str());
    let available = usize::from(width).saturating_sub(prefix_width);
    if available == 0 {
        lines.push(Line::from(Span::styled(
            crate::tui::selection::truncate_cells(&format!("{prefix}{path}"), usize::from(width)),
            Style::default().fg(Color::DarkGray),
        )));
        return;
    }
    for (index, fragment) in crate::tui::output_layout::wrap_cells(&path, available)
        .into_iter()
        .enumerate()
    {
        lines.push(Line::from(vec![
            Span::styled(
                if index == 0 {
                    prefix.clone()
                } else {
                    " ".repeat(prefix_width)
                },
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                fragment,
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::UNDERLINED),
            ),
        ]));
    }
}

fn activity_command_text(arguments: &str) -> String {
    let command = serde_json::from_str::<serde_json::Value>(arguments)
        .ok()
        .and_then(|value| value.get("command")?.as_str().map(str::to_string))
        .filter(|command| !command.is_empty())
        .unwrap_or_else(|| arguments.to_string());
    crate::terminal_text::sanitize_terminal_text(&command)
}

fn push_activity_command_rows(
    lines: &mut Vec<Line<'static>>,
    call: ActivityCallRef<'_>,
    width: u16,
) {
    let (arguments, active, error) = match call {
        ActivityCallRef::Complete(call) => (
            &call.arguments,
            matches!(call.status, ToolStatus::Pending | ToolStatus::Running),
            call.status == ToolStatus::Error,
        ),
        ActivityCallRef::Partial(call) => (&call.arguments, true, false),
    };
    let label = if error {
        "Command failed"
    } else if active {
        "Running command…"
    } else {
        "Ran command"
    };
    lines.push(Line::from(Span::styled(
        crate::tui::selection::truncate_cells(&format!("    ▣ {label}"), usize::from(width)),
        Style::default().fg(Color::DarkGray),
    )));
    let command = activity_command_text(arguments);
    let indent = "      ";
    let available = usize::from(width).saturating_sub(indent.len()).max(1);
    for fragment in crate::tui::output_layout::wrap_cells(&command, available) {
        lines.push(Line::from(vec![
            Span::raw(indent),
            Span::styled(fragment, Style::default().fg(Color::Gray)),
        ]));
    }
}

fn render_activity_group(
    lines: &mut Vec<Line<'static>>,
    headers: &mut Vec<ActivityHeaderLine>,
    state: &AppState,
    group: &ActivityGroup<'_>,
    width: u16,
    expanded: bool,
) {
    let style = activity_header_style(group, palette(state));
    let icon = if group.calls.iter().any(|call| call.is_error()) {
        "✗"
    } else if group.calls.iter().any(|call| call.is_active()) {
        "⟳"
    } else if group
        .calls
        .iter()
        .any(|call| call.kind() == Some(crate::agent::registry::ToolActivityKind::File))
    {
        "✎"
    } else {
        "▣"
    };
    let chevron = if expanded { '▾' } else { '▸' };
    let fixed = unicode_width::UnicodeWidthStr::width(format!("  [ {icon}  {chevron} ]").as_str());
    let summary = crate::tui::selection::truncate_cells(
        &activity_group_summary(group),
        usize::from(width).saturating_sub(fixed),
    );
    let header = format!("[ {icon} {summary} {chevron} ]");
    let header_width =
        unicode_width::UnicodeWidthStr::width(header.as_str()).min(usize::from(u16::MAX)) as u16;
    headers.push(ActivityHeaderLine {
        line_index: lines.len(),
        key: group.key.clone(),
        left: 2,
        width: header_width,
    });
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(header, style),
    ]));
    if !expanded {
        return;
    }
    for call in &group.calls {
        match call.kind() {
            Some(crate::agent::registry::ToolActivityKind::File) => {
                push_activity_file_rows(lines, state, *call, width);
            }
            Some(crate::agent::registry::ToolActivityKind::Command) => {
                push_activity_command_rows(lines, *call, width);
            }
            None => continue,
        }
        if let ActivityCallRef::Complete(call) = call {
            if let Some(output) = call.output.as_deref() {
                render_tool_output(
                    lines,
                    &call.name,
                    output,
                    width,
                    true,
                    state.command_presentations.get(&call.id),
                    &palette(state),
                );
            }
        }
    }
}

fn render_partial_tool_call(
    lines: &mut Vec<Line<'static>>,
    call: &PartialToolCall,
    theme: &ThemePalette,
) {
    let Some(name) = call.name.as_deref() else {
        return;
    };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("⟳ ", Style::default().fg(theme.state_working)),
        Span::styled(
            crate::terminal_text::sanitize_terminal_text(name),
            Style::default().fg(theme.muted),
        ),
        Span::styled(
            format!(
                " {}",
                truncate(
                    &crate::terminal_text::sanitize_terminal_text(&call.arguments),
                    60,
                )
            ),
            Style::default().fg(theme.muted),
        ),
    ]));
}

fn render_tool_call(
    lines: &mut Vec<Line<'static>>,
    call: &ToolCall,
    area_width: u16,
    expanded: bool,
    semantic: Option<&crate::command_output::SemanticPresentation>,
    theme: &ThemePalette,
) {
    if matches!(
        call.name.as_str(),
        "propose_plan" | "edit_plan" | "configure_agent_graph"
    ) && call.status == ToolStatus::Done
        && !expanded
    {
        lines.push(Line::from(Span::styled(
            if call.name == "configure_agent_graph" {
                "  ✓ Task graph configured"
            } else {
                "  ✓ Plan updated"
            },
            Style::default().fg(theme.accent),
        )));
        return;
    }
    let (icon, style) = match call.status {
        ToolStatus::Done => ("✓", Style::default().fg(theme.success)),
        ToolStatus::Error => ("✗", Style::default().fg(theme.error)),
        ToolStatus::Running => ("⟳", Style::default().fg(theme.state_working)),
        ToolStatus::Pending => ("⋯", Style::default().fg(theme.muted)),
    };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{icon} "), style),
        Span::styled(
            crate::terminal_text::sanitize_terminal_text(&call.name),
            Style::default()
                .fg(theme.muted)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                " {}",
                truncate(
                    &crate::terminal_text::sanitize_terminal_text(&call.arguments),
                    60,
                )
            ),
            Style::default().fg(theme.muted),
        ),
    ]));
    let Some(output) = &call.output else {
        return;
    };
    render_tool_output(
        lines, &call.name, output, area_width, expanded, semantic, theme,
    );
}

fn render_identical_spawn_failure_group(
    lines: &mut Vec<Line<'static>>,
    calls: &[ToolCall],
    area_width: u16,
    expanded: bool,
    theme: &ThemePalette,
) {
    let Some(first) = calls.first() else {
        return;
    };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("✗ ", Style::default().fg(theme.error)),
        Span::styled("spawn_agent", Style::default().fg(theme.accent)),
        Span::styled(
            format!(
                " ×{} identical failures{}",
                calls.len(),
                if expanded { "" } else { " · ctrl+o details" }
            ),
            Style::default().fg(theme.muted),
        ),
    ]));
    if expanded {
        for (index, call) in calls.iter().enumerate() {
            lines.push(Line::from(Span::styled(
                format!("    {}. {}", index + 1, spawn_failure_task_summary(call)),
                Style::default().fg(theme.muted),
            )));
        }
    }
    if let Some(output) = first.output.as_deref() {
        render_tool_output(
            lines,
            &first.name,
            output,
            area_width,
            expanded,
            None,
            theme,
        );
    }
}

fn render_tool_output(
    lines: &mut Vec<Line<'static>>,
    tool_name: &str,
    output: &str,
    area_width: u16,
    expanded: bool,
    semantic: Option<&crate::command_output::SemanticPresentation>,
    theme: &ThemePalette,
) {
    let display_output = if expanded {
        semantic
            .map(|presentation| presentation.raw.as_str())
            .filter(|raw| !raw.is_empty())
            .unwrap_or(output)
    } else {
        semantic.map_or(output, |presentation| presentation.collapsed.as_str())
    };
    let safe_output = crate::terminal_text::sanitize_terminal_text(display_output);
    if tool_name == "send_message" && safe_output.starts_with("[SEND_MESSAGE] ") {
        lines.push(Line::from(Span::styled(
            format!("  … {}", &safe_output[15..]),
            Style::default()
                .fg(theme.muted)
                .add_modifier(Modifier::ITALIC),
        )));
    } else if safe_output.starts_with("@@ ") {
        render_diff_lines(&safe_output, area_width, lines, theme);
        lines.push(Line::from(""));
    } else {
        let width = area_width.saturating_sub(8) as usize;
        if width < 2 {
            lines.push(Line::from(Span::styled(
                crate::tui::selection::truncate_cells(
                    &safe_output.replace(['\n', '\t'], " "),
                    area_width as usize,
                ),
                Style::default().fg(theme.muted).add_modifier(Modifier::DIM),
            )));
            return;
        }
        let content_width = width.saturating_sub(2);
        let layout =
            crate::tui::output_layout::layout_tool_output(&safe_output, content_width, expanded);
        let border = "─".repeat(width);

        lines.push(Line::from(Span::styled(
            format!("    ┌{border}┐"),
            Style::default().fg(theme.border),
        )));
        for row in &layout.rows {
            push_tool_output_card_row(lines, row, content_width);
        }
        if !expanded && layout.omitted_source_lines > 0 {
            push_tool_output_marker(
                lines,
                &format!(
                    "… ({} more source lines) ctrl+o to expand",
                    layout.omitted_source_lines
                ),
                content_width,
            );
        } else if expanded && layout.total_source_lines > 20 {
            push_tool_output_marker(
                lines,
                &format!(
                    "({} source lines total) ctrl+o to collapse",
                    layout.total_source_lines
                ),
                content_width,
            );
        }
        lines.push(Line::from(Span::styled(
            format!("    └{border}┘"),
            Style::default().fg(theme.border),
        )));
    }
}

fn push_tool_output_card_row(
    lines: &mut Vec<Line<'static>>,
    row: &crate::tui::output_layout::ToolOutputRow,
    content_width: usize,
) {
    let used = unicode_width::UnicodeWidthStr::width(row.text.as_str());
    let padding = " ".repeat(content_width.saturating_sub(used));
    let style = match row.kind {
        crate::tui::output_layout::ToolOutputRowKind::Plain
        | crate::tui::output_layout::ToolOutputRowKind::TableRow => {
            Style::default().fg(Color::Gray).add_modifier(Modifier::DIM)
        }
        crate::tui::output_layout::ToolOutputRowKind::TableHeader => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        crate::tui::output_layout::ToolOutputRowKind::TableDivider => {
            Style::default().fg(Color::DarkGray)
        }
    };
    lines.push(Line::from(vec![
        Span::styled("    │ ", Style::default().fg(Color::DarkGray)),
        Span::styled(row.text.clone(), style),
        Span::raw(padding),
        Span::styled(" │", Style::default().fg(Color::DarkGray)),
    ]));
}

fn push_tool_output_marker(lines: &mut Vec<Line<'static>>, marker: &str, content_width: usize) {
    let marker = crate::tui::selection::truncate_cells(marker, content_width);
    let padding = " ".repeat(
        content_width.saturating_sub(unicode_width::UnicodeWidthStr::width(marker.as_str())),
    );
    lines.push(Line::from(vec![
        Span::styled("    │ ", Style::default().fg(Color::DarkGray)),
        Span::styled(marker, Style::default().fg(Color::DarkGray)),
        Span::raw(padding),
        Span::styled(" │", Style::default().fg(Color::DarkGray)),
    ]));
}

#[derive(Debug, Clone)]
struct ConversationContent {
    lines: Vec<Line<'static>>,
    activity_headers: Vec<ActivityHeaderLine>,
    user_prompt_cards: Vec<UserPromptCardSpan>,
    links: Vec<crate::markdown::MarkdownLink>,
}

fn conversation_content(
    state: &AppState,
    width: u16,
    include_transient: bool,
    group_activity: bool,
) -> ConversationContent {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut activity_headers = Vec::new();
    let mut user_prompt_cards = Vec::new();
    let mut links = Vec::new();
    let exploration_groups = exploration_groups(state);
    let activity_groups = if group_activity {
        activity_groups(state)
    } else {
        Vec::new()
    };
    let mut activity_starts = std::collections::HashMap::new();
    for group in &activity_groups {
        if let Some(id) = group.calls.first().and_then(|call| call.id()) {
            activity_starts.entry(id).or_insert(group);
        }
    }
    let activity_members: std::collections::HashSet<_> = activity_groups
        .iter()
        .flat_map(|group| group.calls.iter().filter_map(|call| call.id()))
        .collect();
    let mut exploration_starts = std::collections::HashMap::new();
    for group in &exploration_groups {
        if let Some(id) = group.calls.first().and_then(|call| call.id()) {
            exploration_starts.entry(id).or_insert(group);
        }
    }
    let exploration_members: std::collections::HashSet<_> = exploration_groups
        .iter()
        .flat_map(|group| group.calls.iter().filter_map(|call| call.id()))
        .collect();
    let mut annotations = std::collections::HashMap::new();
    for annotation in &state.transcript_annotations {
        annotations
            .entry(annotation.message_timestamp)
            .or_insert(annotation);
    }
    let mut last_turn_message = std::collections::HashMap::new();
    for (index, message) in state.messages.iter().enumerate() {
        if let Some(annotation) = annotations.get(&message.timestamp) {
            last_turn_message.insert(annotation.turn_id, index);
        }
    }

    let history_start = state
        .session_history
        .as_ref()
        .map_or(0, |history| history.start_index);
    if let Some(history) = state.session_history.as_ref().filter(|_| include_transient) {
        let label = if history.busy {
            format!(
                "* Loading older history · {} of {} messages loaded",
                state.messages.len(),
                history.total_messages
            )
        } else if history.is_complete() {
            format!("* Full saved history · {} messages", history.total_messages)
        } else {
            format!(
                "* Recent history · {} of {} messages loaded · PageUp loads older",
                state.messages.len(),
                history.total_messages
            )
        };
        lines.push(Line::from(Span::styled(
            label,
            Style::default()
                .fg(if history.error.is_some() {
                    Color::Red
                } else {
                    Color::DarkGray
                })
                .add_modifier(Modifier::ITALIC),
        )));
        if let Some(error) = &history.error {
            lines.push(Line::from(Span::styled(
                format!(
                    "* Older history unavailable: {}",
                    crate::terminal_text::sanitize_terminal_text(error)
                ),
                Style::default().fg(Color::Red),
            )));
        }
        lines.push(Line::from(""));
    }

    for (local_idx, msg) in state.messages.iter().enumerate() {
        let msg_idx = history_start.saturating_add(local_idx);
        if state
            .context_checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.through_message_count == msg_idx)
        {
            render_checkpoint_marker(&mut lines, msg_idx);
        }
        if let Some(provenance) = state
            .fallback_provenance
            .iter()
            .find(|entry| entry.user_message_index.saturating_add(1) == msg_idx)
        {
            render_fallback_marker(&mut lines, provenance);
        }
        if state.show_transcript_timestamps && matches!(msg.role, Role::User | Role::Assistant) {
            lines.push(Line::from(Span::styled(
                format!("[{} UTC]", msg.timestamp.format("%Y-%m-%d %H:%M:%S")),
                Style::default().fg(Color::DarkGray),
            )));
        }
        match msg.role {
            Role::User => {
                let span = render_user_prompt_card(
                    &mut lines,
                    &msg.content,
                    width,
                    palette(state),
                    msg_idx,
                    state.user_prompt_scroll.get(&msg_idx).copied().unwrap_or(0),
                );
                if span.height > 0 {
                    user_prompt_cards.push(span);
                }
            }
            Role::Assistant => {
                let annotation = annotations.get(&msg.timestamp).copied();
                if let Some(thinking) = msg.thinking.as_deref() {
                    let duration_ms =
                        annotation.and_then(|annotation| annotation.reasoning_duration_ms);
                    render_reasoning_block(
                        &mut lines,
                        thinking,
                        state.show_thinking,
                        duration_ms,
                        width,
                        palette(state),
                    );
                }
                if !msg.content.is_empty() {
                    let rendered = render_safe_markdown_document(
                        &msg.content,
                        width.saturating_sub(2),
                        state.show_line_numbers,
                        palette(state),
                    );
                    links.extend(rendered.links);
                    lines.extend(rendered.lines);
                }
                if let Some(ref tcs) = msg.tool_calls {
                    let mut index = 0;
                    while index < tcs.len() {
                        if let Some(group) = identical_spawn_failure_run(tcs, index) {
                            let expanded = TranscriptBlockKey::tool_calls(&group[0].id)
                                .is_some_and(|key| state.expanded_blocks.contains(&key));
                            render_identical_spawn_failure_group(
                                &mut lines,
                                group,
                                width,
                                expanded,
                                &palette(state),
                            );
                            index += group.len();
                            continue;
                        }
                        let tc = &tcs[index];
                        if let Some(group) = activity_starts.get(tc.id.as_str()) {
                            let expanded = state.expanded_blocks.contains(&group.key);
                            render_activity_group(
                                &mut lines,
                                &mut activity_headers,
                                state,
                                group,
                                width,
                                expanded,
                            );
                        } else if activity_members.contains(tc.id.as_str()) {
                            // Rendered by the group's first call.
                        } else if is_exploration_tool(&tc.name) {
                            if let Some(group) = exploration_starts.get(tc.id.as_str()) {
                                let expanded = state.expanded_blocks.contains(&group.key);
                                render_exploration_group(
                                    &mut lines,
                                    group,
                                    width,
                                    expanded,
                                    &palette(state),
                                );
                            } else if !exploration_members.contains(tc.id.as_str()) {
                                render_tool_call(
                                    &mut lines,
                                    tc,
                                    width,
                                    false,
                                    None,
                                    &palette(state),
                                );
                            }
                        } else {
                            let expanded = ordinary_tool_block_key(tc)
                                .is_some_and(|key| state.expanded_blocks.contains(&key));
                            render_tool_call(
                                &mut lines,
                                tc,
                                width,
                                expanded,
                                state.command_presentations.get(&tc.id),
                                &palette(state),
                            );
                        }
                        index += 1;
                    }
                }
                if let Some(annotation) = annotation {
                    let later_same_turn = last_turn_message
                        .get(&annotation.turn_id)
                        .is_some_and(|index| *index > local_idx);
                    let streaming_same_turn = state.streaming.is_some()
                        && state.active_agent_turn_id == Some(annotation.turn_id);
                    if !later_same_turn && !streaming_same_turn {
                        if state.show_assistant_footers {
                            render_turn_footer(&mut lines, annotation);
                        }
                        render_agent_activity_group(&mut lines, state, annotation.turn_id, width);
                    }
                }
                lines.push(Line::from(""));
            }
            _ => {}
        }
    }
    if state
        .context_checkpoint
        .as_ref()
        .is_some_and(|checkpoint| checkpoint.through_message_count == state.messages.len())
    {
        render_checkpoint_marker(&mut lines, state.messages.len());
    }
    if let Some(provenance) = state
        .fallback_provenance
        .iter()
        .find(|entry| entry.user_message_index.saturating_add(1) == state.messages.len())
    {
        render_fallback_marker(&mut lines, provenance);
    }

    // Streaming content
    if let Some(ref streaming) = state.streaming {
        if let Some(thinking) = streaming.thinking.as_deref() {
            let duration_ms = streaming.visible_reasoning_duration().map(|duration| {
                u64::try_from(duration.as_millis())
                    .unwrap_or(u64::MAX)
                    .max(1)
            });
            render_reasoning_block(
                &mut lines,
                thinking,
                state.show_thinking,
                duration_ms,
                width,
                palette(state),
            );
        }
        if !streaming.content.is_empty() {
            let rendered = render_safe_markdown_document(
                &streaming.content,
                width.saturating_sub(2),
                state.show_line_numbers,
                palette(state),
            );
            links.extend(rendered.links);
            let mut rendered = rendered.lines;
            if include_transient {
                let line = rendered.iter_mut().rfind(|line| line.width() > 0);
                if let Some(line) = line {
                    let cursor_style = Style::default().fg(Color::DarkGray);
                    let cursor_style = if state.cursor_blink_visible {
                        cursor_style
                    } else {
                        cursor_style.add_modifier(Modifier::HIDDEN)
                    };
                    line.spans.push(Span::styled("▌", cursor_style));
                }
            }
            lines.extend(rendered);
        } else if streaming.tool_calls.is_empty() {
            lines.push(Line::from(Span::styled(
                "⟳ Thinking…",
                Style::default().fg(palette(state).muted),
            )));
        }
        for ptc in &streaming.tool_calls {
            let id = ptc.id.as_deref().unwrap_or_default();
            if let Some(group) = activity_starts.get(id) {
                let expanded = state.expanded_blocks.contains(&group.key);
                render_activity_group(
                    &mut lines,
                    &mut activity_headers,
                    state,
                    group,
                    width,
                    expanded,
                );
            } else if activity_members.contains(id) {
                // Rendered by the group's first call.
            } else if ptc.name.as_deref().is_some_and(is_exploration_tool) {
                if let Some(group) = exploration_starts.get(id) {
                    let expanded = state.expanded_blocks.contains(&group.key);
                    render_exploration_group(&mut lines, group, width, expanded, &palette(state));
                } else if !exploration_members.contains(id) {
                    render_partial_tool_call(&mut lines, ptc, &palette(state));
                }
            } else {
                render_partial_tool_call(&mut lines, ptc, &palette(state));
            }
        }
        if let Some(turn_id) = state.active_agent_turn_id {
            render_agent_activity_group(&mut lines, state, turn_id, width);
        }
    }

    append_conversation_controls(&mut lines, state, width, include_transient);
    ConversationContent {
        lines,
        activity_headers,
        user_prompt_cards,
        links,
    }
}

fn append_conversation_controls(
    lines: &mut Vec<Line<'static>>,
    state: &AppState,
    width: u16,
    include_transient: bool,
) {
    if include_transient
        && state.task_graph_manager.is_none()
        && state.plan_proposal.is_some()
        && !state.turn_lifecycle.is_active()
        && state.streaming.is_none()
    {
        render_plan_card(lines, state, width);
    }

    // Approval prompt
    if let Some(pa) = state.approval_queue.current().filter(|_| include_transient) {
        let (resolved, total) = if state.manual_resolutions.is_active() {
            state.manual_resolutions.progress()
        } else {
            state.approval_queue.progress()
        };
        let n = resolved + 1; // current item is 1-indexed
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            format!(
                "  Approve {} [{}/{}] — {}",
                if pa.tool_name == "run_command" {
                    "command"
                } else {
                    "tool"
                },
                n,
                total,
                pa.tool_name
            ),
            Style::default().fg(Color::Yellow),
        )]));
        if let Some(privilege) = state.manual_resolutions.current_privilege() {
            lines.push(Line::from(format!(
                "    Program: {}",
                serde_json::to_string(&privilege.invocation.program.to_string_lossy())
                    .unwrap_or_else(|_| "\"[invalid]\"".into())
            )));
            for (index, argument) in privilege.invocation.display_arguments() {
                lines.push(Line::from(format!(
                    "    argv[{index}]: {}",
                    serde_json::to_string(argument).unwrap_or_else(|_| "\"[invalid]\"".into())
                )));
            }
            lines.push(Line::from(format!(
                "    cwd: {}",
                serde_json::to_string(&privilege.invocation.cwd.to_string_lossy())
                    .unwrap_or_else(|_| "\"[invalid]\"".into())
            )));
            lines.push(Line::from(format!(
                "    Filesystem authority: {} · Network: {}",
                privilege.invocation.filesystem_authority_summary(),
                privilege.invocation.network_enforcement_summary()
            )));
            lines.push(Line::from(format!(
                "    Invocation digest: {}",
                privilege.invocation.digest()
            )));
            lines.push(Line::from(vec![
                Span::styled("  [y] authorize once  ", Style::default().fg(Color::Green)),
                Span::styled("[n/Esc] deny", Style::default().fg(Color::Red)),
            ]));
        } else if let Some(preview) = &pa.email_preview {
            for (label, values) in [
                ("Account", vec![preview.account_id.clone()]),
                ("To", preview.to.clone()),
                ("Cc", preview.cc.clone()),
                ("Bcc", preview.bcc.clone()),
            ] {
                lines.push(Line::from(format!(
                    "    {label}: {}",
                    if values.is_empty() {
                        "(none)".into()
                    } else {
                        values.join(", ")
                    }
                )));
            }
            lines.push(Line::from(format!("    Subject: {}", preview.subject)));
            lines.push(Line::from(format!(
                "    Body: {} bytes · sha256 {}",
                preview.body_bytes, preview.body_sha256
            )));
            lines.push(Line::from(format!(
                "    Draft sha256: {} · {} attachment(s)",
                preview.draft_sha256,
                preview.attachments.len()
            )));
            lines.push(Line::from(vec![
                Span::styled(
                    "  [y] send exactly once  ",
                    Style::default().fg(Color::Green),
                ),
                Span::styled("[n/Esc] deny", Style::default().fg(Color::Red)),
            ]));
        } else if let Some(editor) = state.approval_queue.command_editor() {
            lines.extend(inline_command_editor_lines(editor, width, &palette(state)));
        } else {
            let (reason, current_run_scope) = approval_reason_and_scope(pa);
            lines.push(Line::from(vec![
                Span::styled("    Reason: ", Style::default().fg(Color::DarkGray)),
                Span::styled(reason, Style::default().fg(Color::Yellow)),
                Span::styled(" · [g] ", Style::default().fg(Color::DarkGray)),
                Span::styled(current_run_scope, Style::default().fg(Color::Green)),
            ]));
            lines.extend(approval_argument_lines(pa));
            lines.push(Line::from(vec![
                Span::styled("  [y] run  ", Style::default().fg(Color::Green)),
                Span::styled("[n] deny  ", Style::default().fg(Color::Red)),
                Span::styled("[e] edit  ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    "[s] all uses of this tool  ",
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    "[g] allow exact for run  ",
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    "[p] always allow exact path  ",
                    Style::default().fg(Color::Blue),
                ),
                Span::styled("[x] always deny  ", Style::default().fg(Color::Red)),
                Span::styled("[a] yolo", Style::default().fg(Color::Cyan)),
            ]));
        }
        lines.push(Line::from(""));
    }
}

pub(crate) fn privacy_safe_transcript_text(state: &AppState, width: u16) -> String {
    let mut output = conversation_content(state, width.max(1), false, false)
        .lines
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>();
    while output.last().is_some_and(String::is_empty) {
        output.pop();
    }
    output.join("\n")
}

fn render_conversation(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &AppState,
    focused: bool,
) -> u16 {
    let scroll = state.transcript_cache.render(frame, area, state, focused);
    let theme = palette(state);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut frame.buffer_mut()[(x, y)];
            if cell.bg == Color::Reset {
                cell.bg = theme.surface;
            }
            if cell.fg == Color::Reset {
                cell.fg = theme.text;
            }
        }
    }
    scroll
}

pub(super) fn apply_hyperlinks(
    buffer: &mut ratatui::buffer::Buffer,
    area: Rect,
    links: &[crate::markdown::MarkdownLink],
    workspace: &crate::workspace::WorkspaceContext,
) {
    if area.is_empty() {
        return;
    }
    let mut destinations = std::collections::HashMap::new();
    for link in links {
        destinations
            .entry(link.label.as_str())
            .and_modify(|destination: &mut Option<&str>| {
                if *destination != Some(link.destination.as_str()) {
                    *destination = None;
                }
            })
            .or_insert(Some(link.destination.as_str()));
    }
    let mut positions = std::collections::HashMap::<String, Vec<(u16, u16)>>::new();
    for row in area.top()..area.bottom() {
        for column in area.left()..area.right() {
            positions
                .entry(buffer[(column, row)].symbol().to_string())
                .or_default()
                .push((column, row));
        }
    }
    for link in links {
        if destinations.get(link.label.as_str()) == Some(&None) {
            continue;
        }
        let Some(destination) =
            crate::terminal_awareness::safe_link_destination(&link.destination, workspace)
        else {
            continue;
        };
        let label = crate::terminal_text::sanitize_terminal_text(&link.label);
        if label.is_empty() || label.contains('\n') {
            continue;
        }
        let graphemes = unicode_segmentation::UnicodeSegmentation::graphemes(label.as_str(), true)
            .collect::<Vec<_>>();
        let Some(candidates) = positions.get(graphemes[0]) else {
            continue;
        };
        'positions: for &(column, row) in candidates {
            let mut cursor = (column, row);
            let mut matched = Vec::with_capacity(graphemes.len());
            let mut valid = true;
            for grapheme in &graphemes {
                if cursor.1 >= area.bottom() {
                    valid = false;
                    break;
                }
                let cell = &buffer[cursor];
                if cell.symbol() != *grapheme || cell.symbol().contains('\u{1b}') {
                    valid = false;
                    break;
                }
                matched.push(cursor);
                let width = unicode_width::UnicodeWidthStr::width(*grapheme).max(1) as u16;
                cursor.0 = cursor.0.saturating_add(width);
                while cursor.0 >= area.right() {
                    cursor.0 = area.left().saturating_add(cursor.0 - area.right());
                    cursor.1 = cursor.1.saturating_add(1);
                }
            }
            if !valid {
                continue;
            }
            for position in matched {
                let symbol = buffer[position].symbol().to_string();
                buffer[position]
                    .set_symbol(&crate::terminal_awareness::osc8_cell(&symbol, &destination));
            }
            break 'positions;
        }
    }
}

fn inline_command_editor_lines(
    editor: &crate::approval_amend::CommandEditor,
    width: u16,
    theme: &ThemePalette,
) -> Vec<Line<'static>> {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let content_width = usize::from(width.saturating_sub(4).max(1));
    let mut rows = vec![Vec::<Span<'static>>::new()];
    let mut column: usize = 0;
    let mut cursor_row = 0;
    for (offset, grapheme) in editor.text.grapheme_indices(true) {
        let cursor = offset == editor.cursor;
        if grapheme == "\n" {
            if cursor {
                rows.last_mut().unwrap().push(Span::styled(
                    " ",
                    Style::default().fg(theme.surface).bg(theme.text),
                ));
                cursor_row = rows.len() - 1;
            }
            rows.push(Vec::new());
            column = 0;
            continue;
        }
        let safe = crate::terminal_text::sanitize_terminal_text(grapheme);
        let safe = if safe == "\t" {
            "    ".to_string()
        } else {
            safe
        };
        let cells = safe.width().max(1);
        if column > 0 && column.saturating_add(cells) > content_width {
            rows.push(Vec::new());
            column = 0;
        }
        let style = if cursor {
            cursor_row = rows.len() - 1;
            Style::default().fg(theme.surface).bg(theme.text)
        } else {
            Style::default().fg(theme.text)
        };
        rows.last_mut().unwrap().push(Span::styled(safe, style));
        column = column.saturating_add(cells);
    }
    if editor.cursor == editor.text.len() {
        if column >= content_width {
            rows.push(Vec::new());
        }
        cursor_row = rows.len() - 1;
        rows.last_mut().unwrap().push(Span::styled(
            " ",
            Style::default().fg(theme.surface).bg(theme.text),
        ));
    }
    let first = cursor_row
        .saturating_sub(6)
        .min(rows.len().saturating_sub(8));
    let end = first.saturating_add(8).min(rows.len());
    let total_rows = rows.len();
    let mut lines = vec![Line::from(Span::styled(
        "    Editing command · approval required after save",
        Style::default().fg(theme.warning),
    ))];
    if first > 0 {
        lines.push(Line::from(Span::styled(
            "    … earlier lines",
            Style::default().fg(theme.muted),
        )));
    }
    for row in rows.into_iter().skip(first).take(end - first) {
        let mut spans = vec![Span::raw("    ")];
        spans.extend(row);
        lines.push(Line::from(spans));
    }
    if end < total_rows {
        lines.push(Line::from("    … later lines"));
    }
    lines.push(Line::from(Span::styled(
        if editor.saving {
            "    Checking command… · Esc cancel"
        } else {
            "    Ctrl+S save · Enter newline · Esc cancel"
        },
        Style::default().fg(theme.muted),
    )));
    if let Some(error) = &editor.error {
        lines.push(Line::from(Span::styled(
            format!(
                "    {}",
                crate::terminal_text::sanitize_terminal_text(error)
            ),
            Style::default().fg(theme.error),
        )));
    }
    lines
}

fn approval_reason_and_scope(pa: &crate::app::PendingApproval) -> (&'static str, &'static str) {
    if pa.tool_name == "run_command" {
        let compound = serde_json::from_str::<serde_json::Value>(&pa.tool_args)
            .ok()
            .and_then(|arguments| arguments["command"].as_str().map(str::to_string))
            .is_some_and(|command| {
                command
                    .chars()
                    .any(|character| matches!(character, ';' | '|' | '&' | '$' | '`'))
            });
        return if compound && pa.network_requested {
            (
                "compound shell syntax · network requested",
                "this exact command for this run",
            )
        } else if compound {
            ("compound shell syntax", "this exact command for this run")
        } else if pa.network_requested {
            (
                "local process · network requested",
                "this exact command for this run",
            )
        } else {
            ("local process execution", "this exact command for this run")
        };
    }
    if pa.network_requested {
        return (
            "network-capable tool execution",
            "this tool call for this run",
        );
    }
    if matches!(
        pa.tool_name.as_str(),
        "write_file"
            | "edit_file"
            | "hashline_edit"
            | "apply_patch"
            | "delete_file"
            | "create_directory"
    ) {
        return ("workspace write", "this file for this run");
    }
    if pa.tool_name == "activate_skill" {
        return ("instructions change", "this exact activation for this run");
    }
    (
        "tool requires consent",
        "this exact invocation for this run",
    )
}

fn approval_argument_lines(pa: &crate::app::PendingApproval) -> Vec<Line<'static>> {
    let arguments = serde_json::from_str::<serde_json::Value>(&pa.tool_args).ok();
    if let Some(preview) = arguments
        .as_ref()
        .and_then(|a| a.get("_ca_edit_preview"))
        .and_then(serde_json::Value::as_str)
    {
        let path = arguments
            .as_ref()
            .and_then(|a| a["path"].as_str())
            .unwrap_or("file");
        return crate::terminal_text::sanitize_terminal_text(&format!(
            "{path} — final edit contents:\n{preview}"
        ))
        .lines()
        .map(|line| Line::from(format!("    {line}")))
        .collect();
    }
    if pa.tool_name == "run_command" {
        let command = arguments
            .as_ref()
            .and_then(|arguments| arguments["command"].as_str());
        if let Some(command) = command {
            let safe = crate::terminal_text::sanitize_terminal_text(command);
            let mut rendered = safe
                .split('\n')
                .enumerate()
                .map(|(index, line)| {
                    Line::from(Span::styled(
                        if index == 0 {
                            format!("    Command: {line}")
                        } else {
                            format!("             {line}")
                        },
                        Style::default().fg(Color::DarkGray),
                    ))
                })
                .collect::<Vec<_>>();
            if let Some(cwd) = arguments
                .as_ref()
                .and_then(|arguments| arguments["cwd"].as_str())
            {
                rendered.push(Line::from(Span::styled(
                    format!(
                        "    cwd: {}",
                        crate::terminal_text::sanitize_terminal_text(cwd)
                    ),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            return rendered;
        }
    }

    crate::terminal_text::sanitize_terminal_text(&pa.tool_args)
        .split('\n')
        .map(|line| {
            Line::from(Span::styled(
                format!("    {line}"),
                Style::default().fg(Color::DarkGray),
            ))
        })
        .collect()
}

#[derive(Clone)]
struct ActivityDisplayRow {
    id: crate::agent::subagent::AgentId,
    status: crate::agent::subagent::AgentStatus,
    task: String,
    role: Option<String>,
    model: Option<String>,
    phase: crate::agent::subagent::AgentExecutionPhase,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    current_action: Option<String>,
    summary: Option<String>,
    updated_at: chrono::DateTime<chrono::Utc>,
    events: Vec<crate::agent::subagent::AgentEvent>,
    recent_output: String,
    live: bool,
}

fn agent_activity_rows(state: &AppState, turn_id: uuid::Uuid) -> Vec<ActivityDisplayRow> {
    let mut rows = state
        .live_agent_activity
        .iter()
        .filter(|agent| {
            matches!(
                agent.origin,
                crate::agent::subagent::AgentRunOrigin::Conversation
                    | crate::agent::subagent::AgentRunOrigin::AutomaticReview
            ) && agent.parent_turn == turn_id
        })
        .map(|agent| ActivityDisplayRow {
            id: agent.id,
            status: agent.status.clone(),
            task: crate::proactive_delegation::visible_task(&agent.task).to_string(),
            role: agent.role.clone(),
            model: agent.model.clone(),
            phase: agent.phase,
            created_at: Some(agent.created_at),
            current_action: agent.current_action.clone(),
            summary: agent.result_summary.clone(),
            updated_at: agent.updated_at,
            events: agent.events.clone(),
            recent_output: agent.recent_output.clone(),
            live: true,
        })
        .collect::<Vec<_>>();
    let live_ids = rows
        .iter()
        .map(|row| row.id)
        .collect::<std::collections::HashSet<_>>();
    rows.extend(
        state
            .transcript_agent_summaries
            .iter()
            .filter(|summary| summary.turn_id == turn_id && !live_ids.contains(&summary.agent_id))
            .map(|summary| ActivityDisplayRow {
                id: summary.agent_id,
                status: summary.status.clone(),
                task: crate::proactive_delegation::visible_task(&summary.task).to_string(),
                role: summary.role.clone(),
                model: summary.model.clone(),
                phase: crate::agent::subagent::AgentExecutionPhase::Finished,
                created_at: summary.created_at,
                current_action: None,
                summary: Some(summary.summary.clone()),
                updated_at: summary.updated_at,
                events: Vec::new(),
                recent_output: String::new(),
                live: false,
            }),
    );
    rows.sort_by_key(|row| (row.created_at.unwrap_or(row.updated_at), row.id.0));
    rows
}

fn agent_status_label(status: &crate::agent::subagent::AgentStatus) -> &'static str {
    use crate::agent::subagent::AgentStatus;
    match status {
        AgentStatus::Queued => "Queued",
        AgentStatus::Running => "Running",
        AgentStatus::WaitingApproval => "Waiting for approval",
        AgentStatus::Completed => "Completed",
        AgentStatus::Failed => "Failed",
        AgentStatus::Cancelled => "Cancelled",
        AgentStatus::TimedOut => "Timed out",
        AgentStatus::Interrupted => "Interrupted",
    }
}

fn agent_status_color(status: &crate::agent::subagent::AgentStatus, theme: &ThemePalette) -> Color {
    use crate::agent::subagent::AgentStatus;
    match status {
        AgentStatus::Completed => theme.state_completed,
        AgentStatus::Failed | AgentStatus::TimedOut => theme.state_failed,
        AgentStatus::WaitingApproval => theme.state_needs_input,
        AgentStatus::Cancelled | AgentStatus::Interrupted => theme.state_idle,
        AgentStatus::Queued | AgentStatus::Running => theme.state_working,
    }
}

#[derive(Clone, Copy)]
struct AgentWorkerIcon {
    glyph: &'static str,
    color: Color,
}

const AGENT_WORKER_ICONS: [AgentWorkerIcon; 6] = [
    AgentWorkerIcon {
        glyph: "✳",
        color: Color::Rgb(232, 185, 69),
    },
    AgentWorkerIcon {
        glyph: "◆",
        color: Color::Rgb(173, 127, 255),
    },
    AgentWorkerIcon {
        glyph: "✿",
        color: Color::Rgb(244, 107, 147),
    },
    AgentWorkerIcon {
        glyph: "⬢",
        color: Color::Rgb(86, 190, 208),
    },
    AgentWorkerIcon {
        glyph: "❖",
        color: Color::Rgb(118, 199, 127),
    },
    AgentWorkerIcon {
        glyph: "✦",
        color: Color::Rgb(239, 146, 74),
    },
];

fn agent_worker_icon(id: crate::agent::subagent::AgentId) -> AgentWorkerIcon {
    let slot = usize::from(id.0.as_bytes()[0]) % AGENT_WORKER_ICONS.len();
    AGENT_WORKER_ICONS[slot]
}

fn format_agent_elapsed(seconds: u64) -> String {
    if seconds >= 3_600 {
        format!("{}h{:02}m", seconds / 3_600, seconds % 3_600 / 60)
    } else if seconds >= 60 {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

fn render_agent_activity_group<'a>(
    lines: &mut Vec<Line<'a>>,
    state: &AppState,
    turn_id: uuid::Uuid,
    width: u16,
) {
    let rows = agent_activity_rows(state, turn_id);
    if rows.is_empty() {
        return;
    }
    let focus = state
        .agent_activity_focus
        .as_ref()
        .filter(|focus| focus.turn_id == turn_id);
    let theme = palette(state);
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            "  ▾ Subagents ",
            Style::default()
                .fg(theme.muted)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(rows.len().to_string(), Style::default().fg(theme.muted)),
    ]));
    for row in &rows {
        let selected = focus.and_then(|focus| focus.selected_agent) == Some(row.id);
        let icon = agent_worker_icon(row.id);
        let marker = if selected { "›" } else { " " };
        let role = crate::tui::selection::truncate_cells(
            &crate::terminal_text::sanitize_terminal_text(row.role.as_deref().unwrap_or("Agent")),
            16,
        );
        let phase = if row.status.terminal() {
            agent_status_label(&row.status).to_string()
        } else if row.phase == crate::agent::subagent::AgentExecutionPhase::Tool {
            row.current_action.as_deref().map_or_else(
                || "Running tool".to_string(),
                |action| {
                    format!(
                        "Running {}",
                        crate::terminal_text::sanitize_terminal_text(action)
                    )
                },
            )
        } else {
            row.phase.label().to_string()
        };
        let elapsed = row
            .created_at
            .map(|started| {
                let end = if row.status.terminal() {
                    row.updated_at
                } else {
                    state.agent_activity_now
                };
                let seconds = end.signed_duration_since(started).num_seconds().max(0) as u64;
                format_agent_elapsed(seconds)
            })
            .unwrap_or_default();
        let model = row
            .model
            .as_deref()
            .map(crate::terminal_text::sanitize_terminal_text)
            .unwrap_or_default();
        let model_budget = (width as usize / 3)
            .saturating_sub(elapsed.len() + 1)
            .min(if width < 80 { 10 } else { usize::MAX });
        let model = crate::tui::selection::truncate_cells(&model, model_budget);
        let right = [model, elapsed]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let left_width = 4 + unicode_width::UnicodeWidthStr::width(role.as_str()) + 1;
        let right_width = unicode_width::UnicodeWidthStr::width(right.as_str());
        let middle_width = (width as usize).saturating_sub(left_width + right_width + 2);
        let phase_budget = middle_width
            .saturating_sub(3)
            .saturating_sub(if row.task.is_empty() { 0 } else { 8 });
        let phase = crate::tui::selection::truncate_cells(&phase, phase_budget);
        let phase_width = unicode_width::UnicodeWidthStr::width(phase.as_str());
        let task = crate::tui::selection::truncate_cells(
            &crate::terminal_text::sanitize_terminal_text(&row.task),
            middle_width.saturating_sub(phase_width + 3),
        );
        let task = if task.is_empty() {
            String::new()
        } else {
            format!("{task} · ")
        };
        let middle_used = unicode_width::UnicodeWidthStr::width(task.as_str()) + phase_width;
        let padding =
            " ".repeat((width as usize).saturating_sub(left_width + middle_used + right_width + 1));
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker}"), Style::default().fg(theme.accent)),
            Span::styled(icon.glyph, Style::default().fg(icon.color)),
            Span::styled(
                format!(" {role} "),
                Style::default().fg(theme.accent).add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            ),
            Span::styled(task, Style::default().fg(theme.text)),
            Span::styled(
                phase,
                Style::default().fg(agent_status_color(&row.status, &theme)),
            ),
            Span::raw(padding),
            Span::styled(right, Style::default().fg(theme.muted)),
        ]));
        if row.status.terminal() {
            if let Some(summary) = row.summary.as_deref() {
                lines.push(Line::from(Span::styled(
                    format!(
                        "      ↳ {}",
                        crate::tui::selection::truncate_cells(
                            &crate::terminal_text::sanitize_terminal_text(summary),
                            width.saturating_sub(8) as usize
                        )
                    ),
                    Style::default().fg(theme.muted),
                )));
            }
        }
    }
    if let Some(focus) = focus {
        if focus.detail_open {
            if let Some(row) = focus
                .selected_agent
                .and_then(|id| rows.iter().find(|row| row.id == id))
            {
                render_agent_activity_detail(lines, row, focus, width, theme);
            }
        }
        lines.push(Line::from(Span::styled(
            if width < 80 {
                "  ↑/↓ select · Enter · m · y/n · c · Esc"
            } else {
                "  ↑/↓ select · Enter details · m message · y/n approval · c cancel · Esc close"
            },
            Style::default().fg(theme.muted),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!(
                "  {} view agents",
                state
                    .keymap
                    .primary_binding(crate::keymap::KeyAction::FocusAgentActivity)
            ),
            Style::default().fg(theme.muted),
        )));
    }
}

fn render_agent_activity_detail<'a>(
    lines: &mut Vec<Line<'a>>,
    row: &ActivityDisplayRow,
    focus: &crate::app::AgentActivityFocusState,
    width: u16,
    theme: ThemePalette,
) {
    let body_width = width.saturating_sub(10) as usize;
    let id = row.id.to_string();
    lines.push(Line::from(Span::styled(
        format!(
            "      status: {} · id: {}",
            agent_status_label(&row.status),
            &id[..8]
        ),
        Style::default().fg(theme.text),
    )));
    let mut detail = row
        .events
        .iter()
        .map(|event| format!("{}  {}", event.at.format("%H:%M:%S"), event.message))
        .collect::<Vec<_>>();
    if detail.is_empty() {
        detail.extend(row.recent_output.lines().map(str::to_string));
    }
    let visible = 8_usize;
    let max_scroll = detail.len().saturating_sub(visible);
    let scroll = usize::from(focus.detail_scroll).min(max_scroll);
    let start = max_scroll.saturating_sub(scroll);
    for line in detail.iter().skip(start).take(visible) {
        lines.push(Line::from(Span::styled(
            format!(
                "      │ {}",
                truncate(
                    &crate::terminal_text::sanitize_terminal_text(line),
                    body_width
                )
            ),
            Style::default().fg(theme.muted),
        )));
    }
    if let Some(draft) = focus.message_draft.as_deref() {
        lines.push(Line::from(vec![
            Span::styled("      message> ", Style::default().fg(theme.accent)),
            Span::styled(
                truncate(
                    &crate::terminal_text::sanitize_terminal_text(draft),
                    body_width,
                ),
                Style::default().fg(theme.text),
            ),
        ]));
    }
    if focus.cancel_confirmation {
        lines.push(Line::from(vec![
            Span::styled(
                if focus.cancel_confirm_selected {
                    "      [Cancel]  "
                } else {
                    "      > Cancel  "
                },
                Style::default().fg(theme.text),
            ),
            Span::styled(
                if focus.cancel_confirm_selected {
                    "> Confirm cancellation"
                } else {
                    "[Confirm cancellation]"
                },
                Style::default().fg(theme.error),
            ),
        ]));
    }
    if !row.live {
        lines.push(Line::from(Span::styled(
            "      saved terminal summary · controls unavailable",
            Style::default().fg(theme.muted),
        )));
    }
}

fn render_checkpoint_marker<'a>(lines: &mut Vec<Line<'a>>, covered_messages: usize) {
    lines.push(Line::from(Span::styled(
        format!("* Context checkpoint · {covered_messages} messages · /context to inspect"),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(""));
}

fn render_fallback_marker<'a>(
    lines: &mut Vec<Line<'a>>,
    provenance: &crate::fallback::TurnProviderProvenance,
) {
    let primary = crate::terminal_text::sanitize_terminal_text(&format!(
        "{}/{}",
        provenance.primary_provider, provenance.primary_model
    ));
    let fallback = crate::terminal_text::sanitize_terminal_text(&format!(
        "{}/{}",
        provenance.provider, provenance.model
    ));
    lines.push(Line::from(Span::styled(
        format!(
            "* Provider fallback · {primary} → {fallback} · {}",
            provenance.cause
        ),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::ITALIC),
    )));
    lines.push(Line::from(""));
}

fn has_active_background(state: &AppState) -> bool {
    state
        .job_list
        .iter()
        .any(|job| job.background && !job.status.is_terminal())
        || state
            .watch_list
            .iter()
            .any(|watcher| !watcher.event.state.is_terminal())
        || state
            .delegation_list
            .iter()
            .any(|delegation| !delegation.status.is_terminal())
}

pub(crate) fn awaited_managed_job(state: &AppState) -> Option<&crate::jobs::JobInfo> {
    let calls = state.messages.last()?.tool_calls.as_ref()?;
    let awaited = |job: &&crate::jobs::JobInfo| {
        !job.status.is_terminal()
            && calls.iter().any(|call| {
                call.name == "await_job"
                    && call.status == crate::app::ToolStatus::Running
                    && call.output.is_none()
                    && serde_json::from_str::<serde_json::Value>(&call.arguments)
                        .ok()
                        .and_then(|args| {
                            args.get("job_id")
                                .and_then(|value| value.as_str())
                                .map(str::to_owned)
                        })
                        .is_some_and(|id| id == job.id.to_string())
            })
    };
    if let Some(crate::app::BackgroundActivityRef::Job(id)) = state.latest_background_activity {
        if let Some(job) = state
            .job_list
            .iter()
            .filter(awaited)
            .find(|job| job.id == id)
        {
            return Some(job);
        }
    }
    calls
        .iter()
        .filter(|call| {
            call.name == "await_job"
                && call.status == crate::app::ToolStatus::Running
                && call.output.is_none()
        })
        .find_map(|call| {
            let args: serde_json::Value = serde_json::from_str(&call.arguments).ok()?;
            let id = args.get("job_id")?.as_str()?;
            state
                .job_list
                .iter()
                .find(|job| !job.status.is_terminal() && job.id.to_string() == id)
        })
}

fn awaited_job_preview(job: &crate::jobs::JobInfo) -> Vec<String> {
    let mut lines = vec![format!(
        " ◉ {} · {} · {:.0}s · /jobs",
        job.command,
        job.status.label(),
        job.elapsed.as_secs_f64()
    )];
    for (label, output) in [("stdout", &job.stdout), ("stderr", &job.stderr)] {
        let tail = output
            .split(['\n', '\r'])
            .filter(|line| !line.trim().is_empty())
            .rev()
            .take(2)
            .collect::<Vec<_>>();
        for line in tail.into_iter().rev() {
            lines.push(format!(
                "   {label}: {}",
                crate::changes::sanitize_terminal_text(line)
            ));
        }
    }
    if lines.len() == 1 {
        lines.push("   No output yet".into());
    }
    lines
}

fn background_activity_rows(state: &AppState, available: u16) -> u16 {
    if let Some(job) = awaited_managed_job(state) {
        (awaited_job_preview(job).len() as u16)
            .min(5)
            .min(available)
    } else {
        u16::from(has_active_background(state)).min(available)
    }
}

fn active_background_count(state: &AppState) -> usize {
    state
        .job_list
        .iter()
        .filter(|job| job.background && !job.status.is_terminal())
        .count()
        + state
            .watch_list
            .iter()
            .filter(|watcher| !watcher.event.state.is_terminal())
            .count()
        + state
            .delegation_list
            .iter()
            .filter(|delegation| !delegation.status.is_terminal())
            .count()
}

fn render_background_activity(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    if let Some(job) = awaited_managed_job(state) {
        let lines = awaited_job_preview(job)
            .into_iter()
            .take(area.height as usize)
            .map(|line| {
                Line::from(crate::tui::selection::truncate_cells(
                    &crate::changes::sanitize_terminal_text(&line),
                    area.width as usize,
                ))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().fg(palette(state).muted)),
            area,
        );
        return;
    }
    let count = active_background_count(state);
    if count == 0 {
        return;
    }
    let selected = match state.latest_background_activity {
        Some(crate::app::BackgroundActivityRef::Job(id)) => state
            .job_list
            .iter()
            .find(|job| job.id == id && job.background && !job.status.is_terminal())
            .map(|job| {
                let recent = job
                    .stdout
                    .lines()
                    .last()
                    .or_else(|| job.stderr.lines().last())
                    .map(crate::changes::sanitize_terminal_text)
                    .filter(|line| !line.is_empty())
                    .map_or_else(String::new, |line| format!(" · {line}"));
                format!(
                    "managed job · {} · {:.0}s{recent}",
                    job.status.label(),
                    job.elapsed.as_secs_f64()
                )
            }),
        Some(crate::app::BackgroundActivityRef::Watch(id)) => state
            .watch_list
            .iter()
            .find(|watcher| watcher.watch_id == id && !watcher.event.state.is_terminal())
            .map(watcher_activity_label),
        None => None,
    }
    .or_else(|| {
        state
            .watch_list
            .iter()
            .find(|watcher| !watcher.event.state.is_terminal())
            .map(watcher_activity_label)
    })
    .or_else(|| {
        state
            .delegation_list
            .iter()
            .find(|delegation| !delegation.status.is_terminal())
            .map(|delegation| {
                format!(
                    "codex-team delegation · {:?} · {}",
                    delegation.status,
                    delegation.action_categories.join("+")
                )
            })
    })
    .or_else(|| {
        state
            .job_list
            .iter()
            .find(|job| job.background && !job.status.is_terminal())
            .map(|job| {
                format!(
                    "managed job · {} · {:.0}s",
                    job.status.label(),
                    job.elapsed.as_secs_f64()
                )
            })
    })
    .unwrap_or_else(|| "background activity".to_string());
    let suffix = format!(" · {count} active · /jobs");
    let suffix_width = unicode_width::UnicodeWidthStr::width(suffix.as_str());
    let prefix = crate::tui::selection::truncate_cells(
        &format!(" ◉ {selected}"),
        (area.width as usize).saturating_sub(suffix_width),
    );
    let line = format!("{prefix}{suffix}");
    frame.render_widget(
        Paragraph::new(crate::changes::sanitize_terminal_text(&line))
            .style(Style::default().fg(palette(state).muted)),
        area,
    );
}

fn watcher_activity_label(watcher: &ca_watch::WatchSnapshotV1) -> String {
    let elapsed = watcher
        .event
        .observed_at
        .signed_duration_since(watcher.started_at)
        .num_seconds()
        .max(0);
    format!(
        "{} · {} PID {} · {} · {elapsed}s · {}",
        watcher.request.label,
        watcher.request.host.display_name(),
        watcher.request.pid,
        watcher.event.state,
        watcher.event.summary,
    )
}

fn render_input(frame: &mut ratatui::Frame, area: Rect, state: &AppState, focused: bool) {
    let palette = palette(state);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(palette.border));
    frame.render_widget(block, area);

    let mut inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };

    // This dedicated composer row is never replaced by a transient status.
    // Keep the urgent action first so it survives even very narrow terminals.
    let queue_binding = state
        .keymap
        .display_binding(crate::keymap::KeyAction::QueueInput);
    let controls = if state.hook_running
        || state.attachment_resolution_busy
        || state.review_in_flight
        || state.foreground_job.is_some()
        || (state.turn_lifecycle.is_active()
            && (state.interaction.is_some() || state.approval_queue.is_active()))
    {
        Some("Esc stop · Ctrl+C stop".to_string())
    } else if state.turn_lifecycle.is_active() {
        Some(format!(
            "Esc stop · Enter interrupt · {queue_binding} queue"
        ))
    } else if state.input_queue.editing_id.is_some() {
        Some(format!("Enter send now · {queue_binding} save to queue"))
    } else if !state.input_queue.is_empty() {
        Some(format!(
            "{} edit latest · /queue view all",
            state
                .keymap
                .display_binding(crate::keymap::KeyAction::EditLastQueued)
        ))
    } else {
        None
    };
    let controls_width = controls
        .as_deref()
        .map_or(0, unicode_width::UnicodeWidthStr::width);
    let metadata_budget = (area.width as usize).saturating_sub(controls_width + 6);
    let metadata =
        crate::tui::selection::truncate_cells(&composer_metadata(state), metadata_budget);
    let metadata_width = unicode_width::UnicodeWidthStr::width(metadata.as_str());
    let show_metadata = area.height >= 3 && metadata_width > 0 && metadata_budget >= 12;
    if let Some(controls) = controls.as_deref().filter(|_| area.height >= 3) {
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!(" {controls} "),
                Style::default().fg(palette.muted).bg(palette.surface),
            )),
            Rect::new(
                inner.x,
                area.bottom() - 1,
                if show_metadata {
                    inner.width.saturating_sub(metadata_width as u16 + 3)
                } else {
                    inner.width
                },
                1,
            ),
        );
    }
    if show_metadata {
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!(" {metadata} "),
                Style::default().fg(palette.muted).bg(palette.surface),
            ))
            .alignment(Alignment::Right),
            Rect::new(
                inner.right().saturating_sub(metadata_width as u16 + 2),
                area.bottom() - 1,
                metadata_width as u16 + 2,
                1,
            ),
        );
    }

    let previews: Vec<_> = state.input_queue.items().take(3).collect();
    for (index, pending) in previews.iter().enumerate() {
        let marker = if state.input_queue.editing_id == Some(pending.id) {
            "Editing queued draft"
        } else if state.restored_queue_ids.contains(&pending.id) {
            "On hold"
        } else {
            "Queued"
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{marker} · "), Style::default().fg(palette.accent)),
                Span::styled(
                    truncate(
                        &pending.text.replace('\n', " "),
                        inner.width.saturating_sub(marker.len() as u16 + 3) as usize,
                    ),
                    Style::default().fg(palette.muted),
                ),
            ])),
            Rect {
                x: inner.x,
                y: inner.y + index as u16,
                width: inner.width,
                height: 1,
            },
        );
    }
    let overflow = state.input_queue.len().saturating_sub(previews.len());
    let overflow_rows = u16::from(overflow > 0);
    if overflow > 0 {
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("+{overflow} more in /queue"),
                Style::default().fg(palette.muted),
            )),
            Rect {
                x: inner.x,
                y: inner.y + previews.len() as u16,
                width: inner.width,
                height: 1,
            },
        );
    }
    let preview_rows = previews.len() as u16 + overflow_rows;
    inner.y += preview_rows;
    inner.height = inner.height.saturating_sub(preview_rows);

    if !state.pasted_images.is_empty() || state.image_attachment_busy {
        let label = if state.image_attachment_busy {
            "[Image processing…]".to_string()
        } else {
            format!(
                "[{} image attachment{}]",
                state.pasted_images.len(),
                if state.pasted_images.len() == 1 {
                    ""
                } else {
                    "s"
                }
            )
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                label,
                Style::default()
                    .fg(palette.accent_foreground)
                    .bg(palette.accent),
            )),
            Rect {
                x: inner.x,
                y: inner.y,
                width: inner.width,
                height: 1,
            },
        );
        inner.y += 1;
        inner.height = inner.height.saturating_sub(1);
    }

    let paste_index = state
        .composer_ui
        .active_large_paste_index(state.input_cursor);
    if let Some(index) = paste_index {
        if let Some(payload) = state.composer_ui.large_paste_payload(index) {
            let body_rows = crate::tui::composer::PASTE_PREVIEW_ROWS.min(4) as u16;
            let strip_height = body_rows + 1;
            if inner.height > strip_height {
                let offset = state
                    .composer_ui
                    .paste_preview_scroll
                    .get(&index)
                    .copied()
                    .unwrap_or(0);
                let payload_lines: Vec<&str> = payload.lines().collect();
                let line_count = payload_lines.len().max(1);
                let max_top = line_count.saturating_sub(body_rows as usize);
                let top = offset.min(max_top);
                let end = (top + body_rows as usize).min(line_count);
                let hidden_above = top;
                let hidden_below = line_count.saturating_sub(end);
                let chars = payload.chars().count();
                let mut header = format!("[Pasted Content {chars} chars]");
                if hidden_above > 0 {
                    header.push_str(&format!(" · ↑{hidden_above}"));
                }
                if hidden_below > 0 {
                    header.push_str(&format!(" · ↓{hidden_below} more"));
                }
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        header,
                        Style::default()
                            .fg(palette.accent_foreground)
                            .bg(palette.accent),
                    )),
                    Rect {
                        x: inner.x,
                        y: inner.y,
                        width: inner.width,
                        height: 1,
                    },
                );
                let mut body_lines = Vec::with_capacity(body_rows as usize);
                for line in payload_lines.iter().skip(top).take(end.saturating_sub(top)) {
                    body_lines.push(Line::from(crate::tui::selection::truncate_cells(
                        line,
                        inner.width as usize,
                    )));
                }
                while body_lines.len() < body_rows as usize {
                    body_lines.push(Line::from(""));
                }
                frame.render_widget(
                    Paragraph::new(body_lines).style(Style::default().fg(palette.muted)),
                    Rect {
                        x: inner.x,
                        y: inner.y + 1,
                        width: inner.width,
                        height: body_rows,
                    },
                );
                *state.composer_ui.paste_preview_hit.borrow_mut() =
                    crate::tui::composer::PastePreviewHit {
                        x: inner.x,
                        y: inner.y,
                        width: inner.width,
                        height: strip_height,
                    };
                inner.y += strip_height;
                inner.height = inner.height.saturating_sub(strip_height);
            } else {
                *state.composer_ui.paste_preview_hit.borrow_mut() =
                    crate::tui::composer::PastePreviewHit::default();
            }
        } else {
            *state.composer_ui.paste_preview_hit.borrow_mut() =
                crate::tui::composer::PastePreviewHit::default();
        }
    } else {
        *state.composer_ui.paste_preview_hit.borrow_mut() =
            crate::tui::composer::PastePreviewHit::default();
    }

    if state.input.is_empty() {
        let placeholder = if state.turn_lifecycle.is_active() {
            "Add a follow-up…"
        } else {
            "Ask anything…"
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                composer_prompt(&palette),
                Span::styled(placeholder, Style::default().fg(palette.muted)),
            ])),
            inner,
        );
        *state.composer_ui.hit_map.borrow_mut() = crate::tui::composer::ComposerHitMap {
            text_area: Some((inner.x, inner.y, inner.width, inner.height)),
            rows: Vec::new(),
            empty_placeholder: true,
        };
    } else {
        let layout = crate::tui::composer::ComposerLayout::new(
            &state.input,
            state.input_cursor,
            inner.width as usize,
            2,
        );
        let viewport = layout.visible_window(state.composer_ui.viewport_top);
        let mut lines = Vec::with_capacity(viewport.end.saturating_sub(viewport.top));
        let mut hit_rows = Vec::with_capacity(viewport.end.saturating_sub(viewport.top));
        for (visible_index, row_index) in (viewport.top..viewport.end).enumerate() {
            let row = &layout.rows[row_index];
            let mut spans = Vec::new();
            let prefix_width = if visible_index == 0 && viewport.hidden_above > 0 {
                let marker = format!("↑{} ", viewport.hidden_above);
                let width = unicode_width::UnicodeWidthStr::width(marker.as_str()) as u16;
                spans.push(Span::styled(marker, Style::default().fg(palette.muted)));
                width
            } else if row_index == 0 {
                spans.push(composer_prompt(&palette));
                2
            } else {
                spans.push(Span::raw("  "));
                2
            };
            spans.extend(composer_row_spans(state, row.range.clone(), &palette));
            if row_index == layout.rows.len().saturating_sub(1)
                && state.input_cursor == state.input.len()
            {
                if let Some(ref ghost) = state.ghost_text {
                    spans.push(Span::styled(
                        ghost.as_str(),
                        Style::default().fg(palette.muted),
                    ));
                }
            }
            if visible_index + 1 == viewport.end.saturating_sub(viewport.top)
                && viewport.hidden_below > 0
            {
                spans.push(Span::styled(
                    format!(" ↓{}", viewport.hidden_below),
                    Style::default().fg(palette.muted),
                ));
            }
            hit_rows.push(crate::tui::composer::ComposerRowHit {
                y: inner.y.saturating_add(visible_index as u16),
                prefix_width,
                boundaries: row.boundaries.clone(),
            });
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), inner);
        *state.composer_ui.hit_map.borrow_mut() = crate::tui::composer::ComposerHitMap {
            text_area: Some((inner.x, inner.y, inner.width, inner.height)),
            rows: hit_rows,
            empty_placeholder: false,
        };
    }

    if focused && composer_has_focus(state) && state.cursor_blink_visible {
        if let Some(position) = composer_cursor_position(inner, state) {
            render_white_composer_cursor(frame, position);
        }
    }
}

fn render_white_composer_cursor(frame: &mut ratatui::Frame, position: ratatui::layout::Position) {
    frame.buffer_mut()[(position.x, position.y)]
        .set_style(Style::default().fg(Color::Black).bg(Color::White));
}

fn composer_prompt(palette: &ThemePalette) -> Span<'static> {
    Span::styled(
        "› ",
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD),
    )
}

fn composer_row_spans<'a>(
    state: &'a AppState,
    range: std::ops::Range<usize>,
    palette: &ThemePalette,
) -> Vec<Span<'a>> {
    if range.is_empty() {
        return Vec::new();
    }
    let mut spans = Vec::new();
    let mut cursor = range.start;
    for element in state
        .composer_ui
        .atomic_elements
        .iter()
        .filter(|element| element.range.start < range.end && element.range.end > range.start)
    {
        let start = element.range.start.max(range.start);
        let end = element.range.end.min(range.end);
        if cursor < start {
            spans.push(Span::styled(
                &state.input[cursor..start],
                Style::default().fg(Color::White),
            ));
        }
        let style = match element.kind {
            crate::tui::composer::AtomicKind::Folder {
                outside_workspace: true,
            } => Style::default()
                .fg(palette.warning)
                .bg(palette.title_hint_background),
            _ => Style::default()
                .fg(palette.accent_foreground)
                .bg(palette.accent),
        };
        spans.push(Span::styled(&state.input[start..end], style));
        cursor = end;
    }
    if cursor < range.end {
        spans.push(Span::styled(
            &state.input[cursor..range.end],
            Style::default().fg(Color::White),
        ));
    }
    spans
}

pub(crate) fn composer_has_focus(state: &AppState) -> bool {
    !suggestion_overlay_open(state)
        && state.provider_auth.is_none()
        && state.theme_manager.is_none()
        && state.scratch_delete_confirmation.is_none()
        && state.statusline_manager.is_none()
        && state.trace_delete_confirmation.is_none()
        && state.project_init.is_none()
        && state.loop_guard_interaction.is_none()
}

fn composer_cursor_position(inner: Rect, state: &AppState) -> Option<ratatui::layout::Position> {
    if inner.is_empty() {
        return None;
    }
    let layout = crate::tui::composer::ComposerLayout::new(
        &state.input,
        state.input_cursor,
        inner.width as usize,
        2,
    );
    let viewport = layout.visible_window(state.composer_ui.viewport_top);
    if layout.cursor_row < viewport.top || layout.cursor_row >= viewport.end {
        return None;
    }
    let column = 2_usize.saturating_add(layout.cursor_column);
    if column >= inner.width as usize {
        return None;
    }
    Some(ratatui::layout::Position::new(
        inner.x.saturating_add(column as u16),
        inner
            .y
            .saturating_add(layout.cursor_row.saturating_sub(viewport.top) as u16),
    ))
}

fn render_status(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if area.height == 0 {
        return;
    }
    let palette = palette(state);
    let (value, style) = if state.turn_lifecycle.phase == crate::activity::ActivityPhase::Paused {
        (
            state
                .status_message
                .as_deref()
                .unwrap_or("Paused · /unpause · Ctrl+C cancel")
                .to_string(),
            Style::default().fg(palette.warning),
        )
    } else if let Some(msg) = state
        .turn_recovery
        .notice
        .as_ref()
        .filter(|_| !state.turn_lifecycle.is_active())
        .or(state.status_message.as_ref())
    {
        (
            crate::terminal_text::sanitize_terminal_text(msg),
            Style::default().fg(palette.warning),
        )
    } else if state.slash_palette_open
        || state.file_picker_open
        || state.composer_ui.path_completion.open
    {
        (
            "↑↓ select · Tab complete · Enter choose · Esc dismiss".into(),
            Style::default().fg(palette.accent),
        )
    } else if state.transcript_scroll.bottom_offset() > 0 {
        (
            format!(
                "{} rows up{} · Ctrl+End latest",
                state.transcript_scroll.bottom_offset(),
                if state.transcript_scroll.unseen_messages == 0 {
                    String::new()
                } else {
                    format!(" · {} new", state.transcript_scroll.unseen_messages)
                }
            ),
            Style::default().fg(palette.accent),
        )
    } else if state.turn_lifecycle.is_active() {
        (
            if area.width < 80 {
                format!("{} · Ctrl+C stops", composer_metadata(state))
            } else {
                format!(
                    "Ctrl+C also stops · {} edits the latest queued message",
                    state
                        .keymap
                        .display_binding(crate::keymap::KeyAction::EditLastQueued)
                )
            },
            Style::default().fg(palette.muted),
        )
    } else if state.input.is_empty() {
        let metadata = statusline_text(state, false);
        (
            if metadata.is_empty() {
                "/ commands".into()
            } else {
                format!("{metadata} · / commands")
            },
            Style::default().fg(palette.muted),
        )
    } else {
        (
            statusline_text(state, false),
            Style::default().fg(palette.muted),
        )
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            crate::tui::selection::truncate_cells(&value, area.width as usize),
            style,
        )),
        area,
    );
}

fn statusline_text(state: &AppState, include_hints: bool) -> String {
    let mut parts = Vec::new();
    for field in &state.statusline_fields {
        let value = match field {
            crate::ui_settings::StatusField::Messages => {
                (!state.messages.is_empty()).then(|| format!("{} messages", state.messages.len()))
            }
            crate::ui_settings::StatusField::Tokens => {
                let total = state.token_count.0.saturating_add(state.token_count.1);
                if total == 0 {
                    None
                } else if total >= 1_000 {
                    Some(format!("{:.1}k tokens", total as f32 / 1_000.0))
                } else {
                    Some(format!("{total} tokens"))
                }
            }
            crate::ui_settings::StatusField::Model => state
                .current_model
                .as_ref()
                .map(|model| format!("model {}", model.display_name)),
            crate::ui_settings::StatusField::Profile => {
                Some(format!("profile {}", state.active_profile.label()))
            }
            crate::ui_settings::StatusField::Mode => Some(format!(
                "mode {}",
                match &state.mode {
                    crate::app::AgentMode::Agent => "agent",
                    crate::app::AgentMode::Plan => "plan",
                    crate::app::AgentMode::Ask => "ask",
                    crate::app::AgentMode::Debug => "debug",
                    crate::app::AgentMode::Custom(name) => name,
                }
            )),
            crate::ui_settings::StatusField::Workspace => std::path::Path::new(&state.cwd)
                .file_name()
                .map(|name| format!("workspace {}", name.to_string_lossy())),
            crate::ui_settings::StatusField::GitBranch => state
                .attached_worktree
                .as_ref()
                .map(|record| record.branch.clone())
                .or_else(|| state.status_git_branch.clone())
                .map(|branch| format!("branch {branch}")),
            crate::ui_settings::StatusField::GenerationSpeed => state
                .latest_generation_speed_tenths
                .map(|tenths| format!("{}.{:01} tok/s", tenths / 10, tenths % 10))
                .or_else(|| Some("speed unknown".into())),
            crate::ui_settings::StatusField::SittingCost => {
                Some(state.sitting_cost.compact_label())
            }
            crate::ui_settings::StatusField::Custom => state.status_command.output.clone(),
            crate::ui_settings::StatusField::SendHint if include_hints => Some("Enter send".into()),
            crate::ui_settings::StatusField::HistoryHint if include_hints => {
                Some("↑↓ history".into())
            }
            crate::ui_settings::StatusField::CommandsHint if include_hints => {
                Some("/ commands".into())
            }
            crate::ui_settings::StatusField::SendHint
            | crate::ui_settings::StatusField::HistoryHint
            | crate::ui_settings::StatusField::CommandsHint => None,
        };
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            parts.push(crate::terminal_text::sanitize_terminal_text(&value));
        }
    }
    parts.join(" · ")
}

fn suggestion_overlay_open(state: &AppState) -> bool {
    state.model_picker_open
        || state.config_hub.is_some()
        || state.sandbox_hub.is_some()
        || state.context_limit_editor.is_some()
        || state.model_prompt_cache_editor.is_some()
        || state.provider_manager.is_some()
        || state.provider_discovery.is_some()
        || state.provider_editor.is_some()
        || state.session_picker_open
        || state.session_portability.is_some()
        || state.session_retention.is_some()
        || state.timeline_open
        || state.queue_view_open
        || state.pager_open
        || state.dashboard.open
        || state.usage_modal.is_some()
        || state.change_review.open
        || state.change_walkthrough.is_some()
        || state.terminal_manager.is_some()
        || state.copy_picker.is_some()
        || state.notification_picker.is_some()
        || state.command_output_picker.is_some()
        || state.output_full_confirmation.is_some()
        || state.reasoning_picker.is_some()
        || state.route_preset_picker.is_some()
        || state.route_preset_overwrite.is_some()
        || state.jobs_view_open
        || state.btw.is_some()
        || state.voice.is_some()
        || state.history_search.is_some()
        || state.prompt_stash.is_some()
        || state.hook_manager.is_some()
        || state.permission_manager.is_some()
        || state.memory_manager.is_some()
        || state.personality_editor.is_some()
        || state.multi_run_manager.is_some()
        || state.task_graph_manager.is_some()
        || state.plan_workflow_graph.is_some()
        || state.keymap_hub.is_some()
        || state.workflow_preview.is_some()
        || state.workflow_clear_confirmation.is_some()
        || state.worktree_cleanup_confirmation.is_some()
        || state.worktree_setup_confirmation.is_some()
        || state.support_bundle_confirmation.is_some()
        || state.setup_import_confirmation.is_some()
        || state.interaction.is_some()
        || state.workspace_trust_prompt
}

fn transcript_activity_clicks_blocked(state: &AppState) -> bool {
    suggestion_overlay_open(state)
        || state.provider_auth.is_some()
        || state.slash_palette_open
        || state.file_picker_open
        || state.composer_ui.path_completion.open
        || state.theme_manager.is_some()
        || state.scratch_delete_confirmation.is_some()
        || state.statusline_manager.is_some()
        || state.trace_delete_confirmation.is_some()
        || state.project_init.is_some()
        || state.loop_guard_interaction.is_some()
}

fn render_user_prompt_card<'a>(
    lines: &mut Vec<Line<'a>>,
    content: &str,
    width: u16,
    theme: ThemePalette,
    message_index: usize,
    scroll_offset: usize,
) -> UserPromptCardSpan {
    if width == 0 {
        return UserPromptCardSpan::default();
    }
    let style = Style::default()
        .fg(theme.text)
        .bg(theme.title_hint_background);
    let safe = crate::terminal_text::sanitize_terminal_text(content).replace('\t', "    ");
    let layout = crate::tui::composer::ComposerLayout::new(
        &safe,
        safe.len(),
        width.saturating_sub(4) as usize,
        0,
    );
    let total_rows = layout.rows.len();
    let window = crate::tui::composer::USER_PROMPT_CARD_ROWS.max(1);
    let max_top = total_rows.saturating_sub(window);
    let top = scroll_offset.min(max_top);
    let end = (top + window).min(total_rows);
    let hidden_above = top;
    let hidden_below = total_rows.saturating_sub(end);
    let first_line = lines.len();
    lines.push(Line::from("").style(style));
    for (visible_index, row_index) in (top..end).enumerate() {
        let row = &layout.rows[row_index];
        let row_text = &safe[row.range.clone()];
        let mut prefix = if row_index == 0 { " › " } else { "   " }.to_string();
        if visible_index == 0 && hidden_above > 0 {
            prefix = format!(" ↑{hidden_above} ");
        }
        let body_width = usize::from(width.saturating_sub(4)).saturating_sub(
            unicode_width::UnicodeWidthStr::width(prefix.as_str()).saturating_sub(3),
        );
        let mut text = crate::tui::selection::truncate_cells(row_text, body_width.max(1));
        if visible_index + 1 == end.saturating_sub(top) && hidden_below > 0 {
            let marker = format!(" ↓{hidden_below} more ");
            let marker_width = unicode_width::UnicodeWidthStr::width(marker.as_str());
            if marker_width < body_width {
                let keep = body_width - marker_width;
                text = crate::tui::selection::truncate_cells(row_text, keep.max(1));
                text.push_str(&marker);
            } else {
                text = marker;
            }
        }
        lines.push(
            Line::from(vec![
                Span::styled(prefix, style.add_modifier(Modifier::BOLD)),
                Span::styled(text, style),
            ])
            .style(style),
        );
    }
    lines.push(Line::from("").style(style));
    UserPromptCardSpan {
        message_index,
        first_line,
        height: lines.len().saturating_sub(first_line),
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct UserPromptCardSpan {
    pub(crate) message_index: usize,
    pub(crate) first_line: usize,
    pub(crate) height: usize,
}

fn render_reasoning_block<'a>(
    lines: &mut Vec<Line<'a>>,
    thinking: &str,
    expanded: bool,
    duration_ms: Option<u64>,
    width: u16,
    theme: ThemePalette,
) {
    let safe = crate::terminal_text::sanitize_terminal_text(thinking);
    let duration = duration_ms.map_or_else(String::new, |millis| {
        format!(" for {:.1}s", millis as f64 / 1_000.0)
    });
    lines.push(Line::from(Span::styled(
        format!("{} Thought{duration}", if expanded { "▾" } else { "◆" }),
        Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::BOLD),
    )));
    if expanded {
        for line in render_safe_markdown(&safe, width.saturating_sub(2), false, theme) {
            lines.push(Line::from(
                line.spans
                    .into_iter()
                    .map(|span| {
                        Span::styled(span.content.into_owned(), Style::default().fg(theme.muted))
                    })
                    .collect::<Vec<_>>(),
            ));
        }
    }
}

fn render_turn_footer<'a>(
    lines: &mut Vec<Line<'a>>,
    annotation: &crate::app::TranscriptMessageAnnotation,
) {
    if annotation.terminal_outcome.is_none() {
        return;
    }
    let mut parts = Vec::new();
    match (annotation.provider.as_deref(), annotation.model.as_deref()) {
        (Some(provider), Some(model)) => parts.push(format!(
            "{}/{}",
            crate::terminal_text::sanitize_terminal_text(provider),
            crate::terminal_text::sanitize_terminal_text(model)
        )),
        (Some(provider), None) => {
            parts.push(crate::terminal_text::sanitize_terminal_text(provider));
        }
        (None, Some(model)) => {
            parts.push(crate::terminal_text::sanitize_terminal_text(model));
        }
        (None, None) => {}
    }
    if let Some(duration_ms) = annotation.total_turn_duration_ms {
        parts.push(format_turn_duration(duration_ms));
    }
    match annotation.terminal_outcome {
        Some(crate::app::TranscriptTurnOutcome::Cancelled) => parts.push("interrupted".into()),
        Some(crate::app::TranscriptTurnOutcome::Failed) => parts.push("failed".into()),
        Some(crate::app::TranscriptTurnOutcome::Completed) | None => {}
    }
    if parts.is_empty() {
        return;
    }
    lines.push(Line::from(Span::styled(
        parts.join(" · "),
        Style::default().fg(Color::DarkGray),
    )));
}

pub(crate) fn format_turn_duration(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms}ms")
    } else if duration_ms < 60_000 {
        format!(
            "{}.{:01}s",
            duration_ms / 1_000,
            (duration_ms % 1_000) / 100
        )
    } else {
        let seconds = duration_ms / 1_000;
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    }
}

fn render_safe_markdown(
    input: &str,
    width: u16,
    line_numbers: bool,
    theme: ThemePalette,
) -> Vec<Line<'static>> {
    render_safe_markdown_document(input, width, line_numbers, theme).lines
}

fn render_safe_markdown_document(
    input: &str,
    width: u16,
    line_numbers: bool,
    theme: ThemePalette,
) -> crate::markdown::MarkdownDocument {
    if let Some(cached) = transcript_cache::memo_document(input, width, line_numbers, theme) {
        return cached;
    }
    let safe = crate::terminal_text::sanitize_terminal_text(input);
    let rendered = markdown::render_markdown_document_with_palette(
        &safe,
        width,
        line_numbers,
        markdown_palette(theme),
    );
    let document = crate::markdown::MarkdownDocument {
        lines: rendered
            .lines
            .into_iter()
            .map(|line| {
                Line::from(
                    line.spans
                        .into_iter()
                        .map(|span| Span::styled(span.content.into_owned(), span.style))
                        .collect::<Vec<_>>(),
                )
            })
            .collect(),
        links: rendered.links,
    };
    transcript_cache::memo_insert(input, width, line_numbers, theme, &document);
    document
}

fn render_workspace_trust_prompt(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let ws_path = state
        .workspace
        .roots()
        .iter()
        .find(|r| r.primary)
        .map(|r| r.path.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".into());
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Trust this workspace?",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!("  {ws_path}")),
        Line::from(""),
        Line::from("  Until trusted, tools that can read or write this directory stay blocked."),
        Line::from(""),
        Line::from(vec![
            Span::styled("  y", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  Trust this workspace (saved to config trusted_workspaces)"),
        ]),
        Line::from(vec![
            Span::styled("  n", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  Exit without trusting"),
        ]),
        Line::from(""),
        Line::from("  Headless recovery: --trust is process-only fresh-workspace authority"),
        Line::from("  (does not save trusted_workspaces; cannot combine with --continue/resume)."),
    ];
    let block = Block::default()
        .title(" Workspace Trust ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::DarkGray));
    let popup = centered_rect(72, 16, area);
    frame.render_widget(Clear, popup);
    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, popup);
}

fn render_config_hub(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(hub) = state.config_hub.as_ref() else {
        return;
    };
    let pw = 76.min(area.width.saturating_sub(4));
    let ph = 20.min(area.height.saturating_sub(4));
    let pa = centered_rect(pw, ph, area);
    let visible = hub
        .items
        .iter()
        .filter(|item| item.matches(&hub.filter))
        .collect::<Vec<_>>();
    let selected = hub.selected_idx.min(visible.len().saturating_sub(1));
    let mut items = vec![
        ListItem::new(Span::styled(
            " Configuration",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        ListItem::new(""),
        ListItem::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::styled(
                if hub.filter.is_empty() {
                    "type to filter…"
                } else {
                    &hub.filter
                },
                Style::default().fg(Color::White),
            ),
        ])),
        ListItem::new(""),
    ];
    for (index, item) in visible.iter().enumerate() {
        let is_selected = index == selected;
        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        let line = format!(
            "{}{}  {}",
            if is_selected { "▸ " } else { "  " },
            crate::terminal_text::sanitize_terminal_text(&item.label),
            crate::terminal_text::sanitize_terminal_text(&item.value)
        );
        items.push(ListItem::new(Span::styled(
            pad_right(&line, pw.saturating_sub(2) as usize),
            style,
        )));
    }
    if visible.is_empty() {
        items.push(ListItem::new(Span::styled(
            "  No matching settings",
            Style::default().fg(Color::DarkGray),
        )));
    }
    items.push(ListItem::new(""));
    items.push(ListItem::new(Span::styled(
        "Enter open/edit · Esc close · ↑↓ navigate · type to filter",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Clear, pa);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        pa,
    );
}

fn render_sandbox_hub(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(hub) = state.sandbox_hub.as_ref() else {
        return;
    };
    let pw = 82.min(area.width.saturating_sub(4));
    let ph = 24.min(area.height.saturating_sub(2));
    let pa = centered_rect(pw, ph, area);
    let mut items = vec![
        ListItem::new(Span::styled(
            " Sandbox",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        ListItem::new(""),
        ListItem::new(format!("  Saved for next launch  {}", hub.saved_mode)),
        ListItem::new(format!("  Effective this run     {}", hub.effective_mode)),
        ListItem::new(format!(
            "  macOS backend          {}",
            if hub.backend_available {
                "ready"
            } else {
                "unavailable"
            }
        )),
        ListItem::new(format!(
            "  Network enforcement    {}",
            sandbox_network_summary(hub.effective_mode)
        )),
        ListItem::new(format!(
            "  Writable roots         {}",
            hub.writable_roots.len()
        )),
    ];
    for root in hub.writable_roots.iter().take(3) {
        items.push(ListItem::new(format!(
            "    {}",
            crate::terminal_text::sanitize_terminal_text(root)
        )));
    }
    if hub.writable_roots.len() > 3 {
        items.push(ListItem::new(format!(
            "    … {} more",
            hub.writable_roots.len() - 3
        )));
    }
    if hub.writable_roots.is_empty() {
        items.push(ListItem::new(Span::styled(
            "    none",
            Style::default().fg(Color::Yellow),
        )));
    }
    items.push(ListItem::new(""));

    if let Some(confirmation) = hub.confirmation {
        items.push(ListItem::new(Span::styled(
            format!("  Use {} on the next launch?", confirmation.target),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        items.push(ListItem::new(format!(
            "  {}",
            sandbox_mode_description(confirmation.target)
        )));
        items.push(ListItem::new(format!(
            "  This run stays {} until ca restarts.",
            hub.effective_mode
        )));
        items.push(ListItem::new(""));
        let cancel_style = if confirmation.confirm_selected {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        };
        let save_style = if confirmation.confirm_selected {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        items.push(ListItem::new(Line::from(vec![
            Span::styled("  Cancel  ", cancel_style),
            Span::raw("   "),
            Span::styled("  Save for next launch  ", save_style),
        ])));
        items.push(ListItem::new(""));
        items.push(ListItem::new(Span::styled(
            "←→ choose · Enter confirm · Esc cancel",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (index, mode) in crate::app::SandboxHubState::MODES.iter().enumerate() {
            let selected = index == hub.selected_idx;
            let mut markers = Vec::new();
            if *mode == hub.saved_mode {
                markers.push("saved");
            }
            if *mode == hub.effective_mode {
                markers.push("active");
            }
            let marker = if markers.is_empty() {
                String::new()
            } else {
                format!(" [{}]", markers.join(", "))
            };
            let line = format!(
                "{}{}{} — {}",
                if selected { "▸ " } else { "  " },
                mode,
                marker,
                sandbox_mode_description(*mode)
            );
            let style = if selected {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            items.push(ListItem::new(Span::styled(
                pad_right(&line, pw.saturating_sub(2) as usize),
                style,
            )));
        }
        items.push(ListItem::new(""));
        if let Some(error) = &hub.error {
            items.push(ListItem::new(Span::styled(
                format!(
                    "Error: {}",
                    crate::terminal_text::sanitize_terminal_text(error)
                ),
                Style::default().fg(Color::Red),
            )));
        } else if let Some(notice) = &hub.notice {
            items.push(ListItem::new(Span::styled(
                crate::terminal_text::sanitize_terminal_text(notice),
                Style::default().fg(if hub.busy {
                    Color::Yellow
                } else {
                    Color::Green
                }),
            )));
        }
        let footer = if hub.busy {
            "Saving…"
        } else if hub.writes_enabled {
            "Enter choose · Esc close · ↑↓ navigate"
        } else {
            "View only in bare mode · Esc close · ↑↓ navigate"
        };
        items.push(ListItem::new(Span::styled(
            footer,
            Style::default().fg(Color::DarkGray),
        )));
    }

    frame.render_widget(Clear, pa);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        pa,
    );
}

fn sandbox_network_summary(mode: crate::sandbox::SandboxMode) -> &'static str {
    match mode {
        crate::sandbox::SandboxMode::Required => "OS-enforced when granted",
        crate::sandbox::SandboxMode::PolicyOnly => "policy only; not OS-enforced",
        crate::sandbox::SandboxMode::Off => "none; direct processes",
    }
}

fn sandbox_mode_description(mode: crate::sandbox::SandboxMode) -> &'static str {
    match mode {
        crate::sandbox::SandboxMode::Required => "OS process isolation",
        crate::sandbox::SandboxMode::PolicyOnly => "approval policy; no OS sandbox",
        crate::sandbox::SandboxMode::Off => "no process sandbox",
    }
}

fn render_context_limit_editor(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(editor) = state.context_limit_editor.as_ref() else {
        return;
    };
    let pw = 76.min(area.width.saturating_sub(4));
    let ph = 20.min(area.height.saturating_sub(2));
    let pa = centered_rect(pw, ph, area);
    let target = crate::terminal_text::sanitize_terminal_text(&format!(
        "{}/{}",
        editor.provider, editor.model
    ));
    let storage = if editor.editing_profile {
        "model profile"
    } else {
        "exact model context"
    };
    let configured_window = editor.window_input.parse::<u64>().ok();
    let configured_threshold = editor.threshold_input.parse::<u64>().ok();
    let effective_window = configured_window.or(editor.fallback_window);
    let explicit_threshold = configured_threshold.or(editor.fallback_threshold);
    let effective_threshold =
        explicit_threshold.or_else(|| effective_window.map(|window| window.saturating_mul(4) / 5));
    let mut items = vec![
        ListItem::new(Span::styled(
            " Context limits",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        ListItem::new(""),
        ListItem::new(format!("  Model    {target}")),
        ListItem::new(format!("  Stored   {storage}")),
        ListItem::new(""),
    ];

    for (field, label, input, fallback) in [
        (
            crate::app::ContextLimitField::Window,
            "Context window      ",
            &editor.window_input,
            editor.fallback_window,
        ),
        (
            crate::app::ContextLimitField::Threshold,
            "Checkpoint threshold",
            &editor.threshold_input,
            editor.fallback_threshold,
        ),
    ] {
        let selected = editor.field == field;
        let value = if input.is_empty() {
            "<inherit>".into()
        } else {
            input.clone()
        };
        let inheritance = if input.is_empty() {
            fallback.map_or_else(
                || {
                    if field == crate::app::ContextLimitField::Threshold {
                        "automatic 80%".into()
                    } else {
                        "unknown".into()
                    }
                },
                |value| format!("inherits {value}"),
            )
        } else {
            "configured here".into()
        };
        let line = if pw < 70 {
            format!(
                "{}{}  {}  [{}]",
                if selected { "▸ " } else { "  " },
                if field == crate::app::ContextLimitField::Window {
                    "Window    "
                } else {
                    "Checkpoint"
                },
                value,
                match inheritance.as_str() {
                    "configured here" => "set",
                    "automatic 80%" => "80% default",
                    "unknown" => "unknown",
                    inherited => inherited,
                }
            )
        } else {
            format!(
                "{}{}  {:<14}  {}",
                if selected { "▸ " } else { "  " },
                label,
                value,
                inheritance
            )
        };
        let style = if selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        items.push(ListItem::new(Span::styled(
            pad_right(&line, pw.saturating_sub(2) as usize),
            style,
        )));
    }

    items.push(ListItem::new(""));
    items.push(ListItem::new(format!(
        "  Effective window      {}",
        effective_window.map_or_else(|| "unknown".into(), |value| value.to_string())
    )));
    items.push(ListItem::new(format!(
        "  Effective checkpoint  {}{}",
        effective_threshold.map_or_else(|| "unknown".into(), |value| value.to_string()),
        if configured_threshold.is_none() && editor.fallback_threshold.is_none() {
            " (automatic 80%)"
        } else {
            ""
        }
    )));
    items.push(ListItem::new(Span::styled(
        "  Empty values inherit independently.",
        Style::default().fg(Color::DarkGray),
    )));
    if let Some(error) = &editor.error {
        let safe_error = crate::terminal_text::sanitize_terminal_text(error);
        if pw < 70 {
            items.push(ListItem::new(Span::styled(
                "Error:",
                Style::default().fg(Color::Red),
            )));
            items.push(ListItem::new(Span::styled(
                format!("  {safe_error}"),
                Style::default().fg(Color::Red),
            )));
        } else {
            items.push(ListItem::new(Span::styled(
                format!("Error: {safe_error}"),
                Style::default().fg(Color::Red),
            )));
        }
    }
    let footer = if editor.saving {
        "Saving…"
    } else if editor.writes_enabled {
        if pw < 70 {
            "Enter save · Esc cancel · Tab/↑↓ field"
        } else {
            "Enter save · Esc cancel · Tab/↑↓ field · empty inherits"
        }
    } else {
        "View only in bare mode · Esc close · Tab/↑↓ field"
    };
    items.push(ListItem::new(""));
    items.push(ListItem::new(Span::styled(
        footer,
        Style::default().fg(Color::DarkGray),
    )));

    frame.render_widget(Clear, pa);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        pa,
    );
}

fn render_model_prompt_cache_editor(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(editor) = state.model_prompt_cache_editor.as_ref() else {
        return;
    };
    let pw = 78.min(area.width.saturating_sub(4));
    let ph = 22.min(area.height.saturating_sub(2));
    let pa = centered_rect(pw, ph, area);
    let target = crate::terminal_text::sanitize_terminal_text(&format!(
        "{}/{}",
        editor.provider, editor.model
    ));
    let request = match editor.request_format {
        None => "Undeclared",
        Some(crate::config::PromptCacheRequestFormat::ProviderManaged) => "Provider managed",
        Some(crate::config::PromptCacheRequestFormat::OpenAiResponsesExplicit) => {
            "Responses explicit"
        }
    };
    let usage = match editor.usage_format {
        None => "Undeclared",
        Some(crate::config::PromptCacheUsageFormat::Auto) => "Automatic detection",
        Some(crate::config::PromptCacheUsageFormat::OpenAiDetails) => "OpenAI token details",
        Some(crate::config::PromptCacheUsageFormat::HitMiss) => "Hit/miss totals",
        Some(crate::config::PromptCacheUsageFormat::ReadWrite) => "Read/write totals",
    };
    let policy = editor.policy_override.map_or_else(
        || {
            editor.effective_policy().map_or_else(
                || "Inherit (unavailable)".to_string(),
                |policy| format!("Inherit ({})", prompt_cache_policy_label(policy)),
            )
        },
        |policy| prompt_cache_policy_label(policy).to_string(),
    );
    let contract_source = match editor.contract_source {
        crate::provider_contract::PromptCacheContractSource::FirstParty => "built in",
        crate::provider_contract::PromptCacheContractSource::ModelConfig => "exact model",
        crate::provider_contract::PromptCacheContractSource::Unspecified => "undeclared",
    };
    let policy_source = match editor.policy_source {
        crate::provider_contract::PromptCachePolicySource::ModelConfig => "exact model",
        crate::provider_contract::PromptCachePolicySource::LegacyProvider => "legacy provider",
        crate::provider_contract::PromptCachePolicySource::ContractDefault => "contract default",
        crate::provider_contract::PromptCachePolicySource::Unspecified => "undeclared",
    };
    let field = |label: &str, value: &str, selected: bool, locked: bool| -> ListItem<'static> {
        let marker = if selected { "▸" } else { " " };
        let suffix = if locked { "  [built in]" } else { "" };
        let line = format!("{marker} {label:<18}{value}{suffix}");
        ListItem::new(Span::styled(
            line,
            if selected {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else if locked {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            },
        ))
    };
    let contract_id = crate::terminal_text::sanitize_terminal_text(&editor.contract_id);
    let mut items = vec![
        ListItem::new(Span::styled(
            " Prompt cache",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        ListItem::new(""),
    ];
    if pw < 70 {
        let value_width = pw.saturating_sub(14) as usize;
        items.extend([
            ListItem::new(format!(
                "  Model     {}",
                truncate_middle_chars(&target, value_width)
            )),
            ListItem::new(format!("  Protocol  {}", editor.protocol.label())),
            ListItem::new(format!("  Contract  {contract_source}")),
            ListItem::new(format!(
                "  ID        {}",
                truncate_middle_chars(&contract_id, value_width)
            )),
        ]);
    } else {
        items.extend([
            ListItem::new(format!("  Model             {target}")),
            ListItem::new(format!("  Protocol          {}", editor.protocol.label())),
            ListItem::new(format!(
                "  Contract          {contract_id} ({contract_source})"
            )),
        ]);
    }
    items.extend([
        ListItem::new(""),
        field(
            "Request format",
            request,
            editor.field == crate::app::ModelPromptCacheField::RequestFormat,
            editor.formats_locked,
        ),
        field(
            "Usage format",
            usage,
            editor.field == crate::app::ModelPromptCacheField::UsageFormat,
            editor.formats_locked,
        ),
        field(
            "Policy",
            &policy,
            editor.field == crate::app::ModelPromptCacheField::Policy,
            false,
        ),
        ListItem::new(format!("  Effective source  {policy_source}")),
        ListItem::new(""),
    ]);
    if let Some(error) = &editor.error {
        items.push(ListItem::new(Span::styled(
            format!("  {}", crate::terminal_text::sanitize_terminal_text(error)),
            Style::default().fg(Color::Red),
        )));
    } else if editor.formats_locked {
        items.push(ListItem::new(Span::styled(
            if pw < 70 {
                "  First-party formats are fixed."
            } else {
                "  First-party request and usage formats are fixed for this exact route."
            },
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        items.push(ListItem::new(Span::styled(
            if pw < 70 {
                "  Use only documented provider formats."
            } else {
                "  Select custom formats only when documented by the provider."
            },
            Style::default().fg(Color::DarkGray),
        )));
    }
    items.push(ListItem::new(""));
    let footer_style = Style::default().fg(Color::DarkGray);
    if editor.saving {
        items.push(ListItem::new(Span::styled("Saving…", footer_style)));
    } else if editor.writes_enabled && pw < 70 {
        items.push(ListItem::new(Span::styled(
            "Tab/↑↓ field · ←/→ choose",
            footer_style,
        )));
        items.push(ListItem::new(Span::styled(
            "Backspace inherit · Enter save · Esc",
            footer_style,
        )));
    } else {
        let footer = if editor.writes_enabled {
            "Tab/↑↓ field · ←/→ choose · Backspace inherit · Enter save · Esc"
        } else {
            "View only in bare mode · Esc close"
        };
        items.push(ListItem::new(Span::styled(footer, footer_style)));
    }
    frame.render_widget(Clear, pa);
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        pa,
    );
}

fn prompt_cache_policy_label(policy: crate::config::PromptCachePolicy) -> &'static str {
    match policy {
        crate::config::PromptCachePolicy::Automatic => "Automatic",
        crate::config::PromptCachePolicy::StablePrefix => "Stable prefix",
        crate::config::PromptCachePolicy::Off => "Off",
    }
}

fn render_model_picker(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let pw = 70.min(area.width.saturating_sub(4));
    let ph = 20.min(area.height.saturating_sub(4));
    let pa = centered_rect(pw, ph, area);

    let visible = crate::tui::input::visible_model_rows(state);

    let mut items: Vec<ListItem> = Vec::new();
    items.push(ListItem::new(Line::from(Span::styled(
        match state.model_picker_target {
            crate::app::ModelPickerTarget::Current => " Switch model",
            crate::app::ModelPickerTarget::Review => " Select review model",
            crate::app::ModelPickerTarget::Delegation => " Select delegated agent",
            crate::app::ModelPickerTarget::MultiRun(_) => " Select candidate route",
            crate::app::ModelPickerTarget::PlanWorkflow => " Select plan agent provider/model",
        },
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ))));
    items.push(ListItem::new(""));
    items.push(ListItem::new(Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Cyan)),
        Span::styled(
            if state.model_filter.is_empty() {
                "type to filter…"
            } else {
                &state.model_filter
            },
            Style::default().fg(Color::White),
        ),
    ])));
    let catalog_status = state
        .model_catalog_status
        .as_deref()
        .unwrap_or("Configured models");
    items.push(ListItem::new(Span::styled(
        format!(
            "  {}",
            crate::terminal_text::sanitize_terminal_text(catalog_status)
        ),
        Style::default().fg(if state.model_catalog_refreshing {
            Color::Yellow
        } else {
            Color::DarkGray
        }),
    )));
    let inherit_visible = crate::tui::input::delegation_inherit_visible(state);
    let row_offset = usize::from(inherit_visible);
    let visible_count = visible.len() + row_offset;
    let selected_idx = if visible_count == 0 {
        0
    } else {
        state.selected_model_idx.min(visible_count - 1)
    };
    let selection_status = if inherit_visible && selected_idx == 0 {
        format!("1/{visible_count} · follows the active parent agent")
    } else {
        visible
            .get(selected_idx.saturating_sub(row_offset))
            .map_or_else(
                || "No matches. ⇧A add model · ⇧P add provider".into(),
                |row| {
                    let model = &row.model;
                    let provider_status = if state.model_catalog_refreshing {
                        "refreshing"
                    } else {
                        state
                            .model_catalog_provider_status
                            .get(&model.provider)
                            .map(String::as_str)
                            .unwrap_or("configured only")
                    };
                    format!(
                        "{}/{} · {} · {}{}",
                        selected_idx + 1,
                        visible_count,
                        crate::terminal_text::sanitize_terminal_text(&model.provider),
                        if state.model_is_configured_only(model) {
                            "configured only · "
                        } else {
                            ""
                        },
                        crate::terminal_text::sanitize_terminal_text(provider_status)
                    )
                },
            )
    };
    items.push(ListItem::new(Span::styled(
        format!("  {selection_status}"),
        Style::default().fg(Color::DarkGray),
    )));
    items.push(ListItem::new(""));

    let interior_height = ph.saturating_sub(2) as usize;
    let model_capacity = interior_height
        .saturating_sub(10)
        .saturating_sub(row_offset)
        .max(1);
    let narrow_rows = pw < 64;
    let projected_height = |start: usize, end: usize| {
        let mut height = 0usize;
        let mut section = None;
        for row in visible.iter().take(end.saturating_add(1)).skip(start) {
            if section.as_ref() != Some(&row.section) {
                height = height.saturating_add(1);
                section = Some(row.section.clone());
            }
            height = height.saturating_add(if narrow_rows { 2 } else { 1 });
        }
        height
    };
    let selected_model_idx = selected_idx.saturating_sub(row_offset);
    let mut start = selected_model_idx;
    while start > 0 && projected_height(start - 1, selected_model_idx) <= model_capacity {
        start -= 1;
    }
    let mut rendered = 0usize;
    if inherit_visible {
        let is_selected = selected_idx == 0;
        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_selected { "▸ " } else { "  " };
        let label = format!("{prefix}  Same as current agent");
        let description = "inherits parent";
        let row_width = pw.saturating_sub(2) as usize;
        let row = crate::tui::selection::layout_row(
            &label,
            description,
            row_width.saturating_sub(1),
            narrow_rows,
        );
        if row.stacked {
            items.push(ListItem::new(Span::styled(
                pad_right(&row.label, row_width),
                style,
            )));
            items.push(ListItem::new(Span::styled(
                pad_right(&row.description, row_width),
                style,
            )));
        } else {
            items.push(ListItem::new(Span::styled(
                pad_right(
                    &format!("{}{}{}", row.label, row.gap, row.description),
                    row_width,
                ),
                style,
            )));
        }
    }
    let mut section = None;
    for (vi, row) in visible.iter().enumerate().skip(start) {
        let section_changed = section.as_ref() != Some(&row.section);
        let required = usize::from(section_changed).saturating_add(if narrow_rows { 2 } else { 1 });
        if rendered.saturating_add(required) > model_capacity {
            break;
        }
        if section_changed {
            items.push(ListItem::new(Span::styled(
                format!(
                    "  {}",
                    crate::terminal_text::sanitize_terminal_text(row.section.label())
                ),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )));
            rendered = rendered.saturating_add(1);
            section = Some(row.section.clone());
        }
        let m = &row.model;
        let is_sel = vi + row_offset == selected_idx;
        let is_current = state
            .current_model
            .as_ref()
            .is_some_and(|current| current == m);
        let style = if is_sel {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_sel { "▸ " } else { "  " };
        let favorite = if row.favorite { "★ " } else { "  " };
        let label = format!(
            "{}{}{}",
            prefix,
            favorite,
            crate::terminal_text::sanitize_terminal_text(&m.display_name),
        );
        let description = format!(
            "[{}]{}{}",
            crate::terminal_text::sanitize_terminal_text(&m.provider),
            if state.model_is_configured_only(m) {
                "  configured only"
            } else {
                ""
            },
            if is_current { "  current" } else { "" }
        );
        let row_width = pw.saturating_sub(2) as usize;
        let row = crate::tui::selection::layout_row(
            &label,
            &description,
            row_width.saturating_sub(1),
            narrow_rows,
        );
        if row.stacked {
            items.push(ListItem::new(Span::styled(
                pad_right(&row.label, row_width),
                style,
            )));
            items.push(ListItem::new(Span::styled(
                pad_right(&row.description, row_width),
                style,
            )));
            rendered = rendered.saturating_add(2);
        } else {
            let combined = format!("{}{}{}", row.label, row.gap, row.description);
            items.push(ListItem::new(Span::styled(
                pad_right(&combined, row_width),
                style,
            )));
            rendered = rendered.saturating_add(1);
        }
    }

    items.push(ListItem::new(""));
    items.push(ListItem::new(Span::styled(
        if matches!(
            state.model_picker_target,
            crate::app::ModelPickerTarget::Delegation
        ) {
            "Enter save · Esc close · ↑↓ move · PgUp/PgDn page"
        } else {
            "Enter select · Esc close · ↑↓ move · PgUp/PgDn page"
        },
        Style::default().fg(Color::DarkGray),
    )));
    if matches!(
        state.model_picker_target,
        crate::app::ModelPickerTarget::MultiRun(_) | crate::app::ModelPickerTarget::PlanWorkflow
    ) {
        items.push(ListItem::new(Span::styled(
            "Home/End jump · ⇧R refresh · selection only",
            Style::default().fg(Color::DarkGray),
        )));
        items.push(ListItem::new(Span::styled(
            "Edit providers and models through /provider",
            Style::default().fg(Color::DarkGray),
        )));
    } else if matches!(
        state.model_picker_target,
        crate::app::ModelPickerTarget::Delegation
    ) {
        items.push(ListItem::new(Span::styled(
            if pw < 64 {
                "Default inherits current · other providers supported"
            } else {
                "Default inherits the active agent · cross-provider routes supported"
            },
            Style::default().fg(Color::DarkGray),
        )));
        items.push(ListItem::new(Span::styled(
            "⇧R refresh · use /provider to add or edit routes",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        items.push(ListItem::new(Span::styled(
            "Home/End jump · ⇧A model · ⇧P provider",
            Style::default().fg(Color::DarkGray),
        )));
        items.push(ListItem::new(Span::styled(
            if matches!(
                state.model_picker_target,
                crate::app::ModelPickerTarget::Current
            ) {
                "⇧F favorite · ⇧E edit model/key · ⇧R refresh"
            } else {
                "⇧E edit model/key · ⇧R refresh"
            },
            Style::default().fg(Color::DarkGray),
        )));
    }

    frame.render_widget(Clear, pa);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let list = List::new(items).block(block);
    frame.render_widget(list, pa);
}

fn render_provider_editor(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    use crate::app::ProviderEditorField;

    let Some(editor) = state.provider_editor.as_ref() else {
        return;
    };
    let prompt_cache = editor.mode == crate::app::ProviderEditorMode::EditProvider
        && editor.draft.provider == "openai"
        && editor.draft.base_url.trim_end_matches('/') == crate::pricing::OPENAI_API_BASE_URL;
    let pw = 72.min(area.width.saturating_sub(4));
    let ph = if editor.mode == crate::app::ProviderEditorMode::EditProvider {
        if prompt_cache {
            17
        } else {
            15
        }
    } else {
        18
    }
    .min(area.height.saturating_sub(4));
    let pa = centered_rect(pw, ph, area);
    let existing_provider = editor.mode != crate::app::ProviderEditorMode::AddProvider;
    let title = match editor.mode {
        crate::app::ProviderEditorMode::AddProvider if editor.protocol_override.is_some() => {
            format!(
                " Add provider: {} ",
                crate::terminal_text::sanitize_terminal_text(&editor.draft.provider)
            )
        }
        crate::app::ProviderEditorMode::AddProvider => " Add provider ".to_string(),
        crate::app::ProviderEditorMode::EditProvider => format!(
            " Edit provider: {} ",
            crate::terminal_text::sanitize_terminal_text(&editor.draft.provider)
        ),
        crate::app::ProviderEditorMode::AddModel => format!(
            " Add model to {} ",
            crate::terminal_text::sanitize_terminal_text(&editor.draft.provider)
        ),
        crate::app::ProviderEditorMode::EditModel => " Edit model ".to_string(),
    };

    let field_value_width = (pw.saturating_sub(2) as usize).saturating_sub(17).max(1);
    let field = |label: &str, value: &str, selected: bool| {
        let marker = if selected { "›" } else { " " };
        let selected_style = if selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        let mut spans = vec![
            Span::styled(
                format!("{marker} {label:<14}"),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(" "),
        ];
        if selected {
            let cursor = floor_render_char_boundary(
                value,
                if label == "API key" {
                    value.len()
                } else {
                    editor.cursor
                },
            );
            let (before, after) = editor_value_window(value, cursor, field_value_width);
            spans.push(Span::styled(before, selected_style));
            spans.push(Span::styled(
                "▌",
                Style::default().fg(Color::White).bg(Color::Cyan),
            ));
            spans.push(Span::styled(after, selected_style));
        } else {
            spans.push(Span::styled(
                if value.is_empty() {
                    " ".to_string()
                } else {
                    truncate_middle_chars(value, field_value_width)
                },
                selected_style,
            ));
        }
        Line::from(spans)
    };

    let hint = |text: &str| {
        Line::from(Span::styled(
            format!("                  {text}"),
            Style::default().fg(Color::DarkGray),
        ))
    };

    let mut lines = vec![
        Line::from(""),
        field(
            "Provider",
            &editor.draft.provider,
            editor.field == ProviderEditorField::Provider,
        ),
        hint(if existing_provider {
            "Existing provider"
        } else {
            "Use letters, numbers, . _ or -"
        }),
        field(
            "Base URL",
            &editor.draft.base_url,
            editor.field == ProviderEditorField::BaseUrl,
        ),
        hint(if existing_provider {
            "Provider endpoint"
        } else {
            "Required; use http:// or https://"
        }),
        field(
            "API key",
            if editor.api_key.is_empty() {
                ""
            } else {
                "••••••••"
            },
            editor.field == ProviderEditorField::ApiKey,
        ),
        hint(if !editor.api_key.is_empty() {
            "Ready to save in the secure credential store"
        } else if editor.credential_in_keychain {
            "Saved securely; paste to replace"
        } else if editor.credential_configured {
            "Blank keeps the current credential"
        } else {
            "Paste to save in the secure credential store"
        }),
        field(
            "API key env",
            &editor.draft.api_key_env,
            editor.field == ProviderEditorField::ApiKeyEnv,
        ),
        hint(if !editor.draft.api_key_env.is_empty() {
            "Environment variable name"
        } else {
            "Alternative: environment variable"
        }),
    ];
    if let Some(google) = editor
        .draft
        .google
        .as_ref()
        .filter(|settings| settings.mode == crate::google_auth::Mode::Vertex)
    {
        lines.truncate(5);
        lines.push(field(
            "Project ID",
            google.project_id.as_deref().unwrap_or(""),
            editor.field == ProviderEditorField::GoogleProject,
        ));
        lines.push(field(
            "Location",
            google.location.as_deref().unwrap_or(""),
            editor.field == ProviderEditorField::GoogleLocation,
        ));
        lines.push(field(
            "Quota project",
            google.quota_project_id.as_deref().unwrap_or(""),
            editor.field == ProviderEditorField::GoogleQuotaProject,
        ));
        lines.push(hint("Uses Application Default Credentials"));
    }
    if prompt_cache {
        let cache_label = match editor.prompt_cache_mode {
            crate::config::PromptCacheMode::Implicit => "Automatic",
            crate::config::PromptCacheMode::StablePrefix => "Stable prefix",
            crate::config::PromptCacheMode::Off => "Off",
        };
        lines.push(Line::from(vec![
            Span::styled("  Legacy cache    ", Style::default().fg(Color::Cyan)),
            Span::styled(cache_label, Style::default().fg(Color::DarkGray)),
            Span::styled("  [read only]", Style::default().fg(Color::DarkGray)),
        ]));
        lines.push(hint("Use /cache provider/model"));
    }
    if editor.mode != crate::app::ProviderEditorMode::EditProvider {
        lines.push(field(
            "Model ID",
            &editor.draft.model,
            editor.field == ProviderEditorField::Model,
        ));
        lines.push(hint("Exact ID from the provider"));
    }
    if let Some(protocol) = editor.protocol_override {
        let label = match protocol {
            crate::agent::ProviderProtocol::ChatCompletions => "Chat Completions",
            crate::agent::ProviderProtocol::OpenAiResponses => "OpenAI Responses",
            crate::agent::ProviderProtocol::AnthropicMessages => "Anthropic Messages",
            crate::agent::ProviderProtocol::GoogleGenerateContent => "Google GenerateContent",
        };
        lines.push(field("Protocol", label, false));
    }
    if editor.saving {
        lines.push(Line::from(Span::styled(
            "Saving provider and API key…",
            Style::default().fg(Color::Yellow),
        )));
    } else if let Some(error) = &editor.error {
        lines.push(Line::from(Span::styled(
            error.as_str(),
            Style::default().fg(Color::Red),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Key stays masked; config stores only a reference.",
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tab fields · ←→ cursor · Enter save · Esc back",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        "Home/End jump · Backspace/Delete edit",
        Style::default().fg(Color::DarkGray),
    )));

    frame.render_widget(Clear, pa);
    let paragraph = Paragraph::new(Text::from(lines)).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(paragraph, pa);
}

fn floor_render_char_boundary(value: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(value.len());
    while !value.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn truncate_middle_chars(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_string();
    }
    if max_chars <= 1 {
        return "…".into();
    }
    let left = (max_chars - 1) / 2;
    let right = max_chars - 1 - left;
    let beginning: String = value.chars().take(left).collect();
    let ending: String = value
        .chars()
        .rev()
        .take(right)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{beginning}…{ending}")
}

fn editor_value_window(value: &str, cursor: usize, max_chars: usize) -> (String, String) {
    if max_chars <= 1 {
        return (String::new(), String::new());
    }
    let cursor = floor_render_char_boundary(value, cursor);
    let total = value.chars().count();
    let cursor_chars = value[..cursor].chars().count();
    let mut content_capacity = max_chars - 1;
    let mut start = cursor_chars.saturating_sub(content_capacity / 2);
    start = start.min(total.saturating_sub(content_capacity));
    let mut end = (start + content_capacity).min(total);

    for _ in 0..2 {
        let markers = usize::from(start > 0) + usize::from(end < total);
        content_capacity = max_chars.saturating_sub(1 + markers);
        start = cursor_chars.saturating_sub(content_capacity / 2);
        start = start.min(total.saturating_sub(content_capacity));
        end = (start + content_capacity).min(total);
    }

    let mut before = String::new();
    if start > 0 {
        before.push('…');
    }
    before.extend(value.chars().skip(start).take(cursor_chars - start));

    let mut after: String = value
        .chars()
        .skip(cursor_chars)
        .take(end.saturating_sub(cursor_chars))
        .collect();
    if end < total {
        after.push('…');
    }
    (before, after)
}

fn render_slash_palette(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let palette = palette(state);
    let busy = state.streaming.is_some() || state.approval_queue.is_active();
    let commands = crate::tui::commands::matching_commands(&state.slash_filter, busy);
    let prompt_commands = state.prompt_commands.matching(&state.slash_filter, busy);
    let command_count = commands.len() + prompt_commands.len();
    let narrow = area.width < 80;
    let max_rows = if narrow { 5 } else { 8 };
    let selected = state
        .selected_slash_idx
        .min(command_count.saturating_sub(1));
    let visible =
        crate::tui::selection::ListViewport::default().visible(selected, command_count, max_rows);
    let provisional_pa = composer_popup_rect(area, state, 3, area.width);
    let row_width = provisional_pa.width.saturating_sub(2);
    let mut lines = Vec::new();
    for index in visible.start..visible.end {
        let (name, usage, description) = if let Some(spec) = commands.get(index) {
            (spec.name, spec.usage, spec.description)
        } else {
            let command = prompt_commands[index - commands.len()];
            (
                command.name.as_str(),
                "[arguments]",
                command.description.as_str(),
            )
        };
        let is_selected = index == selected;
        let style = if is_selected {
            Style::default()
                .fg(palette.accent_foreground)
                .bg(palette.accent)
        } else {
            Style::default().fg(Color::White)
        };
        let usage = if usage.is_empty() {
            String::new()
        } else {
            format!(" {usage}")
        };
        let prefix = if is_selected { "▸ " } else { "  " };
        let label = format!("{prefix}/{name}{usage}");
        let row = slash_palette_row(&label, description, row_width as usize, narrow);
        if row.stacked {
            lines.push(Line::from(Span::styled(row.label, style)));
            lines.push(Line::from(Span::styled(
                row.description,
                if is_selected {
                    style
                } else {
                    Style::default().fg(palette.muted)
                },
            )));
        } else {
            lines.push(Line::from(vec![
                Span::styled(row.label, style),
                Span::styled(row.gap, style),
                Span::styled(
                    row.description,
                    if is_selected {
                        style
                    } else {
                        Style::default().fg(palette.muted)
                    },
                ),
            ]));
        }
    }
    if command_count == 0 {
        lines.push(Line::from(Span::styled(
            "  No matching commands · /help browses all",
            Style::default().fg(palette.muted),
        )));
    }
    let paragraph = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
    let content_rows = paragraph.line_count(row_width).max(1);
    let pa = composer_popup_rect(
        area,
        state,
        content_rows.saturating_add(2).min(u16::MAX as usize) as u16,
        area.width,
    );
    frame.render_widget(Clear, pa);
    let block = Block::default()
        .title(format!(
            " Commands {}/{} ",
            if commands.is_empty() { 0 } else { selected + 1 },
            commands.len()
        ))
        .title_bottom(Line::from(Span::styled(
            " ↑↓ select · Tab complete · Enter choose · Esc dismiss ",
            Style::default().fg(palette.muted),
        )))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.accent));
    frame.render_widget(paragraph.block(block), pa);
}

fn slash_palette_row(
    label: &str,
    description: &str,
    width: usize,
    stacked: bool,
) -> crate::tui::selection::SelectionRowLayout {
    let label_cells = unicode_width::UnicodeWidthStr::width(label);
    let description_cells = unicode_width::UnicodeWidthStr::width(description);
    if stacked
        || label_cells
            .saturating_add(1)
            .saturating_add(description_cells)
            > width
    {
        return crate::tui::selection::SelectionRowLayout {
            label: label.to_string(),
            gap: String::new(),
            description: format!("    {description}"),
            stacked: true,
        };
    }

    let maximum_label_column = width.saturating_sub(description_cells.saturating_add(1));
    let label_width = (width / 2)
        .clamp(12, 32)
        .max(label_cells)
        .min(maximum_label_column);
    let gap = " ".repeat(label_width.saturating_sub(label_cells).saturating_add(1));
    crate::tui::selection::SelectionRowLayout {
        label: label.to_string(),
        gap,
        description: description.to_string(),
        stacked: false,
    }
}

fn composer_popup_rect(area: Rect, state: &AppState, desired_height: u16, max_width: u16) -> Rect {
    let composer_layout = crate::tui::composer::ComposerLayout::new(
        &state.input,
        state.input_cursor,
        area.width.saturating_sub(2) as usize,
        2,
    );
    let viewport = composer_layout.visible_window(state.composer_ui.viewport_top);
    let queue_rows = state.input_queue.len().min(3) as u16 + u16::from(state.input_queue.len() > 3);
    let attachment_rows = u16::from(!state.pasted_images.is_empty() || state.image_attachment_busy);
    let paste_preview_rows = u16::from(
        state
            .composer_ui
            .active_large_paste_index(state.input_cursor)
            .is_some(),
    ) * (crate::tui::composer::PASTE_PREVIEW_ROWS as u16 + 1);
    let input_height = state.input_height.max(
        2 + viewport.end.saturating_sub(viewport.top) as u16
            + queue_rows
            + attachment_rows
            + paste_preview_rows,
    );
    let composer_y = area
        .y
        .saturating_add(area.height)
        .saturating_sub(1)
        .saturating_sub(input_height);
    let height = desired_height.min(composer_y.saturating_sub(area.y).max(3));
    let width = max_width
        .min(area.width.saturating_sub(4))
        .max(20.min(area.width));
    Rect {
        x: area.x.saturating_add(2),
        y: composer_y.saturating_sub(height),
        width,
        height,
    }
}

fn render_session_picker(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let pw = 120.min(area.width.saturating_sub(4));
    let ph = 28.min(area.height.saturating_sub(4));
    let pa = centered_rect(pw, ph, area);
    let theme = palette(state);
    let inner = Rect {
        x: pa.x.saturating_add(2),
        y: pa.y.saturating_add(1),
        width: pa.width.saturating_sub(4),
        height: pa.height.saturating_sub(2),
    };

    let mut items: Vec<ListItem> = Vec::new();
    let heading = "Sessions";
    let close = "Esc";
    let heading_gap = " ".repeat(
        inner
            .width
            .saturating_sub((heading.len() + close.len()) as u16) as usize,
    );
    items.push(ListItem::new(Line::from(vec![
        Span::styled(
            heading,
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::raw(heading_gap),
        Span::styled(close, Style::default().fg(theme.muted)),
    ])));
    items.push(ListItem::new(""));
    items.push(ListItem::new(Line::from(vec![
        Span::styled(
            format!(" Recent {} ", state.former_active_session_count()),
            if state.session_picker_view == crate::app::SessionPickerView::Active {
                Style::default()
                    .fg(theme.accent_foreground)
                    .bg(theme.accent)
            } else {
                Style::default().fg(theme.muted)
            },
        ),
        Span::raw("  "),
        Span::styled(
            format!(" Archived {} ", state.session_archive_list.len()),
            if state.session_picker_view == crate::app::SessionPickerView::Archive {
                Style::default()
                    .fg(theme.accent_foreground)
                    .bg(theme.accent)
            } else {
                Style::default().fg(theme.muted)
            },
        ),
        Span::styled("  Tab changes list", Style::default().fg(theme.muted)),
    ])));
    let (search_text, search_style) = if state.session_filter.is_empty() {
        (
            "Type to search names, folders, or IDs".to_string(),
            Style::default().fg(theme.muted),
        )
    } else {
        (
            crate::terminal_text::sanitize_terminal_text(&state.session_filter),
            Style::default().fg(theme.text),
        )
    };
    items.push(ListItem::new(Line::from(vec![
        Span::styled("Search  ", Style::default().fg(theme.muted)),
        Span::styled(search_text, search_style),
    ])));
    items.push(ListItem::new(""));

    let visible = state.session_picker_visible_summaries();
    let visible_count = visible.len();
    let show_name_hint = visible
        .get(state.selected_session_idx)
        .is_some_and(|summary| crate::session::session_title_needs_repair(&summary.title));
    let footer_rows = if state.session_picker_busy {
        1
    } else if inner.width >= 78 {
        2
    } else {
        3
    };
    let available_rows = inner
        .height
        .saturating_sub(6 + footer_rows + u16::from(show_name_hint))
        as usize;
    let capacity = (available_rows / 2).max(1);
    let range = crate::tui::selection::ListViewport::default().visible(
        state.selected_session_idx,
        visible_count,
        capacity,
    );
    let offset = chrono::FixedOffset::east_opt(state.session_time_offset_seconds)
        .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).expect("UTC offset is valid"));
    let today = chrono::Utc::now().with_timezone(&offset).date_naive();
    let mut previous_date = None;
    for (visible_index, summary) in visible
        .iter()
        .enumerate()
        .skip(range.start)
        .take(range.len())
    {
        let local = summary.updated_at.with_timezone(&offset);
        let date = local.date_naive();
        if previous_date != Some(date) {
            let category = if date == today {
                "Today".to_string()
            } else {
                local.format("%a %b %-d %Y").to_string()
            };
            items.push(ListItem::new(Span::styled(
                category,
                Style::default()
                    .fg(theme.heading)
                    .add_modifier(Modifier::BOLD),
            )));
            previous_date = Some(date);
        }
        let is_sel = visible_index == state.selected_session_idx;
        let style = if is_sel {
            Style::default()
                .fg(theme.accent_foreground)
                .bg(theme.accent)
        } else {
            Style::default().fg(theme.text)
        };
        let title = if summary.title.trim().is_empty() {
            "Untitled session".to_string()
        } else {
            crate::terminal_text::sanitize_terminal_text(&summary.title)
        };
        let pinned = if summary.pinned { "★ " } else { "" };
        let label = format!("{pinned}{title}");
        let time = local.format("%-I:%M %p").to_string();
        let row_width = inner.width as usize;
        let title_width = row_width.saturating_sub(time.len().saturating_add(1));
        let label = crate::tui::selection::truncate_cells(&label, title_width);
        let label_width = unicode_width::UnicodeWidthStr::width(label.as_str());
        let gap = " ".repeat(row_width.saturating_sub(label_width + time.len()));
        items.push(ListItem::new(Line::from(vec![
            Span::styled(label, style),
            Span::styled(gap, style),
            Span::styled(time, style),
        ])));
    }

    if visible_count == 0 {
        let message = if state.session_filter.is_empty() {
            match state.session_picker_view {
                crate::app::SessionPickerView::Active => "  No recent sessions",
                crate::app::SessionPickerView::Archive => "  No archived sessions",
            }
        } else {
            "  No matching sessions"
        };
        items.push(ListItem::new(Span::styled(
            message,
            Style::default().fg(theme.muted),
        )));
    }

    items.push(ListItem::new(""));
    if show_name_hint && !state.session_picker_busy {
        items.push(ListItem::new(Span::styled(
            "Prompt-like name · Ctrl+G generate a clear name",
            Style::default().fg(theme.heading),
        )));
    }
    if let Some(error) = &state.session_picker_error {
        items.push(ListItem::new(Span::styled(
            crate::terminal_text::sanitize_terminal_text(error),
            Style::default().fg(Color::Red),
        )));
    }
    let footer_style = Style::default().fg(theme.muted);
    if state.session_picker_busy {
        items.push(ListItem::new(Span::styled(
            "Working… · controls locked",
            footer_style,
        )));
    } else {
        if inner.width >= 78 {
            let primary = match state.session_picker_view {
                crate::app::SessionPickerView::Active => {
                    "Enter open  ·  Ctrl+R rename  ·  Ctrl+G generate name"
                }
                crate::app::SessionPickerView::Archive => {
                    "Shift+U restore  ·  Ctrl+R rename  ·  Ctrl+G generate name"
                }
            };
            let secondary = match state.session_picker_view {
                crate::app::SessionPickerView::Active => {
                    "Shift+P pin  ·  Shift+A archive  ·  Shift+D purge  ·  Tab archived"
                }
                crate::app::SessionPickerView::Archive => {
                    "Shift+D purge  ·  Tab recent  ·  Up/Down move  ·  Esc close"
                }
            };
            items.push(ListItem::new(Span::styled(primary, footer_style)));
            items.push(ListItem::new(Span::styled(secondary, footer_style)));
        } else {
            let primary = match state.session_picker_view {
                crate::app::SessionPickerView::Active => "Enter open · Ctrl+R rename",
                crate::app::SessionPickerView::Archive => "Shift+U restore · Ctrl+R rename",
            };
            let tab = match state.session_picker_view {
                crate::app::SessionPickerView::Active => "archived",
                crate::app::SessionPickerView::Archive => "recent",
            };
            items.push(ListItem::new(Span::styled(primary, footer_style)));
            items.push(ListItem::new(Span::styled(
                format!("Ctrl+G generate name · Tab {tab}"),
                footer_style,
            )));
            items.push(ListItem::new(Span::styled(
                "Shift+D purge · Esc close",
                footer_style,
            )));
        }
    }

    frame.render_widget(Clear, pa);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface)),
        pa,
    );
    frame.render_widget(List::new(items), inner);

    if let Some(confirmation) = &state.session_delete_confirmation {
        render_session_delete_confirmation(frame, area, confirmation);
    }
    if let Some(confirmation) = &state.session_archive_confirmation {
        render_session_archive_confirmation(frame, area, confirmation);
    }
    if let Some(rename) = &state.session_rename {
        render_session_rename(frame, area, rename, theme);
    }
}

fn render_session_rename(
    frame: &mut ratatui::Frame,
    area: Rect,
    rename: &crate::app::SessionRenameState,
    theme: ThemePalette,
) {
    let width = 72.min(area.width.saturating_sub(4));
    let height = 9.min(area.height.saturating_sub(4));
    let modal = centered_rect(width, height, area);
    let value_width = width.saturating_sub(6) as usize;
    let safe_title = crate::terminal_text::sanitize_terminal_text(&rename.title);
    let safe_cursor = floor_render_char_boundary(&safe_title, rename.cursor.min(safe_title.len()));
    let (before, after) = editor_value_window(&safe_title, safe_cursor, value_width);
    let mut lines = vec![
        Line::from(Span::styled(
            "Rename Session",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(theme.accent)),
            Span::styled(before, Style::default().fg(theme.text)),
            Span::styled(
                "▌",
                Style::default()
                    .fg(theme.accent_foreground)
                    .bg(theme.accent),
            ),
            Span::styled(after, Style::default().fg(theme.text)),
        ]),
        Line::from(""),
    ];
    if let Some(error) = &rename.error {
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(error),
            Style::default().fg(theme.error),
        )));
    }
    lines.push(Line::from(Span::styled(
        if rename.busy {
            "Saving…"
        } else {
            "Enter save · Esc cancel · ←→ move · Home/End"
        },
        Style::default().fg(theme.muted),
    )));

    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .style(Style::default().bg(theme.surface)),
        ),
        modal,
    );
}

fn render_session_portability(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(portability) = &state.session_portability else {
        return;
    };
    let width = 68.min(area.width.saturating_sub(4));
    let height = if portability.import_preview.is_some() {
        13
    } else if portability.error.is_some() {
        11
    } else {
        9
    }
    .min(area.height.saturating_sub(4));
    let modal = centered_rect(width, height, area);
    let path_width = width.saturating_sub(6) as usize;
    let safe_path =
        crate::terminal_text::sanitize_terminal_text(&portability.path.to_string_lossy());
    let safe_path = truncate_middle_chars(&safe_path, path_width.max(1));
    let mut lines = Vec::new();

    if portability.awaiting_overwrite {
        lines.push(Line::from(Span::styled(
            "Replace existing export?",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(safe_path));
        lines.push(Line::from("The previous file is replaced atomically."));
        lines.push(Line::from(""));
        let cancel_style = if portability.overwrite_selected {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        };
        let replace_style = if portability.overwrite_selected {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(vec![
            Span::styled(" Cancel ", cancel_style),
            Span::raw("   "),
            Span::styled(" Replace ", replace_style),
        ]));
        lines.push(Line::from(Span::styled(
            "←→ choose · Enter confirm · Esc cancel",
            Style::default().fg(Color::DarkGray),
        )));
    } else if let Some(error) = &portability.error {
        let operation = match portability.operation {
            crate::app::SessionPortabilityOperation::Export { .. } => "Session export failed",
            crate::app::SessionPortabilityOperation::Import => "Session import failed",
            crate::app::SessionPortabilityOperation::ForeignImport { .. } => {
                "Foreign session import failed"
            }
        };
        lines.push(Line::from(Span::styled(
            operation,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(safe_path));
        lines.push(Line::from(""));
        let safe_error = crate::terminal_text::sanitize_terminal_text(error);
        lines.push(Line::from(Span::styled(
            truncate(&safe_error, path_width.max(1)),
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Enter or Esc close",
            Style::default().fg(Color::DarkGray),
        )));
    } else if let Some(preview) = &portability.import_preview {
        lines.push(Line::from(Span::styled(
            format!("{} import preview", preview.source.label()),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "Source: {} · {} file{} · {}",
            preview.source_label,
            preview.source_files,
            if preview.source_files == 1 { "" } else { "s" },
            format_import_bytes(preview.source_bytes)
        )));
        lines.push(Line::from(format!(
            "Importable: {} session{} · {} visible message{}",
            preview.sessions,
            if preview.sessions == 1 { "" } else { "s" },
            preview.messages,
            if preview.messages == 1 { "" } else { "s" }
        )));
        lines.push(Line::from(format!(
            "Skipped: {} item{} · errors: {}",
            preview.skipped_items,
            if preview.skipped_items == 1 { "" } else { "s" },
            preview.errors.len()
        )));
        lines.push(Line::from(format!(
            "Destination profile: {}",
            state.active_profile.label()
        )));
        let learning = if portability.import_learning_eligible {
            "on (explicit opt-in)"
        } else {
            "off (default)"
        };
        lines.push(Line::from(format!("Automatic learning: {learning}")));
        if let Some(error) = preview.errors.first() {
            lines.push(Line::from(Span::styled(
                truncate(error, path_width.max(1)),
                Style::default().fg(Color::Yellow),
            )));
        } else {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(""));
        let cancel_style = if portability.import_selected {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        };
        let import_style = if portability.import_selected && preview.sessions > 0 {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else if preview.sessions > 0 {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::styled(" Cancel ", cancel_style),
            Span::raw("   "),
            Span::styled(" Import ", import_style),
        ]));
        lines.push(Line::from(Span::styled(
            "←→ choose · L learning · Enter confirm · Esc cancel",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let (title, detail) = match portability.operation {
            crate::app::SessionPortabilityOperation::Export { format, .. } => (
                "Exporting session…",
                match format {
                    crate::session_portability::ExportFormat::Markdown => "Bounded Markdown",
                    crate::session_portability::ExportFormat::Json => "Portable JSON",
                },
            ),
            crate::app::SessionPortabilityOperation::Import => {
                ("Importing session…", "Validating inert portable history")
            }
            crate::app::SessionPortabilityOperation::ForeignImport { source, commit, .. } => (
                if commit {
                    "Importing foreign sessions…"
                } else {
                    "Inspecting foreign history…"
                },
                source.label(),
            ),
        };
        lines.push(Line::from(Span::styled(
            title,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(safe_path));
        lines.push(Line::from(detail));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Controls locked while the file is checked",
            Style::default().fg(Color::DarkGray),
        )));
    }

    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(
            Block::default()
                .title(" Session portability ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        modal,
    );
}

fn format_import_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn render_session_retention(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(retention) = &state.session_retention else {
        return;
    };
    let width = 72.min(area.width.saturating_sub(4));
    let height = 22.min(area.height.saturating_sub(4));
    let modal = centered_rect(width, height, area);
    let inner_width = width.saturating_sub(4) as usize;
    let mut lines = vec![Line::from(Span::styled(
        "Saved session retention",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ))];

    match retention.mode {
        crate::app::SessionRetentionMode::Overview => {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Age rule       ", Style::default().fg(Color::DarkGray)),
                Span::raw(retention.settings.days.map_or_else(
                    || "disabled".into(),
                    |days| format!("delete older than {days} days"),
                )),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Count rule     ", Style::default().fg(Color::DarkGray)),
                Span::raw(retention.settings.count.map_or_else(
                    || "disabled".into(),
                    |count| format!("keep newest {count} eligible sessions"),
                )),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(format!(
                "Active {} · Archived {} (always excluded)",
                retention.active_count, retention.archived_count
            )));
            lines.push(Line::from(Span::styled(
                "Current and pinned sessions are always protected.",
                Style::default().fg(Color::DarkGray),
            )));
            if !retention.settings.enabled() {
                lines.push(Line::from(Span::styled(
                    "Retention is disabled. Nothing runs automatically.",
                    Style::default().fg(Color::Yellow),
                )));
            }
            lines.push(Line::from(""));
            let footer = if !retention.writes_enabled {
                "View only in bare mode · Esc close"
            } else if !retention.sessions_enabled {
                "Shift+E edit for future runs · Esc close"
            } else {
                "Enter preview · Shift+E edit · Esc close"
            };
            lines.push(Line::from(Span::styled(
                footer,
                Style::default().fg(Color::DarkGray),
            )));
        }
        crate::app::SessionRetentionMode::Edit => {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Blank disables a rule. Saving never deletes sessions.",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));
            for (field, label, value, suffix) in [
                (
                    crate::app::SessionRetentionField::Days,
                    "Delete older than",
                    retention.days_input.as_str(),
                    "days",
                ),
                (
                    crate::app::SessionRetentionField::Count,
                    "Keep newest",
                    retention.count_input.as_str(),
                    "eligible sessions",
                ),
            ] {
                let selected = retention.field == field;
                let display = if value.is_empty() { "disabled" } else { value };
                let content = format!(" {label:<18} {display:<10} {suffix}");
                lines.push(Line::from(Span::styled(
                    pad_right(&content, inner_width),
                    if selected {
                        Style::default().fg(Color::Black).bg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::White)
                    },
                )));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Tab/↑↓ field · Enter save · Esc cancel",
                Style::default().fg(Color::DarkGray),
            )));
        }
        crate::app::SessionRetentionMode::Preview => {
            let preview = retention.preview.as_ref();
            let candidate_count = preview.map_or(0, |preview| preview.candidates.len());
            lines.push(Line::from(""));
            lines.push(Line::from(format!(
                "Exact preview · {candidate_count} permanent deletion{}",
                if candidate_count == 1 { "" } else { "s" }
            )));
            if let Some(preview) = preview {
                lines.push(Line::from(format!(
                    "Protected: current {} · pinned {} · archived {}",
                    preview.protected_current, preview.protected_pinned, retention.archived_count
                )));
                lines.push(Line::from(""));
                if preview.candidates.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "No active sessions match these rules.",
                        Style::default().fg(Color::DarkGray),
                    )));
                } else {
                    let visible_rows = 8usize;
                    let start = retention
                        .preview_selected_idx
                        .saturating_sub(visible_rows.saturating_sub(1));
                    for (index, candidate) in preview
                        .candidates
                        .iter()
                        .enumerate()
                        .skip(start)
                        .take(visible_rows)
                    {
                        let selected = index == retention.preview_selected_idx;
                        let reason = match candidate.reason {
                            crate::session_retention::RetentionReason::Age => "age",
                            crate::session_retention::RetentionReason::Count => "count",
                            crate::session_retention::RetentionReason::Both => "age+count",
                        };
                        let title = if candidate.title.is_empty() {
                            "Untitled session".into()
                        } else {
                            crate::terminal_text::sanitize_terminal_text(&candidate.title)
                        };
                        let row = format!(
                            "{} {} · {} · {reason}",
                            if selected { "▸" } else { " " },
                            truncate_middle_chars(&title, inner_width.saturating_sub(25).max(8)),
                            candidate.updated_at.format("%Y-%m-%d")
                        );
                        lines.push(Line::from(Span::styled(
                            pad_right(&row, inner_width),
                            if selected {
                                Style::default().fg(Color::Black).bg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::White)
                            },
                        )));
                    }
                }
            }
            lines.push(Line::from(""));
            let cancel_style = if !retention.delete_selected {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            let delete_style = if retention.delete_selected && candidate_count > 0 {
                Style::default().fg(Color::White).bg(Color::Red)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(vec![
                Span::styled(" Cancel ", cancel_style),
                Span::raw("   "),
                Span::styled(" Delete permanently ", delete_style),
            ]));
            lines.push(Line::from(Span::styled(
                "←→ choose · Enter confirm · Esc cancel · ↑↓ inspect",
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    if let Some(busy) = retention.busy {
        let label = match busy {
            crate::app::SessionRetentionBusy::Loading => "Loading saved sessions…",
            crate::app::SessionRetentionBusy::Saving => "Saving retention settings…",
            crate::app::SessionRetentionBusy::Deleting => "Deleting confirmed sessions…",
        };
        lines.push(Line::from(Span::styled(
            format!("{label} · controls locked"),
            Style::default().fg(Color::Yellow),
        )));
    }
    if let Some(notice) = &retention.notice {
        lines.push(Line::from(Span::styled(
            truncate_middle_chars(
                &crate::terminal_text::sanitize_terminal_text(notice),
                inner_width,
            ),
            Style::default().fg(Color::Green),
        )));
    }
    if let Some(error) = &retention.error {
        lines.push(Line::from(Span::styled(
            truncate_middle_chars(
                &crate::terminal_text::sanitize_terminal_text(error),
                inner_width,
            ),
            Style::default().fg(Color::Red),
        )));
    }

    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        modal,
    );
}

fn render_session_archive_confirmation(
    frame: &mut ratatui::Frame,
    area: Rect,
    confirmation: &crate::app::SessionArchiveConfirmation,
) {
    let width = 58.min(area.width.saturating_sub(4));
    let height =
        if confirmation.error.is_some() { 12 } else { 10 }.min(area.height.saturating_sub(4));
    let modal = centered_rect(width, height, area);
    let safe_title =
        crate::terminal_text::sanitize_terminal_text(if confirmation.title.is_empty() {
            "Untitled session"
        } else {
            &confirmation.title
        });
    let mut lines = vec![
        Line::from(Span::styled(
            "Archive saved session?",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!(
            "{safe_title} · {}",
            &confirmation.session_id.to_string()[..8]
        )),
        Line::from("This moves exactly this session into the archive."),
        Line::from("Branches stay active; you can restore this session later."),
        Line::from(""),
    ];
    if let Some(error) = &confirmation.error {
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(error),
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(""));
    }
    let (cancel_style, archive_style) = if confirmation.busy {
        (
            Style::default().fg(Color::DarkGray),
            Style::default().fg(Color::DarkGray),
        )
    } else if confirmation.archive_selected {
        (
            Style::default().fg(Color::White),
            Style::default().fg(Color::Black).bg(Color::Yellow),
        )
    } else {
        (
            Style::default().fg(Color::Black).bg(Color::Cyan),
            Style::default().fg(Color::White),
        )
    };
    lines.push(Line::from(vec![
        Span::styled(" Cancel ", cancel_style),
        Span::raw("   "),
        Span::styled(
            if confirmation.busy {
                " Archiving… "
            } else {
                " Archive "
            },
            archive_style,
        ),
    ]));
    lines.push(Line::from(Span::styled(
        if confirmation.busy {
            "Please wait"
        } else {
            "←/→ choose · Enter confirm · Esc cancel"
        },
        Style::default().fg(Color::DarkGray),
    )));

    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .wrap(Wrap { trim: false }),
        modal,
    );
}

fn render_session_delete_confirmation(
    frame: &mut ratatui::Frame,
    area: Rect,
    confirmation: &crate::app::SessionDeleteConfirmation,
) {
    let width = 58.min(area.width.saturating_sub(4));
    let height =
        if confirmation.error.is_some() { 12 } else { 10 }.min(area.height.saturating_sub(4));
    let modal = centered_rect(width, height, area);
    let safe_title =
        crate::terminal_text::sanitize_terminal_text(if confirmation.title.is_empty() {
            "Untitled session"
        } else {
            &confirmation.title
        });
    let mut lines = vec![
        Line::from(Span::styled(
            if confirmation.archived {
                "Purge archived session?"
            } else {
                "Purge saved session?"
            },
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!(
            "{safe_title} · {}",
            &confirmation.session_id.to_string()[..8]
        )),
        Line::from(if confirmation.archived {
            "This permanently removes exactly this archived session."
        } else {
            "This permanently removes exactly this saved session."
        }),
        Line::from("Branches and their history are not deleted."),
        Line::from(""),
    ];
    if let Some(error) = &confirmation.error {
        lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(error),
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(""));
    }
    let (cancel_style, delete_style) = if confirmation.busy {
        (
            Style::default().fg(Color::DarkGray),
            Style::default().fg(Color::DarkGray),
        )
    } else if confirmation.delete_selected {
        (
            Style::default().fg(Color::White),
            Style::default().fg(Color::Black).bg(Color::Red),
        )
    } else {
        (
            Style::default().fg(Color::Black).bg(Color::Cyan),
            Style::default().fg(Color::White),
        )
    };
    lines.push(Line::from(vec![
        Span::styled(" Cancel ", cancel_style),
        Span::raw("   "),
        Span::styled(
            if confirmation.busy {
                " Purging… "
            } else {
                " Purge "
            },
            delete_style,
        ),
    ]));
    lines.push(Line::from(Span::styled(
        if confirmation.busy {
            "Please wait"
        } else {
            "←/→ choose · Enter confirm · Esc cancel"
        },
        Style::default().fg(Color::DarkGray),
    )));

    frame.render_widget(Clear, modal);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red)),
            )
            .wrap(Wrap { trim: false }),
        modal,
    );
}

fn render_timeline(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let pw = 72.min(area.width.saturating_sub(4));
    let ph = 20.min(area.height.saturating_sub(4));
    let pa = centered_rect(pw, ph, area);
    let title = if state.timeline_mode == crate::app::TimelineMode::Rewind {
        " Rewind by creating a branch"
    } else {
        " Conversation timeline"
    };
    let mut items = vec![
        ListItem::new(Span::styled(
            title,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        ListItem::new(Span::styled(
            " Source history will not be changed",
            Style::default().fg(Color::DarkGray),
        )),
        ListItem::new(""),
    ];
    for (index, entry) in state.timeline_entries.iter().enumerate() {
        let selected = index == state.selected_timeline_idx;
        let style = if selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if selected { "▸" } else { " " };
        items.push(ListItem::new(Span::styled(
            format!(
                " {prefix} Turn {} · {} · {}",
                entry.turn_index,
                entry.completed_at.format("%Y-%m-%d %H:%M"),
                entry.title
            ),
            style,
        )));
    }
    if state.timeline_entries.is_empty() {
        items.push(ListItem::new(Span::styled(
            "  No completed turns",
            Style::default().fg(Color::DarkGray),
        )));
    }
    items.push(ListItem::new(""));
    items.push(ListItem::new(Span::styled(
        "Enter branch here · Esc close · ↑↓ navigate",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Clear, pa);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(List::new(items).block(block), pa);
}

fn render_queue_view(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let pw = area.width;
    let pa = area;
    let mut items = vec![
        ListItem::new(Span::styled(
            " Queued messages",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        ListItem::new(""),
    ];
    for (index, pending) in state.input_queue.items().enumerate() {
        let selected = index == state.input_queue.selected_index;
        let style = if selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if selected { "▸" } else { " " };
        items.push(ListItem::new(Span::styled(
            pad_right(
                &format!(
                    " {prefix} {} · {}",
                    if state.restored_queue_ids.contains(&pending.id) {
                        "On hold"
                    } else {
                        "Queued"
                    },
                    pending.text.replace('\n', " ")
                ),
                pw.saturating_sub(2) as usize,
            ),
            style,
        )));
    }
    if state.input_queue.is_empty() {
        items.push(ListItem::new(Span::styled(
            "  Queue is empty",
            Style::default().fg(Color::DarkGray),
        )));
    }
    items.push(ListItem::new(""));
    let hint = if pw < 68 {
        "Enter edit · Del remove · Shift+↑↓ move · Esc"
    } else {
        "Enter/e edit · Delete/d remove · Shift+↑↓ reorder · Esc close"
    };
    items.push(ListItem::new(Span::styled(
        hint,
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Clear, pa);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(List::new(items).block(block), pa);
}

fn centered_rect(w: u16, h: u16, r: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((r.height.saturating_sub(h)) / 2),
            Constraint::Length(h),
            Constraint::Length((r.height.saturating_sub(h)) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((r.width.saturating_sub(w)) / 2),
            Constraint::Length(w),
            Constraint::Length((r.width.saturating_sub(w)) / 2),
        ])
        .split(v[1])[1]
}

fn render_interaction(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(interaction) = state.interaction.as_ref() else {
        return;
    };
    let width = area.width.saturating_sub(4).clamp(20, 72);
    let requested_height = (10 + interaction.visible_count() as u16).min(22);
    let height = area.height.saturating_sub(2).min(requested_height).max(8);
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);

    let learned_recipe = state.learned_recipe_review.is_some();
    let self_improve = state.self_improve_review.is_some();
    let learning_review = state.learning_review_interaction;
    let accent = if learned_recipe || self_improve || learning_review {
        Color::Rgb(194, 226, 164)
    } else {
        Color::Yellow
    };
    let selected_accent = if learned_recipe || self_improve || learning_review {
        Color::Rgb(194, 226, 164)
    } else {
        Color::Cyan
    };
    let mut lines = vec![Line::from(Span::styled(
        interaction.request.question.clone(),
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    ))];
    if let Some(description) = interaction.request.description.as_deref() {
        lines.push(Line::from(Span::styled(
            description,
            Style::default().fg(Color::DarkGray),
        )));
    }
    if interaction.visible_count() > 0 && !interaction.other_editing() {
        lines.push(Line::from(""));
        for (index, choice) in interaction.request.choices.iter().enumerate() {
            let selected = index == interaction.selected_index;
            let checked =
                if interaction.request.mode == crate::interaction::InteractionMode::MultiSelect {
                    if interaction.is_selected(index) {
                        "[x]"
                    } else {
                        "[ ]"
                    }
                } else if selected {
                    "(•)"
                } else {
                    "( )"
                };
            let style = if selected {
                Style::default().fg(selected_accent)
            } else {
                Style::default().fg(Color::Gray)
            };
            lines.push(Line::from(Span::styled(
                format!("  {} {}. {}", checked, index + 1, choice.label),
                style,
            )));
            if let Some(description) = choice.description.as_deref() {
                lines.push(Line::from(Span::styled(
                    format!("      {description}"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
        if interaction.request.allow_other {
            let index = interaction.request.choices.len();
            let selected = index == interaction.selected_index;
            lines.push(Line::from(Span::styled(
                format!(
                    "  {} {}. Other",
                    if selected { "(•)" } else { "( )" },
                    index + 1
                ),
                if selected {
                    Style::default().fg(selected_accent)
                } else {
                    Style::default().fg(Color::Gray)
                },
            )));
        }
    }

    if interaction.visible_count() == 0 || interaction.other_editing() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            if interaction.request.mode == crate::interaction::InteractionMode::Masked {
                "  Secret:"
            } else if interaction.other_editing() {
                "  Other:"
            } else {
                "  Answer:"
            },
            Style::default().fg(Color::Cyan),
        )));
        lines.push(Line::from(Span::styled(
            format!("  {}", interaction.draft_display()),
            Style::default().fg(Color::White),
        )));
    }

    lines.push(Line::from(""));
    let hint = if interaction.request.mode == crate::interaction::InteractionMode::Confirmation
        && !interaction.other_editing()
    {
        "y yes · n no · Enter submit · Esc cancel"
    } else if interaction.request.mode == crate::interaction::InteractionMode::MultiSelect
        && !interaction.other_editing()
    {
        "↑/↓ move · Space toggle · Enter submit · Esc cancel"
    } else if interaction.visible_count() > 0 && !interaction.other_editing() {
        "↑/↓ or 1-9 choose · Enter submit · Esc cancel"
    } else {
        "Enter submit · Esc cancel"
    };
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .title(if learned_recipe {
            " Learned Rust Recipe "
        } else if self_improve {
            " Autonomous Learning "
        } else if learning_review {
            " Learning Review "
        } else {
            " Input required "
        })
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent));
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}

fn render_diff_lines(output: &str, width: u16, lines: &mut Vec<Line>, theme: &ThemePalette) {
    let max = width.saturating_sub(4) as usize;
    for line in output.lines().take(30) {
        let (style, prefix) = if line.starts_with("+++") || line.starts_with("---") {
            (
                Style::default()
                    .fg(theme.diff_file_header_fg)
                    .add_modifier(Modifier::BOLD),
                line,
            )
        } else if line.starts_with('+') {
            (Style::default().fg(theme.diff_insert_fg), line)
        } else if line.starts_with('-') {
            (Style::default().fg(theme.diff_delete_fg), line)
        } else if line.starts_with("@@") {
            (Style::default().fg(theme.diff_hunk_fg), line)
        } else {
            (Style::default().fg(theme.text), line)
        };
        lines.push(Line::from(Span::styled(truncate(prefix, max), style)));
    }
    let count = output.lines().count();
    if count > 30 {
        lines.push(Line::from(Span::styled(
            format!("… ({} more diff lines)", count - 30),
            Style::default().fg(Color::DarkGray),
        )));
    }
}

fn pad_right(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.chars().take(width).collect()
    } else {
        let mut out = s.to_string();
        out.push_str(&" ".repeat(width - len));
        out
    }
}

fn wrap_plan_text(text: &str, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let used = unicode_width::UnicodeWidthStr::width(line.as_str());
        let word_width = unicode_width::UnicodeWidthStr::width(word);
        if !line.is_empty() && used + 1 + word_width > width {
            rows.push(std::mem::take(&mut line));
        }
        if word_width > width {
            let mut pieces = crate::tui::output_layout::wrap_cells(word, width);
            line = pieces.pop().unwrap_or_default();
            rows.extend(pieces);
        } else {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
    }
    if !line.is_empty() || rows.is_empty() {
        rows.push(line);
    }
    rows
}

fn render_plan_card(lines: &mut Vec<Line<'static>>, state: &AppState, width: u16) {
    let work = state
        .work_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let Some(plan) = work.plan.as_ref() else {
        return;
    };
    let inner = usize::from(width.saturating_sub(6).max(1));
    let color = palette(state).plan;
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("  ╭{}╮", "─".repeat(inner + 2)),
        Style::default().fg(color),
    )));
    let mut row = |text: &str, style: Style| {
        let safe = crate::terminal_text::sanitize_terminal_text(text);
        for part in wrap_plan_text(&safe, inner) {
            let padding = " "
                .repeat(inner.saturating_sub(unicode_width::UnicodeWidthStr::width(part.as_str())));
            lines.push(Line::from(vec![
                Span::styled("  │ ", Style::default().fg(color)),
                Span::styled(part, style),
                Span::raw(padding),
                Span::styled(" │", Style::default().fg(color)),
            ]));
        }
    };
    row(
        &format!("Plan · {}", plan.display_status()),
        Style::default().fg(color),
    );
    row(&plan.title, Style::default().add_modifier(Modifier::BOLD));
    if let Some(summary) = plan.body.as_deref().and_then(|body| {
        body.lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#'))
    }) {
        row(summary, Style::default());
    }
    row("", Style::default());
    for (index, task) in plan.tasks.iter().enumerate() {
        let status = match task.status {
            crate::work::TaskStatus::Pending => "",
            crate::work::TaskStatus::InProgress => " [in progress]",
            crate::work::TaskStatus::Completed => " [done]",
            crate::work::TaskStatus::Blocked => " [blocked]",
        };
        row(
            &format!("{}. {}{}", index + 1, task.title, status),
            Style::default(),
        );
    }
    row("", Style::default());
    if state.plan_proposal.is_some() {
        row(
            "Enter Execute · Ctrl+O View plan · /plan graph",
            Style::default().fg(color),
        );
        row(
            "Type to Revise · Esc Dismiss · /plan send",
            Style::default().fg(color),
        );
    } else {
        row("/plan show to review this plan", Style::default().fg(color));
    }
    lines.push(Line::from(Span::styled(
        format!("  ╰{}╯", "─".repeat(inner + 2)),
        Style::default().fg(color),
    )));
    lines.push(Line::from(""));
}

fn render_plan_reader(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Plan ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette(state).plan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let content_area = Rect {
        height: inner.height.saturating_sub(3),
        ..inner
    };
    let rendered = render_safe_markdown_document(
        &state.pager_content,
        content_area.width.max(1),
        false,
        palette(state),
    );
    let paragraph = Paragraph::new(rendered.lines).wrap(Wrap { trim: false });
    let total = paragraph.line_count(content_area.width.max(1));
    let maximum = total
        .saturating_sub(usize::from(content_area.height))
        .min(usize::from(u16::MAX)) as u16;
    state.plan_reader_max_scroll.set(maximum);
    let scroll = state.pager_scroll.min(maximum);
    frame.render_widget(paragraph.scroll((scroll, 0)), content_area);
    let hint_area = Rect::new(
        inner.x,
        content_area.bottom(),
        inner.width,
        inner.height.saturating_sub(content_area.height),
    );
    let actions = if crate::plan::selected_review(state).is_err() {
        "Plan changed. Close and use /plan show to refresh."
    } else {
        "Enter Execute · R Revise · S Send · N comment selection · Esc close · C copy all"
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                actions,
                Style::default().fg(palette(state).plan),
            )),
            Line::from(format!(
                "↑/↓ scroll · PgUp/PgDn page · {}/{}",
                scroll.saturating_add(1),
                maximum.saturating_add(1)
            )),
        ])
        .wrap(Wrap { trim: false }),
        hint_area,
    );
    state.pager_selection.capture_selectable_frame_at(
        frame.buffer_mut(),
        content_area,
        u32::from(scroll),
    );
    state.pager_selection.render_selection(frame.buffer_mut());
}

fn render_pager(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if state.plan_reader.is_some() {
        render_plan_reader(frame, area, state);
        return;
    }
    let ph = area.height;
    let pa = area;

    frame.render_widget(Clear, pa);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let scroll = state.pager_scroll as usize;
    let inner_width = pa.width.saturating_sub(2);
    let document_top = Paragraph::new(Text::from(
        state
            .pager_content
            .lines()
            .take(scroll)
            .map(|line| Line::from(line.to_string()))
            .collect::<Vec<_>>(),
    ))
    .wrap(Wrap { trim: false })
    .line_count(inner_width)
    .min(u32::MAX as usize) as u32;
    let visible_height = ph.saturating_sub(3) as usize;
    let total = state.pager_content.lines().count();
    let lines: Vec<Line> = state
        .pager_content
        .lines()
        .skip(scroll)
        .take(visible_height)
        .map(|line| context_grid_pager_line(line).unwrap_or_else(|| Line::from(line.to_string())))
        .collect();
    let content_rows = Paragraph::new(Text::from(lines.clone()))
        .wrap(Wrap { trim: false })
        .line_count(pa.width.saturating_sub(2))
        .min(u16::MAX as usize) as u16;

    let escape = if state.mcp_task_active {
        "Esc cancel"
    } else {
        "q/Esc close"
    };
    let actions = if state.learning_review_pager_open {
        if state.learning_review_busy {
            Some("working · C copy all")
        } else if state.learning_review_view.as_ref().is_some_and(|view| {
            view.kind == crate::codex_learning::TuiLearningViewKind::Proposal
                && view.proposal.is_some()
        }) {
            Some("y apply · n leave pending · C copy all")
        } else {
            Some("Enter actions · R refresh · C copy all")
        }
    } else {
        None
    };
    let hint = if let Some(actions) = actions {
        format!(
            " {actions} · j/k ↓↑ · g/G top/bottom · space/f page · {escape} | {}/{}",
            (scroll + 1).min(total),
            total
        )
    } else {
        format!(
            " j/k ↓↑ · g/G top/bottom · space/f page · {escape} | {}/{}",
            (scroll + 1).min(total),
            total
        )
    };
    let mut all = lines;
    all.push(Line::from(""));
    all.push(Line::from(Span::styled(
        hint,
        Style::default().fg(Color::DarkGray),
    )));

    frame.render_widget(
        Paragraph::new(Text::from(all))
            .block(block)
            .wrap(Wrap { trim: false }),
        pa,
    );
    let selectable = Rect::new(
        pa.x.saturating_add(1),
        pa.y.saturating_add(1),
        pa.width.saturating_sub(2),
        content_rows.min(pa.height.saturating_sub(2)),
    );
    state
        .pager_selection
        .capture_selectable_frame_at(frame.buffer_mut(), selectable, document_top);
    state.pager_selection.render_selection(frame.buffer_mut());
}

fn context_grid_pager_line(line: &str) -> Option<Line<'static>> {
    let glyphs = line.strip_prefix('│')?.strip_suffix('│')?;
    if glyphs.chars().count() != crate::context::CONTEXT_GRID_WIDTH
        || !glyphs
            .chars()
            .all(|glyph| crate::context::CONTEXT_GRID_GLYPHS.contains(&glyph) || glyph == '·')
    {
        return None;
    }
    let mut spans = Vec::with_capacity(crate::context::CONTEXT_GRID_WIDTH + 2);
    spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
    spans.extend(
        glyphs
            .chars()
            .map(|glyph| Span::styled(glyph.to_string(), context_grid_style(glyph))),
    );
    spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
    Some(Line::from(spans))
}

pub(crate) fn context_grid_style(glyph: char) -> Style {
    let color = match glyph {
        'I' => Color::Cyan,
        'S' => Color::Magenta,
        'B' => Color::Yellow,
        'M' => Color::Green,
        'K' => Color::LightBlue,
        'C' => Color::White,
        'A' => Color::LightRed,
        _ => Color::DarkGray,
    };
    Style::default().fg(color)
}

fn render_file_picker(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let palette = palette(state);
    let narrow = area.width < 80;
    let max_entries = if narrow { 5 } else { 8 };
    let status = match state.file_picker_phase {
        crate::attachments::PickerPhase::Indexing => Some("Indexing workspace files…".to_string()),
        crate::attachments::PickerPhase::Matching => Some("Ranking matches…".to_string()),
        crate::attachments::PickerPhase::Failed => state
            .file_picker_error
            .clone()
            .or_else(|| Some("Attachment search failed".into())),
        crate::attachments::PickerPhase::Idle | crate::attachments::PickerPhase::Ready => None,
    };
    let selected = state
        .selected_file_idx
        .min(state.file_picker_entries.len().saturating_sub(1));
    let visible = crate::tui::selection::ListViewport::default().visible(
        selected,
        state.file_picker_entries.len(),
        max_entries,
    );
    let entry_height = if narrow { 2 } else { 1 };
    let content_height = visible.len() * entry_height + usize::from(status.is_some());
    let pa = composer_popup_rect(area, state, (content_height.max(1) + 2) as u16, 72);
    let mut lines = Vec::new();
    if let Some(status) = status {
        lines.push(Line::from(Span::styled(
            format!("  {status}"),
            Style::default().fg(palette.warning),
        )));
    }
    for (index, entry) in state
        .file_picker_entries
        .iter()
        .enumerate()
        .skip(visible.start)
        .take(visible.len())
    {
        let is_sel = index == selected;
        let style = if is_sel {
            Style::default()
                .fg(palette.accent_foreground)
                .bg(palette.accent)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_sel { "▸ " } else { "  " };
        lines.push(Line::from(Span::styled(
            crate::tui::selection::truncate_cells(
                &format!("{}{}", prefix, entry.display()),
                pa.width.saturating_sub(2) as usize,
            ),
            style,
        )));
        if narrow {
            let description = match entry {
                crate::attachments::PickerEntry::File(file) if file.read_only => {
                    "    workspace file · read-only root"
                }
                crate::attachments::PickerEntry::File(_) => "    workspace file",
                crate::attachments::PickerEntry::Mcp(_) => "    MCP resource",
            };
            lines.push(Line::from(Span::styled(
                description,
                if is_sel {
                    style
                } else {
                    Style::default().fg(palette.muted)
                },
            )));
        }
    }
    if state.file_picker_entries.is_empty()
        && matches!(
            state.file_picker_phase,
            crate::attachments::PickerPhase::Ready
        )
    {
        lines.push(Line::from(Span::styled(
            "  No attachment matches",
            Style::default().fg(palette.muted),
        )));
    }
    if let Some(error) = state
        .file_picker_error
        .as_deref()
        .filter(|_| state.file_picker_phase != crate::attachments::PickerPhase::Failed)
    {
        lines.push(Line::from(Span::styled(
            format!("  {error}"),
            Style::default().fg(palette.error),
        )));
    }
    let cap = if state.file_picker_truncated {
        " · index capped"
    } else {
        ""
    };
    frame.render_widget(Clear, pa);
    let block = Block::default()
        .title(format!(
            " Attachments {}/{} · {} indexed{cap} ",
            if state.file_picker_entries.is_empty() {
                0
            } else {
                selected + 1
            },
            state.file_picker_matches,
            state.file_picker_discovered,
        ))
        .title_bottom(Line::from(Span::styled(
            " ↑↓ select · Enter/Tab attach · Esc dismiss ",
            Style::default().fg(palette.muted),
        )))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.accent));
    frame.render_widget(Paragraph::new(lines).block(block), pa);
}

fn render_path_completion(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let palette = palette(state);
    let completion = &state.composer_ui.path_completion;
    let narrow = area.width < 80;
    let maximum = if narrow { 5 } else { 8 };
    let selected = completion
        .selected
        .min(completion.entries.len().saturating_sub(1));
    let visible = crate::tui::selection::ListViewport::default().visible(
        selected,
        completion.entries.len(),
        maximum,
    );
    let status_rows = usize::from(completion.loading || completion.error.is_some());
    let entry_height = if narrow { 2 } else { 1 };
    let entry_rows = if completion.entries.is_empty() {
        1
    } else {
        visible.len() * entry_height
    };
    let pa = composer_popup_rect(area, state, (entry_rows + status_rows + 2) as u16, 72);
    let mut lines = Vec::new();
    if completion.loading {
        lines.push(Line::from(Span::styled(
            "  Listing parent directory…",
            Style::default().fg(palette.warning),
        )));
    } else if let Some(error) = &completion.error {
        lines.push(Line::from(Span::styled(
            crate::tui::selection::truncate_cells(error, pa.width.saturating_sub(2) as usize),
            Style::default().fg(palette.error),
        )));
    }
    for (index, entry) in completion
        .entries
        .iter()
        .enumerate()
        .skip(visible.start)
        .take(visible.len())
    {
        let active = index == selected;
        let style = if active {
            Style::default()
                .fg(palette.accent_foreground)
                .bg(palette.accent)
        } else {
            Style::default().fg(Color::White)
        };
        let kind = if entry.is_dir { "folder" } else { "file" };
        let prefix = if active { "▸ " } else { "  " };
        let row = crate::tui::selection::layout_row(
            &format!("{prefix}{}", entry.display),
            kind,
            pa.width.saturating_sub(2) as usize,
            narrow,
        );
        if row.stacked {
            lines.push(Line::from(Span::styled(row.label, style)));
            lines.push(Line::from(Span::styled(
                row.description,
                if active {
                    style
                } else {
                    Style::default().fg(palette.muted)
                },
            )));
        } else {
            lines.push(Line::from(vec![
                Span::styled(row.label, style),
                Span::styled(row.gap, style),
                Span::styled(
                    row.description,
                    if active {
                        style
                    } else {
                        Style::default().fg(palette.muted)
                    },
                ),
            ]));
        }
    }
    if !completion.loading && completion.error.is_none() && completion.entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No path matches",
            Style::default().fg(palette.muted),
        )));
    }
    frame.render_widget(Clear, pa);
    let block = Block::default()
        .title(format!(
            " Paths {}/{} ",
            if completion.entries.is_empty() {
                0
            } else {
                selected + 1
            },
            completion.entries.len()
        ))
        .title_bottom(Line::from(Span::styled(
            " ↑↓ select · Enter/Tab complete · Esc dismiss ",
            Style::default().fg(palette.muted),
        )))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.accent));
    frame.render_widget(Paragraph::new(lines).block(block), pa);
}

fn render_change_review(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    use crate::changes::{ChangeReviewPurpose, ChangeViewMode, GitReviewOperation};

    frame.render_widget(Clear, area);
    let title_prefix = match state.change_review.purpose {
        ChangeReviewPurpose::Inspect => "Working tree changes",
        ChangeReviewPurpose::Commit => "Commit changes",
    };
    let title = match state.change_review.mode {
        ChangeViewMode::List => format!(" {title_prefix} "),
        ChangeViewMode::Diff => format!(
            " {title_prefix} · {} ",
            state
                .change_review
                .diff_path
                .as_deref()
                .unwrap_or("tracked file")
        ),
        ChangeViewMode::Subject => " Commit subject ".to_string(),
        ChangeViewMode::Confirm => " Confirm commit ".to_string(),
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let wrapped_height = |text: &str| {
        let width = usize::from(inner.width.max(1));
        text.lines()
            .map(|line| line.chars().count().max(1).div_ceil(width))
            .sum::<usize>() as u16
    };
    let mut header_height = u16::from(state.change_review.is_commit());
    if let Some(error) = &state.change_review.error {
        let safe = crate::terminal_text::sanitize_terminal_text(error);
        header_height = header_height.saturating_add(wrapped_height(&format!("Error: {safe}")));
    } else if let Some(notice) = &state.change_review.notice {
        let safe = crate::terminal_text::sanitize_terminal_text(notice);
        header_height = header_height.saturating_add(wrapped_height(&safe));
    }
    let footer_height = if state.change_review.mode == ChangeViewMode::Confirm {
        2
    } else {
        1
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(1),
            Constraint::Length(footer_height),
        ])
        .split(inner);

    if header_height > 0 {
        let mut header = Vec::new();
        if state.change_review.is_commit() {
            header.push(Line::from(match &state.change_review.repository {
                Some(repository) => format!(
                    "Branch: {} · HEAD: {}",
                    repository.branch,
                    repository
                        .head
                        .as_deref()
                        .map(|head| &head[..head.len().min(12)])
                        .unwrap_or("unborn")
                ),
                None => "Resolving repository state…".into(),
            }));
        }
        if let Some(error) = &state.change_review.error {
            header.push(Line::from(Span::styled(
                format!(
                    "Error: {}",
                    crate::terminal_text::sanitize_terminal_text(error)
                ),
                Style::default().fg(Color::Red),
            )));
        } else if let Some(notice) = &state.change_review.notice {
            header.push(Line::from(Span::styled(
                crate::terminal_text::sanitize_terminal_text(notice),
                Style::default().fg(Color::Green),
            )));
        }
        frame.render_widget(Paragraph::new(header).wrap(Wrap { trim: false }), chunks[0]);
    }

    if state.change_review.loading {
        let label = match state.change_review.operation {
            Some(GitReviewOperation::Staging) => "Staging selected file…",
            Some(GitReviewOperation::Unstaging) => "Unstaging selected file…",
            Some(GitReviewOperation::Preparing) => "Capturing staged commit snapshot…",
            Some(GitReviewOperation::Committing) => "Creating and verifying commit…",
            Some(GitReviewOperation::Loading) | None => "Loading working tree changes…",
        };
        frame.render_widget(
            Paragraph::new(label).style(Style::default().fg(Color::DarkGray)),
            chunks[1],
        );
    } else if state.change_review.error.is_some()
        && state.change_review.repository.is_none()
        && state.change_review.files.is_empty()
    {
        frame.render_widget(
            Paragraph::new("The commit workflow is unavailable until this Git state is resolved.")
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: false }),
            chunks[1],
        );
    } else {
        match state.change_review.mode {
            ChangeViewMode::List => {
                if state.change_review.files.is_empty() {
                    frame.render_widget(
                        Paragraph::new("No working tree changes")
                            .style(Style::default().fg(Color::DarkGray)),
                        chunks[1],
                    );
                } else {
                    let available = chunks[1].height as usize;
                    let selected = state
                        .change_review
                        .selected_index
                        .min(state.change_review.files.len().saturating_sub(1));
                    let first = selected.saturating_sub(available.saturating_sub(1));
                    let items = state
                        .change_review
                        .files
                        .iter()
                        .enumerate()
                        .skip(first)
                        .take(available)
                        .map(|(index, file)| {
                            let path = match &file.old_path {
                                Some(_) => format!(
                                    "{} → {}",
                                    file.display_old_path().unwrap_or_default(),
                                    file.display_path()
                                ),
                                None => file.display_path(),
                            };
                            let counts = match (file.additions, file.deletions) {
                                (Some(additions), Some(deletions)) => {
                                    format!("  +{additions} -{deletions}")
                                }
                                _ if file.is_tracked() => "  binary".into(),
                                _ => String::new(),
                            };
                            let line = format!(
                                " {}  {:<17} {}{}",
                                file.marker(),
                                file.location_label(),
                                path,
                                counts
                            );
                            let style = if index == selected {
                                Style::default().fg(Color::Black).bg(Color::Cyan)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            ListItem::new(Line::from(Span::styled(line, style)))
                        })
                        .collect::<Vec<_>>();
                    frame.render_widget(List::new(items), chunks[1]);
                }
            }
            ChangeViewMode::Diff => {
                let lines = state
                    .change_review
                    .diff_content
                    .lines()
                    .map(|line| {
                        let style = if line.starts_with("+++") || line.starts_with("---") {
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD)
                        } else if line.starts_with('+') {
                            Style::default().fg(Color::Green)
                        } else if line.starts_with('-') {
                            Style::default().fg(Color::Red)
                        } else if line.starts_with("@@") {
                            Style::default().fg(Color::Cyan)
                        } else if line == "Staged changes" || line == "Unstaged changes" {
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        Line::from(Span::styled(line.to_string(), style))
                    })
                    .collect::<Vec<_>>();
                frame.render_widget(
                    Paragraph::new(Text::from(lines))
                        .scroll((state.change_review.scroll, 0))
                        .wrap(Wrap { trim: false }),
                    chunks[1],
                );
            }
            ChangeViewMode::Subject => {
                let cursor = state
                    .change_review
                    .subject_cursor
                    .min(state.change_review.subject.len());
                let (before, after) = state.change_review.subject.split_at(cursor);
                let mut spans = vec![Span::raw(before.to_string())];
                if let Some(character) = after.chars().next() {
                    spans.push(Span::styled(
                        character.to_string(),
                        Style::default().fg(Color::Black).bg(Color::Cyan),
                    ));
                    spans.push(Span::raw(after[character.len_utf8()..].to_string()));
                } else {
                    spans.push(Span::styled(
                        " ",
                        Style::default().fg(Color::Black).bg(Color::Cyan),
                    ));
                }
                let lines = vec![
                    Line::from("Enter one commit subject (maximum 256 characters):"),
                    Line::from(""),
                    Line::from(spans),
                ];
                frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), chunks[1]);
            }
            ChangeViewMode::Confirm => {
                let mut lines = Vec::new();
                if let Some(snapshot) = &state.change_review.commit_snapshot {
                    lines.push(Line::from(format!("Branch: {}", snapshot.branch)));
                    lines.push(Line::from(format!(
                        "Parent: {}",
                        snapshot
                            .head
                            .as_deref()
                            .map(|head| &head[..head.len().min(12)])
                            .unwrap_or("initial commit")
                    )));
                    lines.push(Line::from(format!(
                        "Index tree: {}",
                        &snapshot.index_tree[..snapshot.index_tree.len().min(12)]
                    )));
                    lines.push(Line::from(format!(
                        "Subject: {}",
                        state.change_review.subject
                    )));
                    lines.push(Line::from(""));
                    lines.push(Line::from(format!(
                        "Staged files ({})",
                        snapshot.files.len()
                    )));
                    let mut total_additions = 0u64;
                    let mut total_deletions = 0u64;
                    let mut binary = 0usize;
                    for file in &snapshot.files {
                        let counts = match (file.additions, file.deletions) {
                            (Some(additions), Some(deletions)) => {
                                total_additions = total_additions.saturating_add(additions);
                                total_deletions = total_deletions.saturating_add(deletions);
                                format!("+{additions} -{deletions}")
                            }
                            _ => {
                                binary += 1;
                                "binary".into()
                            }
                        };
                        lines.push(Line::from(format!(
                            " {}  {}  {counts}",
                            match file.kind {
                                crate::changes::ChangeKind::Modified => "M",
                                crate::changes::ChangeKind::Added => "A",
                                crate::changes::ChangeKind::Deleted => "D",
                                crate::changes::ChangeKind::Renamed => "R",
                                crate::changes::ChangeKind::Untracked => "?",
                            },
                            file.display_path()
                        )));
                    }
                    lines.push(Line::from(""));
                    lines.push(Line::from(format!(
                        "Totals: +{total_additions} -{total_deletions}{}",
                        if binary == 0 {
                            String::new()
                        } else {
                            format!(" · {binary} binary")
                        }
                    )));
                }
                frame.render_widget(
                    Paragraph::new(lines)
                        .scroll((state.change_review.confirm_scroll, 0))
                        .wrap(Wrap { trim: false }),
                    chunks[1],
                );
            }
        }
    }

    let hint = match state.change_review.mode {
        ChangeViewMode::List if state.change_review.is_commit() => {
            "↑↓ select · Enter diff · s stage · u unstage · c commit · r refresh · Esc close"
        }
        ChangeViewMode::List => "↑↓ select · Enter open tracked diff · Esc close",
        ChangeViewMode::Diff if state.change_review.is_commit() => {
            "↑↓/Pg scroll · s stage · u unstage · c commit · Esc list"
        }
        ChangeViewMode::Diff => "↑↓/PgUp/PgDn scroll · Esc return to file list",
        ChangeViewMode::Subject => "Enter review commit · Esc return to file list",
        ChangeViewMode::Confirm => "←/→ choose · ↑↓/Pg scroll · Enter select · Esc edit subject",
    };
    if state.change_review.operation.is_some() {
        frame.render_widget(
            Paragraph::new("Ctrl+C cancel operation · controls locked")
                .style(Style::default().fg(Color::DarkGray)),
            chunks[2],
        );
    } else if state.change_review.mode == ChangeViewMode::Confirm {
        let cancel_style = if state.change_review.commit_selected {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        };
        let commit_style = if state.change_review.commit_selected {
            Style::default().fg(Color::Black).bg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(" Cancel ", cancel_style),
                    Span::raw("  "),
                    Span::styled(" Commit ", commit_style),
                ]),
                Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray))),
            ]),
            chunks[2],
        );
    } else {
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
            chunks[2],
        );
    }
}

fn render_change_walkthrough(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(walkthrough) = state.change_walkthrough.as_ref() else {
        return;
    };
    use crate::change_walkthrough::ChangeWalkthroughStatus;

    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Guided changes walkthrough ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let status = match walkthrough.status {
        ChangeWalkthroughStatus::Capturing => "Capturing complete bounded Git snapshot…",
        ChangeWalkthroughStatus::Generating => "Generating ordered explanation…",
        ChangeWalkthroughStatus::Complete => "Fresh · source still matches the explanation",
        ChangeWalkthroughStatus::Refused => "Not generated · incomplete input",
        ChangeWalkthroughStatus::Failed => "Walkthrough failed",
        ChangeWalkthroughStatus::Stale => "STALE · source changed after this explanation",
    };
    let status_style = match walkthrough.status {
        ChangeWalkthroughStatus::Complete => Style::default().fg(Color::Green),
        ChangeWalkthroughStatus::Stale
        | ChangeWalkthroughStatus::Refused
        | ChangeWalkthroughStatus::Failed => Style::default().fg(Color::Yellow),
        _ => Style::default().fg(Color::Cyan),
    };
    let target = crate::change_walkthrough::target_label(&walkthrough.target);
    let mut lines = vec![
        Line::from(Span::styled(
            status,
            status_style.add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "Target: {target} · Route: {}/{}",
            walkthrough.provider, walkthrough.model
        )),
    ];
    if let Some(hash) = &walkthrough.source_hash {
        let short = hash.get(..12).unwrap_or(hash);
        lines.push(Line::from(format!(
            "Source: {short} · {} tracked file{}",
            walkthrough.included_files.len(),
            if walkthrough.included_files.len() == 1 {
                ""
            } else {
                "s"
            }
        )));
    }
    lines.push(Line::from(""));

    match walkthrough.status {
        ChangeWalkthroughStatus::Capturing => {
            lines.push(Line::from(
                "Reading Git state without changing the repository.",
            ));
        }
        ChangeWalkthroughStatus::Generating => {
            lines.push(Line::from(
                "The provider receives only this exact snapshot and the reporting schema.",
            ));
        }
        ChangeWalkthroughStatus::Complete | ChangeWalkthroughStatus::Stale => {
            if walkthrough.status == ChangeWalkthroughStatus::Stale {
                lines.push(Line::from(Span::styled(
                    "This explanation is retained for comparison. Press Shift+R to recapture explicitly.",
                    Style::default().fg(Color::Yellow),
                )));
                lines.push(Line::from(""));
            }
            if let Some(result) = &walkthrough.result {
                lines.push(Line::from(Span::styled(
                    "Summary",
                    Style::default().add_modifier(Modifier::BOLD),
                )));
                lines.extend(
                    result
                        .summary
                        .lines()
                        .map(|line| Line::from(line.to_string())),
                );
                for step in &result.steps {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!("{}. {}", step.order, step.title),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )));
                    if !step.files.is_empty() {
                        lines.push(Line::from(format!("Files: {}", step.files.join(", "))));
                    }
                    lines.extend(
                        step.explanation
                            .lines()
                            .map(|line| Line::from(line.to_string())),
                    );
                }
                if !result.notes.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "Notes",
                        Style::default().add_modifier(Modifier::BOLD),
                    )));
                    lines.extend(
                        result
                            .notes
                            .iter()
                            .map(|note| Line::from(format!("- {note}"))),
                    );
                }
            }
        }
        ChangeWalkthroughStatus::Refused => {
            if let Some(refusal) = &walkthrough.refusal {
                lines.push(Line::from(refusal.reason.clone()));
                if !refusal.omitted_files.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "Omitted tracked files",
                        Style::default().add_modifier(Modifier::BOLD),
                    )));
                    lines.extend(
                        refusal
                            .omitted_files
                            .iter()
                            .map(|path| Line::from(format!("- {path}"))),
                    );
                }
                if !refusal.untracked_files.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "Untracked names only; contents were not sent",
                        Style::default().add_modifier(Modifier::BOLD),
                    )));
                    lines.extend(
                        refusal
                            .untracked_files
                            .iter()
                            .map(|path| Line::from(format!("- {path}"))),
                    );
                }
            }
        }
        ChangeWalkthroughStatus::Failed => {
            if let Some(error) = &walkthrough.error {
                lines.push(Line::from(Span::styled(
                    error.clone(),
                    Style::default().fg(Color::Red),
                )));
            }
        }
    }
    if let Some(error) = &walkthrough.freshness_error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::default().fg(Color::Yellow),
        )));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .scroll((walkthrough.scroll, 0))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );
    let busy = matches!(
        walkthrough.status,
        ChangeWalkthroughStatus::Capturing | ChangeWalkthroughStatus::Generating
    );
    let hint = if busy {
        "Esc cancel"
    } else {
        "↑↓/PgUp/PgDn scroll · Shift+R recapture · Esc close"
    };
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
        chunks[1],
    );
}

fn render_jobs_view(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let activity_count =
        state.job_list.len() + state.watch_list.len() + state.delegation_list.len();
    let selected = state.selected_job_idx.min(activity_count.saturating_sub(1));
    let selected_job = state.job_list.get(selected);
    let selected_watcher = selected
        .checked_sub(state.job_list.len())
        .and_then(|index| state.watch_list.get(index));
    let selected_delegation = selected
        .checked_sub(state.job_list.len() + state.watch_list.len())
        .and_then(|index| state.delegation_list.get(index));
    let title = if state.job_detail_open {
        if let Some(job) = selected_job {
            format!(" Managed shell job · {} ", job.id)
        } else if let Some(watcher) = selected_watcher {
            format!(" PID watcher · {} ", watcher.watch_id)
        } else if let Some(delegation) = selected_delegation {
            format!(" Codex-team delegation · {} ", delegation.job_id)
        } else {
            " Background activity ".to_string()
        }
    } else {
        " Background activity ".to_string()
    };
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    if state.job_detail_open {
        if let Some(job) = selected_job {
            let mode = if job.background {
                "background"
            } else {
                "foreground"
            };
            let output = crate::changes::sanitize_terminal_text(&format!(
                "Command: {}\nDirectory: {}\nMode: {mode}\nStatus: {}\nElapsed: {:.1}s\n\nstdout:\n{}\n\nstderr:\n{}",
                job.command,
                job.cwd,
                job.status.label(),
                job.elapsed.as_secs_f64(),
                job.stdout,
                job.stderr,
            ));
            frame.render_widget(
                Paragraph::new(output)
                    .scroll((state.job_scroll, 0))
                    .wrap(Wrap { trim: false }),
                chunks[0],
            );
        } else if let Some(watcher) = selected_watcher {
            let host = match &watcher.request.host {
                ca_watch::HostSpec::Local => "local".to_string(),
                ca_watch::HostSpec::Ssh { destination, port } => port.map_or_else(
                    || format!("ssh {destination}"),
                    |port| format!("ssh {destination}:{port}"),
                ),
            };
            let paths = match &watcher.request.process {
                ca_watch::ProcessSpec::Generic => String::new(),
                ca_watch::ProcessSpec::Copy {
                    log_path,
                    completion_marker,
                } => format!(
                    "\nLog: {}\nCompletion marker: {}",
                    log_path.as_deref().unwrap_or("none"),
                    completion_marker
                ),
            };
            let elapsed = watcher
                .event
                .observed_at
                .signed_duration_since(watcher.started_at)
                .num_seconds()
                .max(0);
            let output = crate::changes::sanitize_terminal_text(&format!(
                "Label: {}\nHost: {host}\nPID: {}\nProcess: {}\nCadence: {}s\nHealth: {}\nElapsed: {elapsed}s\nProgress: {}{paths}\n\nStopping this watcher stops observation only. The target process is unaffected.",
                watcher.request.label,
                watcher.request.pid,
                watcher.request.process.kind_name(),
                watcher.request.interval_secs,
                watcher.event.state,
                watcher.event.summary,
            ));
            frame.render_widget(
                Paragraph::new(output)
                    .scroll((state.job_scroll, 0))
                    .wrap(Wrap { trim: false }),
                chunks[0],
            );
        } else if let Some(delegation) = selected_delegation {
            let output = crate::changes::sanitize_terminal_text(&format!(
                "Job: {}\nServer: {}\nActions: {}\nStatus: {:?}\nRevision: {}\nStarted: {}\nUpdated: {}\nReceipt digest: {}\nShared work: {}\nAttached consumers: {}\nSource receipt: {}\nAvoided execution: {} ms\n\nCancellation detaches this consumer. Shared executor-owned work stops only when its final consumer leaves; codex-team never signals an observed PID.",
                delegation.job_id,
                delegation.server,
                delegation.action_categories.join(", "),
                delegation.status,
                delegation.repository_revision,
                delegation.started_at,
                delegation.updated_at,
                delegation.receipt_digest.as_deref().unwrap_or("pending"),
                delegation.broker_disposition.as_deref().unwrap_or("local"),
                delegation
                    .consumer_count
                    .map(|count| count.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                delegation
                    .source_receipt_digest
                    .as_deref()
                    .unwrap_or("pending"),
                delegation.avoided_execution_millis,
            ));
            frame.render_widget(
                Paragraph::new(output)
                    .scroll((state.job_scroll, 0))
                    .wrap(Wrap { trim: false }),
                chunks[0],
            );
        }
    } else if activity_count == 0 {
        frame.render_widget(
            Paragraph::new(
                "No shell jobs, PID watchers, or codex-team delegations in this ca process",
            )
            .style(Style::default().fg(Color::DarkGray)),
            chunks[0],
        );
    } else {
        let available = chunks[0].height as usize;
        let first = selected.saturating_sub(available.saturating_sub(1));
        let mut items = state
            .job_list
            .iter()
            .enumerate()
            .map(|(index, job)| {
                let id = job.id.to_string();
                let short_id = &id[..8.min(id.len())];
                let mode = if job.background { "bg" } else { "fg" };
                let command = crate::changes::sanitize_terminal_text(&job.command);
                let recent = job
                    .stdout
                    .lines()
                    .last()
                    .or_else(|| job.stderr.lines().last())
                    .map(crate::changes::sanitize_terminal_text)
                    .filter(|line| !line.is_empty())
                    .map_or_else(String::new, |line| format!(" · {line}"));
                let line = format!(
                    " [job] {short_id}  {mode}  {:<15} {:>7.1}s  {command}{recent}",
                    job.status.label(),
                    job.elapsed.as_secs_f64(),
                );
                let style = if index == selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Span::styled(line, style))
            })
            .collect::<Vec<_>>();
        items.extend(
            state
                .watch_list
                .iter()
                .enumerate()
                .map(|(watch_index, watcher)| {
                    let index = state.job_list.len() + watch_index;
                    let id = watcher.watch_id.to_string();
                    let short_id = &id[..8.min(id.len())];
                    let host = watcher.request.host.display_name();
                    let line = format!(
                        " [watch] {short_id}  {:<11}  {host} PID {} · {} · {}s · {}",
                        watcher.event.state,
                        watcher.request.pid,
                        watcher.request.process.kind_name(),
                        watcher.request.interval_secs,
                        watcher.event.summary,
                    );
                    let style = if index == selected {
                        Style::default().fg(Color::Black).bg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Span::styled(line, style))
                }),
        );
        items.extend(state.delegation_list.iter().enumerate().map(
            |(delegation_index, delegation)| {
                let index = state.job_list.len() + state.watch_list.len() + delegation_index;
                let short_id = &delegation.job_id[..8.min(delegation.job_id.len())];
                let line = format!(
                    " [delegate] {short_id}  {:<11?}  {} · {}",
                    delegation.status,
                    delegation.action_categories.join("+"),
                    delegation.updated_at,
                );
                let style = if index == selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Span::styled(line, style))
            },
        ));
        let visible = items
            .into_iter()
            .skip(first)
            .take(available)
            .collect::<Vec<_>>();
        frame.render_widget(List::new(visible), chunks[0]);
    }

    let narrow = chunks[1].width < 72;
    let hint = if state.job_detail_open && selected_delegation.is_some() && narrow {
        "↑↓ scroll · x cancel · r refresh · a/1-5 feedback · Esc list"
    } else if state.job_detail_open && selected_delegation.is_some() {
        "↑↓/PgUp/PgDn scroll · x cancel · r refresh · a accept · 1-5 negative · C correction · Esc list"
    } else if state.job_detail_open && selected_watcher.is_some() && narrow {
        "↑↓ scroll · x stop watching · Esc list"
    } else if state.job_detail_open && selected_watcher.is_some() {
        "↑↓/PgUp/PgDn scroll · x stop watching (target unaffected) · Esc list"
    } else if state.job_detail_open && narrow {
        "↑↓ scroll · i stdin · x cancel · Esc list"
    } else if state.job_detail_open {
        "↑↓/PgUp/PgDn scroll · i write stdin · x cancel · Esc list"
    } else if selected_delegation.is_some() && narrow {
        "Enter open · x cancel · r refresh · a/1-5 feedback · Esc close"
    } else if selected_delegation.is_some() {
        "↑↓ select · Enter open · x cancel · r refresh · a accept · 1-5 negative · C correction · Esc close"
    } else if narrow {
        "Enter open · x cancel/stop · r refresh · Esc close"
    } else {
        "↑↓ select · Enter open · i stdin (jobs) · x cancel/stop watching · r refresh · Esc close"
    };
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
        chunks[1],
    );
}

fn render_terminal_manager(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(manager) = state.terminal_manager.as_ref() else {
        return;
    };
    let width = 88.min(area.width.saturating_sub(2));
    let height = 26.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Durable terminals ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(inner);
    let status = if manager.unavailable {
        "tmux unavailable · install tmux and reopen /term".to_string()
    } else {
        format!(
            "{} · {} session{}",
            manager.version.as_deref().unwrap_or("tmux"),
            manager.sessions.len(),
            if manager.sessions.len() == 1 { "" } else { "s" }
        )
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(status, Style::default().fg(Color::DarkGray))),
            Line::from(format!("> {}", manager.filter)),
        ]),
        chunks[0],
    );

    match &manager.mode {
        crate::app::TerminalManagerMode::Create { name, cursor: _ } => {
            let lines = vec![
                Line::from(Span::styled(
                    "Create a detached tmux session",
                    Style::default().fg(Color::Cyan),
                )),
                Line::from(""),
                Line::from(format!("Name: {name}")),
                Line::from(""),
                Line::from(Span::styled(
                    "The session is user-owned and survives ca shutdown.",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), chunks[1]);
        }
        crate::app::TerminalManagerMode::List if manager.unavailable => {
            frame.render_widget(
                Paragraph::new(
                    "ca did not start or install anything. Install tmux with your package manager, then press r to refresh.",
                )
                .wrap(Wrap { trim: false }),
                chunks[1],
            );
        }
        crate::app::TerminalManagerMode::List => {
            let filtered = manager.filtered_indices();
            if filtered.is_empty() {
                frame.render_widget(
                    Paragraph::new(if manager.sessions.is_empty() {
                        "No tmux sessions. Press Shift+A to create one."
                    } else {
                        "No terminals match this filter."
                    })
                    .style(Style::default().fg(Color::DarkGray)),
                    chunks[1],
                );
            } else {
                let selected = manager.selected_idx.min(filtered.len().saturating_sub(1));
                let available = chunks[1].height as usize;
                let first = selected.saturating_sub(available.saturating_sub(1));
                let items = filtered
                    .iter()
                    .enumerate()
                    .skip(first)
                    .take(available)
                    .filter_map(|(visible_index, index)| {
                        manager.sessions.get(*index).map(|session| {
                            let attached = if session.attached_clients == 0 {
                                "detached".to_string()
                            } else {
                                format!("{} attached", session.attached_clients)
                            };
                            let line = format!(
                                " {:<28} {:>3} window{} · {}",
                                crate::terminal_text::sanitize_terminal_text(&session.name),
                                session.windows,
                                if session.windows == 1 { " " } else { "s" },
                                attached,
                            );
                            let style = if visible_index == selected {
                                Style::default().fg(Color::Black).bg(Color::Cyan)
                            } else if session.attached_clients > 0 {
                                Style::default().fg(Color::Green)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            ListItem::new(Span::styled(line, style))
                        })
                    })
                    .collect::<Vec<_>>();
                frame.render_widget(List::new(items), chunks[1]);
            }
        }
    }

    let mut footer = if manager.busy {
        "Working… controls locked".to_string()
    } else if matches!(manager.mode, crate::app::TerminalManagerMode::Create { .. }) {
        "Enter create · Esc cancel".to_string()
    } else if chunks[2].width < 72 {
        "Enter attach · ⇧A add · ⇧D detach\n⇧K kill · Esc close".to_string()
    } else {
        "Enter attach · Shift+A create · Shift+D detach\nShift+K kill · r refresh · Esc close"
            .to_string()
    };
    if let Some(error) = &manager.error {
        footer.push('\n');
        footer.push_str(&crate::terminal_text::sanitize_terminal_text(error));
    } else if let Some(notice) = &manager.notice {
        footer.push('\n');
        footer.push_str(&crate::terminal_text::sanitize_terminal_text(notice));
    }
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(if manager.error.is_some() {
            Color::Red
        } else {
            Color::DarkGray
        })),
        chunks[2],
    );

    if let Some(confirmation) = &manager.kill_confirmation {
        let confirm = centered_rect(64.min(area.width.saturating_sub(2)), 10, area);
        frame.render_widget(Clear, confirm);
        let block = Block::default()
            .title(" Kill durable terminal? ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));
        let inner = block.inner(confirm);
        frame.render_widget(block, confirm);
        let cancel_style = if confirmation.kill_selected {
            Style::default()
        } else {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        };
        let kill_style = if confirmation.kill_selected {
            Style::default().fg(Color::White).bg(Color::Red)
        } else {
            Style::default()
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!(
                    "Permanently kill tmux session {}?",
                    confirmation.name
                )),
                Line::from("Its processes and windows will be terminated by tmux."),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        if confirmation.kill_selected {
                            "  Cancel  "
                        } else {
                            "▶ Cancel  "
                        },
                        cancel_style,
                    ),
                    Span::raw("   "),
                    Span::styled(
                        if confirmation.kill_selected {
                            "▶ Kill  "
                        } else {
                            "  Kill  "
                        },
                        kill_style,
                    ),
                ]),
                Line::from(Span::styled(
                    "Arrow/Tab choose · Enter confirm · Esc cancel",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .wrap(Wrap { trim: false }),
            inner,
        );
    }
}

fn render_theme_manager(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(manager) = state.theme_manager.as_ref() else {
        return;
    };
    let palette = palette(state);
    let width = 64.min(area.width.saturating_sub(2));
    let height = 18.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Theme ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!("> {}", manager.filter)),
            Line::from(Span::styled(
                "Highlight previews immediately; Enter persists.",
                Style::default().fg(palette.muted),
            )),
        ]),
        chunks[0],
    );
    let themes = manager.filtered_themes();
    if themes.is_empty() {
        frame.render_widget(
            Paragraph::new("No themes match this filter.")
                .style(Style::default().fg(palette.muted)),
            chunks[1],
        );
    } else {
        let selected = manager.selected_idx.min(themes.len().saturating_sub(1));
        let items = themes
            .iter()
            .enumerate()
            .map(|(index, theme)| {
                let current = if *theme == manager.saved_theme {
                    "  current"
                } else {
                    ""
                };
                let preview = if index == selected { "  preview" } else { "" };
                let line = format!(" {:<12}{current}{preview}", theme.label());
                let style = if index == selected {
                    Style::default()
                        .fg(palette.accent_foreground)
                        .bg(palette.accent)
                } else if *theme == manager.saved_theme {
                    Style::default().fg(palette.success)
                } else {
                    Style::default()
                };
                ListItem::new(Span::styled(line, style))
            })
            .collect::<Vec<_>>();
        frame.render_widget(List::new(items), chunks[1]);
    }
    let mut footer = if manager.busy {
        "Saving… controls locked".to_string()
    } else if state.run_capabilities.config_writes {
        "Enter save · Esc restore · ↑↓ navigate · type to filter".to_string()
    } else {
        "Bare profile · preview only · Esc restore".to_string()
    };
    if let Some(error) = &manager.error {
        footer.push('\n');
        footer.push_str(&crate::terminal_text::sanitize_terminal_text(error));
    } else if let Some(notice) = &manager.notice {
        footer.push('\n');
        footer.push_str(&crate::terminal_text::sanitize_terminal_text(notice));
    }
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(if manager.error.is_some() {
            palette.error
        } else {
            palette.muted
        })),
        chunks[2],
    );
}

fn render_notification_picker(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(picker) = state.notification_picker.as_ref() else {
        return;
    };
    let palette = palette(state);
    let width = 54.min(area.width.saturating_sub(2));
    let height = 13.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Completion notification ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new("Notify when a top-level turn runs for 30 seconds."),
        chunks[0],
    );
    let items = crate::ui_settings::NotificationStyle::ALL
        .iter()
        .enumerate()
        .map(|(index, style)| {
            let selected = index == picker.selected_idx;
            let saved = if *style == picker.saved {
                "  current"
            } else {
                ""
            };
            let line = format!(
                "{} {:<8}{saved}",
                if selected { "▸" } else { " " },
                style.label()
            );
            let style = if selected {
                Style::default()
                    .fg(palette.accent_foreground)
                    .bg(palette.accent)
            } else if *style == picker.saved {
                Style::default().fg(palette.success)
            } else {
                Style::default()
            };
            ListItem::new(Span::styled(line, style))
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), chunks[1]);
    let footer = if picker.busy {
        "Saving… controls locked".into()
    } else if let Some(error) = picker.error.as_deref() {
        crate::terminal_text::sanitize_terminal_text(error)
    } else if state.run_capabilities.config_writes {
        "Enter save · Esc cancel · ↑↓ navigate".into()
    } else {
        "Bare profile · persistent changes disabled".into()
    };
    frame.render_widget(
        Paragraph::new(footer)
            .style(Style::default().fg(if picker.error.is_some() {
                palette.error
            } else {
                palette.muted
            }))
            .wrap(Wrap { trim: false }),
        chunks[2],
    );
}

fn render_copy_picker(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(picker) = state.copy_picker.as_ref() else {
        return;
    };
    let palette = palette(state);
    let width = 88.min(area.width.saturating_sub(2));
    let height = 20.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let title = match picker.level {
        crate::app::CopyPickerLevel::Messages => " Copy message ",
        crate::app::CopyPickerLevel::Parts { .. } => " Copy message part ",
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(format!("Search: {}", picker.filter))
            .style(Style::default().fg(palette.text)),
        chunks[0],
    );
    let rows = match picker.level {
        crate::app::CopyPickerLevel::Messages => picker
            .filtered_message_indices()
            .into_iter()
            .map(|index| picker.messages[index].summary.clone())
            .collect::<Vec<_>>(),
        crate::app::CopyPickerLevel::Parts { message_index } => picker
            .filtered_part_indices(message_index)
            .into_iter()
            .filter_map(|part_index| {
                picker.messages.get(message_index).and_then(|message| {
                    message.parts.get(part_index).map(|part| {
                        let preview = part
                            .value
                            .lines()
                            .find(|line| !line.trim().is_empty())
                            .unwrap_or("");
                        let preview = crate::terminal_text::sanitize_terminal_text(preview)
                            .chars()
                            .take(64)
                            .collect::<String>();
                        format!("{} · {preview}", part.label)
                    })
                })
            })
            .collect::<Vec<_>>(),
    };
    let items = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let selected = index == picker.selected_idx;
            let line = format!("{} {row}", if selected { "▸" } else { " " });
            ListItem::new(Span::styled(
                line,
                if selected {
                    Style::default()
                        .fg(palette.accent_foreground)
                        .bg(palette.accent)
                } else {
                    Style::default()
                },
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), chunks[1]);
    let footer = match picker.level {
        crate::app::CopyPickerLevel::Messages => {
            "Enter inspect parts · Esc close · ↑↓ navigate · type to filter"
        }
        crate::app::CopyPickerLevel::Parts { .. } => {
            "Enter copy exact stored bytes · Esc back · ↑↓ navigate · type to filter"
        }
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(palette.muted)),
        chunks[2],
    );
}

fn render_command_output_picker(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(picker) = state.command_output_picker.as_ref() else {
        return;
    };
    let palette = palette(state);
    let popup = centered_rect(
        58.min(area.width.saturating_sub(2)),
        12.min(area.height.saturating_sub(2)),
        area,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Command output ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(2),
            Constraint::Length(3),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(
            "Semantic summarizes supported Git/Cargo output for display and model continuation.\nCtrl+O always shows the process-local bounded raw output.",
        ),
        chunks[0],
    );
    let items = crate::config::CommandOutputPolicy::ALL
        .iter()
        .enumerate()
        .map(|(index, policy)| {
            let selected = index == picker.selected_idx;
            let current = if *policy == picker.saved {
                "  current"
            } else {
                ""
            };
            ListItem::new(Span::styled(
                format!(
                    "{} {:<10}{current}",
                    if selected { "▸" } else { " " },
                    policy.label()
                ),
                if selected {
                    Style::default()
                        .fg(palette.accent_foreground)
                        .bg(palette.accent)
                } else if *policy == picker.saved {
                    Style::default().fg(palette.success)
                } else {
                    Style::default()
                },
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), chunks[1]);
    let footer = if picker.busy {
        "Saving… controls locked".into()
    } else if let Some(error) = picker.error.as_deref() {
        crate::terminal_text::sanitize_terminal_text(error)
    } else if state.run_capabilities.config_writes {
        "Enter save · Esc cancel · ↑↓ navigate".into()
    } else {
        "Bare profile · persistent changes disabled".into()
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(palette.muted)),
        chunks[2],
    );
}

fn render_output_full_confirmation(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(confirmation) = state.output_full_confirmation.as_ref() else {
        return;
    };
    let palette = palette(state);
    let popup = centered_rect(
        76.min(area.width.saturating_sub(2)),
        14.min(area.height.saturating_sub(2)),
        area,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Full command output ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.warning));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let command = crate::terminal_text::sanitize_terminal_text(&confirmation.rerun.command);
    let cwd = crate::terminal_text::sanitize_terminal_text(&confirmation.rerun.cwd);
    let body = format!(
        "Rerun this exact command in native scrollback?\n\nCommand: {command}\nDirectory: {cwd}\nNetwork: {}\n\n{}   {}",
        if confirmation.rerun.network_requested { "allowed" } else { "disabled" },
        if confirmation.run_selected { "  Cancel  " } else { "[ Cancel ]" },
        if confirmation.run_selected { "[ Run ]" } else { "  Run  " },
    );
    let mut paragraph = Paragraph::new(body).wrap(Wrap { trim: false });
    if confirmation.busy {
        paragraph = paragraph.style(Style::default().fg(palette.muted));
    }
    frame.render_widget(paragraph, inner);
}

fn render_scratch_delete_confirmation(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(confirmation) = state.scratch_delete_confirmation.as_ref() else {
        return;
    };
    let palette = palette(state);
    let popup = centered_rect(
        62.min(area.width.saturating_sub(2)),
        10.min(area.height.saturating_sub(2)),
        area,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Delete scratch entry ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.warning));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let body = format!(
        "Delete scratch entry {}?\n\n{}   {}{}",
        confirmation.id,
        if confirmation.delete_selected {
            "  Cancel  "
        } else {
            "[ Cancel ]"
        },
        if confirmation.delete_selected {
            "[ Delete ]"
        } else {
            "  Delete  "
        },
        confirmation
            .error
            .as_ref()
            .map_or(String::new(), |error| format!(
                "\n\n{}",
                crate::terminal_text::sanitize_terminal_text(error)
            ))
    );
    frame.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), inner);
}

fn render_reasoning_picker(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(picker) = state.reasoning_picker.as_ref() else {
        return;
    };
    let palette = palette(state);
    let width = 54.min(area.width.saturating_sub(2));
    let desired_height = (picker.efforts.len() as u16)
        .saturating_add(7)
        .clamp(10, 16);
    let height = desired_height.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Reasoning effort ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(2),
            Constraint::Length(2),
        ])
        .split(inner);
    let model = state.current_model.as_ref().map_or_else(
        || "current model".into(),
        |model| crate::terminal_text::sanitize_terminal_text(&model.display_name),
    );
    frame.render_widget(
        Paragraph::new(format!("Select reasoning for {model}.")),
        chunks[0],
    );
    let items = picker
        .efforts
        .iter()
        .enumerate()
        .map(|(index, effort)| {
            let selected = index == picker.selected_idx;
            let current = if Some(*effort) == state.reasoning_effort {
                "  current"
            } else {
                ""
            };
            let line = format!(
                "{} {:<10}{current}",
                if selected { "▸" } else { " " },
                effort
            );
            let style = if selected {
                Style::default()
                    .fg(palette.accent_foreground)
                    .bg(palette.accent)
            } else if Some(*effort) == state.reasoning_effort {
                Style::default().fg(palette.success)
            } else {
                Style::default()
            };
            ListItem::new(Span::styled(line, style))
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), chunks[1]);
    frame.render_widget(
        Paragraph::new("Enter select · Esc cancel · ↑↓ navigate")
            .style(Style::default().fg(palette.muted)),
        chunks[2],
    );
}

fn render_route_preset_picker(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(picker) = state.route_preset_picker.as_ref() else {
        return;
    };
    let palette = palette(state);
    let width = 76.min(area.width.saturating_sub(2));
    let desired_height = (picker.rows.len() as u16).saturating_add(8).clamp(11, 24);
    let height = desired_height.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Route presets ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new("Apply provider, model, and reasoning for this process."),
        chunks[0],
    );
    let items = if picker.rows.is_empty() {
        vec![ListItem::new(Span::styled(
            "No route presets configured",
            Style::default().fg(palette.muted),
        ))]
    } else {
        picker
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let reasoning = row
                    .preset
                    .reasoning_effort
                    .map_or_else(|| "off".into(), |effort| effort.to_string());
                let line = format!(
                    "{} {}  {}/{} · reasoning {}",
                    if index == picker.selected_idx {
                        "▸"
                    } else {
                        " "
                    },
                    row.name,
                    row.preset.provider,
                    row.preset.model,
                    reasoning
                );
                let style = if index == picker.selected_idx {
                    Style::default()
                        .fg(palette.accent_foreground)
                        .bg(palette.accent)
                } else {
                    Style::default()
                };
                ListItem::new(Span::styled(line, style))
            })
            .collect()
    };
    frame.render_widget(List::new(items), chunks[1]);
    let footer = picker.error.as_deref().map_or_else(
        || "Enter apply · Esc cancel · ↑↓ navigate".into(),
        crate::terminal_text::sanitize_terminal_text,
    );
    frame.render_widget(
        Paragraph::new(footer)
            .style(Style::default().fg(if picker.error.is_some() {
                palette.error
            } else {
                palette.muted
            }))
            .wrap(Wrap { trim: false }),
        chunks[2],
    );
}

fn render_route_preset_overwrite(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(confirmation) = state.route_preset_overwrite.as_ref() else {
        return;
    };
    let palette = palette(state);
    let width = 70.min(area.width.saturating_sub(2));
    let height = 14.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Overwrite route preset? ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.warning));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let reasoning = confirmation
        .preset
        .reasoning_effort
        .map_or_else(|| "off".into(), |effort| effort.to_string());
    let cancel_style = if confirmation.save_selected {
        Style::default()
    } else {
        Style::default()
            .fg(palette.accent_foreground)
            .bg(palette.accent)
    };
    let save_style = if confirmation.save_selected {
        Style::default()
            .fg(palette.accent_foreground)
            .bg(palette.accent)
    } else {
        Style::default()
    };
    let footer = if confirmation.busy {
        "Saving… controls locked".into()
    } else if let Some(error) = confirmation.error.as_deref() {
        crate::terminal_text::sanitize_terminal_text(error)
    } else {
        "Enter choose · ←→ toggle · Esc cancel".into()
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Replace the existing named route preset?"),
            Line::from(""),
            Line::from(format!("Name: {}", confirmation.name)),
            Line::from(format!(
                "Route: {}/{}",
                confirmation.preset.provider, confirmation.preset.model
            )),
            Line::from(format!("Reasoning: {reasoning}")),
            Line::from(""),
            Line::from(vec![
                Span::styled(" Cancel ", cancel_style),
                Span::raw("   "),
                Span::styled(" Save ", save_style),
            ]),
            Line::from(""),
            Line::styled(
                footer,
                Style::default().fg(if confirmation.error.is_some() {
                    palette.error
                } else {
                    palette.muted
                }),
            ),
        ])
        .wrap(Wrap { trim: false }),
        inner,
    );
}

fn render_statusline_manager(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(manager) = state.statusline_manager.as_ref() else {
        return;
    };
    let palette = palette(state);
    let width = 78.min(area.width.saturating_sub(2));
    let height = 25.min(area.height.saturating_sub(2));
    let popup = centered_rect(width, height, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Statusline ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(palette.accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(4),
        ])
        .split(inner);
    let custom = if !manager.custom_configured {
        "not configured"
    } else if manager.custom_enabled {
        "enabled"
    } else {
        "disabled"
    };
    let command_status = if manager.testing || state.status_command.running {
        "testing…".to_string()
    } else if let Some(error) = state.status_command.error.as_deref() {
        crate::terminal_text::sanitize_terminal_text(error)
    } else if let Some(output) = state.status_command.output.as_deref() {
        format!(
            "last: {}",
            truncate(
                &crate::terminal_text::sanitize_terminal_text(output),
                chunks[0].width.saturating_sub(6) as usize,
            )
        )
    } else {
        "not run".into()
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!(
                "Custom status: {custom} · refresh {}s",
                manager.refresh_secs
            )),
            Line::from(Span::styled(
                command_status,
                Style::default().fg(palette.muted),
            )),
            Line::from(Span::styled(
                "The configured command text is never shown here.",
                Style::default().fg(palette.muted),
            )),
        ]),
        chunks[0],
    );

    let items = crate::ui_settings::StatusField::ALL
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let position = manager.fields.iter().position(|current| current == field);
            let marker =
                position.map_or_else(|| "off".into(), |position| format!("{:>2}", position + 1));
            let line = format!(" [{marker}] {}", field.label());
            let style = if index == manager.selected_idx {
                Style::default()
                    .fg(palette.accent_foreground)
                    .bg(palette.accent)
            } else if position.is_some() {
                Style::default().fg(palette.success)
            } else {
                Style::default().fg(palette.muted)
            };
            ListItem::new(Span::styled(line, style))
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), chunks[1]);

    let mut footer = if manager.busy {
        "Saving… controls locked".to_string()
    } else if inner.width < 66 {
        "Space toggle · ⇧↑↓ reorder · e custom\n⇧T test · Enter save · Esc cancel".into()
    } else {
        "Space toggle · Shift+↑/↓ reorder · e enable custom\nShift+T test · Enter save · Esc cancel"
            .into()
    };
    if let Some(error) = &manager.error {
        footer.push('\n');
        footer.push_str(&crate::terminal_text::sanitize_terminal_text(error));
    } else if let Some(notice) = &manager.notice {
        footer.push('\n');
        footer.push_str(&crate::terminal_text::sanitize_terminal_text(notice));
    } else if !state.run_capabilities.config_writes {
        footer.push_str("\nBare profile: preview only; persistent changes disabled");
    }
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(if manager.error.is_some() {
            palette.error
        } else {
            palette.muted
        })),
        chunks[2],
    );
}

#[cfg(test)]
mod orc290_render_tests {
    use super::*;
    #[test]
    fn orc290_approval_shows_final_contents_without_internal_ticket() {
        let approval = crate::app::PendingApproval { tool_name: "write_file".into(), tool_id: "call".into(), tool_args: serde_json::json!({"path":"sample.rs","content":"raw","_ca_edit_token":"private-ticket","_ca_edit_preview":"FORMATTED\n"}).to_string(), network_requested: false, email_preview: None, email_draft_ref: None };
        let lines = approval_argument_lines(&approval)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(lines.contains("final edit contents"));
        assert!(lines.contains("FORMATTED"));
        assert!(!lines.contains("private-ticket"));
        assert!(!lines.contains("_ca_edit"));
    }
}

#[cfg(test)]
mod agy295_inline_render_tests {
    use super::*;

    #[test]
    fn agy295_inline_editor_wraps_unicode_and_keeps_cursor_visible() {
        for width in [60, 100] {
            let state = crate::tui::test_support::test_state();
            let approval = crate::app::PendingApproval {
                tool_name: "run_command".into(),
                tool_args: serde_json::json!({"command": format!("printf '{}\\n'\n{}\nprintf tail", "界".repeat(80), "echo row\n".repeat(16))}).to_string(),
                tool_id: "render-amend".into(),
                network_requested: false,
                email_preview: None,
                email_draft_ref: None,
            };
            let mut editor = crate::approval_amend::CommandEditor::new(&approval).unwrap();
            let theme = palette(&state);
            for cursor in [0, editor.text.len()] {
                editor.cursor = cursor;
                let lines = inline_command_editor_lines(&editor, width, &theme);
                assert!(lines.iter().all(|line| line.width() <= usize::from(width)));
                assert!(lines.iter().any(|line| line
                    .spans
                    .iter()
                    .any(|span| span.style.bg == Some(theme.text))));
                assert!(lines.len() <= 12);
                if cursor == editor.text.len() {
                    assert!(lines
                        .iter()
                        .any(|line| line.to_string().contains("printf tail")));
                }
            }
        }
    }
}
