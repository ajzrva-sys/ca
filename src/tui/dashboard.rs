//! Full-screen agent dashboard and tabbed usage modal.
//!
//! Layout follows the Grok Build dashboard the user asked to match: location
//! header, action row, state groups, dispatch prompt, and a three-tab usage
//! overlay. Live rows are immutable observations of every local pane and child.

use crate::app::AppState;
use crate::tui::selection::truncate_cells;
use chrono::{DateTime, Utc};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy)]
struct DashboardColors {
    accent: Color,
    accent_foreground: Color,
    text: Color,
    muted: Color,
    surface: Color,
    border: Color,
    heading: Color,
    working: Color,
    idle: Color,
}

fn colors(state: &AppState) -> DashboardColors {
    let theme = super::render::palette(state);
    DashboardColors {
        accent: theme.accent,
        accent_foreground: theme.accent_foreground,
        text: theme.text,
        muted: theme.muted,
        surface: theme.surface,
        border: theme.border,
        heading: theme.heading,
        working: theme.state_working,
        idle: theme.state_idle,
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(area.height.saturating_sub(height) / 2),
            Constraint::Length(height),
            Constraint::Length(area.height.saturating_sub(height) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(area.width.saturating_sub(width) / 2),
            Constraint::Length(width),
            Constraint::Length(area.width.saturating_sub(width) / 2),
        ])
        .split(v[1])[1]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UsageTab {
    Context,
    #[default]
    Limit,
    Session,
}

impl UsageTab {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Context => "Context usage",
            Self::Limit => "Usage limit",
            Self::Session => "Session info",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Context => Self::Limit,
            Self::Limit => Self::Session,
            Self::Session => Self::Context,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Context => Self::Session,
            Self::Limit => Self::Context,
            Self::Session => Self::Limit,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageModalState {
    pub tab: UsageTab,
    pub scroll: u16,
    pub context_body: String,
    pub limit_body: String,
    pub session_body: String,
    pub context_percent: Option<u8>,
    pub limit_percent: Option<u8>,
    pub limit_title: String,
    pub limit_reset: Option<String>,
}

impl UsageModalState {
    fn body(&self) -> &str {
        match self.tab {
            UsageTab::Context => &self.context_body,
            UsageTab::Limit => &self.limit_body,
            UsageTab::Session => &self.session_body,
        }
    }

    fn percent(&self) -> Option<u8> {
        match self.tab {
            UsageTab::Context => self.context_percent,
            UsageTab::Limit => Some(self.limit_percent.unwrap_or(0)),
            UsageTab::Session => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardGroup {
    NeedsInput,
    Working,
    Idle,
    Inactive,
}

impl DashboardGroup {
    pub const fn label(self) -> &'static str {
        match self {
            Self::NeedsInput => "Needs input",
            Self::Working => "Working",
            Self::Idle => "Idle",
            Self::Inactive => "Inactive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DashboardCursor {
    #[default]
    NewAgent,
    Worktree,
    OpenPrevious,
    Section(DashboardGroup),
    Row {
        group: DashboardGroup,
        index: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardRowSource {
    Current,
    Pane {
        pane_id: crate::panes::PaneId,
        session_id: Option<uuid::Uuid>,
    },
    Child {
        pane_id: crate::panes::PaneId,
        session_id: Option<uuid::Uuid>,
        agent_id: crate::agent::subagent::AgentId,
        parent_turn: uuid::Uuid,
    },
    External {
        identity: crate::dashboard_snapshot::DashboardIdentity,
    },
    Saved {
        storage_key: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardRow {
    pub group: DashboardGroup,
    pub title: String,
    pub subtitle: String,
    pub workspace: String,
    pub age: String,
    pub source: DashboardRowSource,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalChildFocus {
    pub pane_id: crate::panes::PaneId,
    pub session_id: Option<uuid::Uuid>,
    pub agent_id: crate::agent::subagent::AgentId,
    pub parent_turn: uuid::Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardState {
    pub open: bool,
    pub cursor: DashboardCursor,
    pub list_focus: bool,
    pub collapsed_working: bool,
    pub collapsed_needs_input: bool,
    pub collapsed_idle: bool,
    pub collapsed_inactive: bool,
    pub worktree_next: bool,
    pub now_override: Option<DateTime<Utc>>,
    pub pending_dispatch: Option<String>,
    pub selected_source: Option<DashboardRowSource>,
    pub pending_child_focus: Option<LocalChildFocus>,
    pub external_entries: Vec<crate::dashboard_snapshot::DashboardEntry>,
    pub presence_status: Option<String>,
}

impl Default for DashboardState {
    fn default() -> Self {
        Self {
            open: false,
            cursor: DashboardCursor::NewAgent,
            list_focus: true,
            collapsed_working: false,
            collapsed_needs_input: false,
            collapsed_idle: false,
            collapsed_inactive: true,
            worktree_next: false,
            now_override: None,
            pending_dispatch: None,
            selected_source: None,
            pending_child_focus: None,
            external_entries: Vec::new(),
            presence_status: None,
        }
    }
}

pub fn open(state: &mut AppState) {
    state.dashboard.open = true;
    state.dashboard.cursor = DashboardCursor::NewAgent;
    state.dashboard.selected_source = None;
    state.dashboard.list_focus = true;
    state.dashboard.collapsed_inactive = true;
    state.pager_open = false;
    state.session_picker_open = false;
}

pub fn close(state: &mut AppState) {
    state.dashboard.open = false;
    state.usage_modal = None;
}

fn now(state: &AppState) -> DateTime<Utc> {
    state.dashboard.now_override.unwrap_or_else(Utc::now)
}

pub fn format_age(updated: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = (now - updated).num_seconds().max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

fn workspace_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

pub fn shorten_path(path: &str) -> String {
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return "/".into();
    }
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() <= 2 {
        return if path.starts_with('/') {
            format!("/{path}", path = parts.join("/"))
        } else {
            parts.join("/")
        };
    }
    let mut out = String::new();
    if path.starts_with('/') {
        out.push('/');
    }
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            out.push('/');
        }
        if index + 2 < parts.len() {
            match part.chars().next() {
                Some(ch) => out.push(ch),
                None => out.push('?'),
            }
        } else {
            out.push_str(part);
        }
    }
    out
}

fn current_title(state: &AppState) -> String {
    let title = state.session_name.trim();
    if !title.is_empty() {
        return crate::terminal_text::sanitize_terminal_text(title);
    }
    state
        .messages
        .iter()
        .find(|message| message.role == crate::app::Role::User)
        .map(|message| {
            let line = message.content.lines().next().unwrap_or("").trim();
            crate::terminal_text::sanitize_terminal_text(line)
        })
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "Untitled session".into())
}

fn current_subtitle(state: &AppState) -> String {
    if state.approval_queue.is_active() || state.interaction.is_some() {
        return "Waiting for your input".into();
    }
    if state.turn_lifecycle.is_active() || state.streaming.is_some() {
        return match state.turn_lifecycle.phase {
            crate::activity::ActivityPhase::Sending => "Sending…",
            crate::activity::ActivityPhase::Waiting => "Waiting for response…",
            crate::activity::ActivityPhase::Reasoning => "Thinking…",
            crate::activity::ActivityPhase::Responding => "Responding…",
            crate::activity::ActivityPhase::Tool => "Executing tools…",
            crate::activity::ActivityPhase::Interaction => "Waiting for your input",
            crate::activity::ActivityPhase::Paused => "Paused",
            crate::activity::ActivityPhase::Idle => "Working…",
        }
        .into();
    }
    state
        .messages
        .iter()
        .rev()
        .find(|message| {
            matches!(
                message.role,
                crate::app::Role::Assistant | crate::app::Role::User
            ) && !message.content.trim().is_empty()
        })
        .map(|message| {
            crate::terminal_text::sanitize_terminal_text(
                message.content.lines().next().unwrap_or(""),
            )
        })
        .unwrap_or_default()
}

pub fn build_rows(state: &AppState) -> Vec<DashboardRow> {
    let now = now(state);
    let mut rows = Vec::new();
    let mut entries = crate::dashboard_snapshot::capture_local(state);
    entries.extend(state.dashboard.external_entries.iter().take(512).cloned());
    let live_sessions = entries
        .iter()
        .filter_map(|entry| entry.identity.session_id)
        .collect::<std::collections::HashSet<_>>();
    for entry in entries {
        use crate::dashboard_snapshot::LiveStatus;
        let multiple_panes = state
            .pane_deck
            .as_ref()
            .is_some_and(|deck| deck.ids().len() > 1);
        if !entry.external
            && !multiple_panes
            && entry.identity.session_id.is_none()
            && entry.identity.child_id.is_none()
            && entry.status == LiveStatus::Idle
            && entry.counters.messages == 0
            && entry.counters.active_children == 0
        {
            continue;
        }
        let group = match entry.status {
            LiveStatus::NeedsInput => DashboardGroup::NeedsInput,
            LiveStatus::Working => DashboardGroup::Working,
            LiveStatus::Stale | LiveStatus::Unavailable => DashboardGroup::Inactive,
            _ => DashboardGroup::Idle,
        };
        let source = if entry.external {
            DashboardRowSource::External {
                identity: entry.identity.clone(),
            }
        } else if let Some(child) = entry.identity.child_id {
            DashboardRowSource::Child {
                pane_id: entry.identity.pane_id.into(),
                session_id: entry.identity.session_id,
                agent_id: crate::agent::subagent::AgentId(child),
                parent_turn: entry
                    .parent_turn
                    .expect("local children have a parent turn"),
            }
        } else if entry.identity.pane_id == state.pane_id.get() {
            DashboardRowSource::Current
        } else {
            DashboardRowSource::Pane {
                pane_id: entry.identity.pane_id.into(),
                session_id: entry.identity.session_id,
            }
        };
        let current = source == DashboardRowSource::Current;
        rows.push(DashboardRow {
            group,
            title: if current {
                current_title(state)
            } else {
                entry.title
            },
            subtitle: if current {
                current_subtitle(state)
            } else {
                format!("Pane {} · {}", entry.identity.pane_id, entry.activity)
            },
            workspace: workspace_label(&entry.workspace),
            age: format_age(entry.updated_at, now),
            source,
            preview: entry.preview,
        });
    }
    for summary in &state.session_list {
        if live_sessions.contains(&summary.id) {
            continue;
        }
        let title = if summary.title.trim().is_empty() {
            "Untitled session".into()
        } else {
            crate::terminal_text::sanitize_terminal_text(&summary.title)
        };
        rows.push(DashboardRow {
            group: DashboardGroup::Inactive,
            title,
            subtitle: crate::terminal_text::sanitize_terminal_text(&summary.workspace),
            workspace: workspace_label(&summary.workspace),
            age: format_age(summary.updated_at, now),
            source: DashboardRowSource::Saved {
                storage_key: summary.storage_key.clone(),
            },
            preview: "Saved session · open to view the conversation".into(),
        });
    }
    rows
}

fn group_collapsed(state: &AppState, group: DashboardGroup) -> bool {
    match group {
        DashboardGroup::NeedsInput => state.dashboard.collapsed_needs_input,
        DashboardGroup::Working => state.dashboard.collapsed_working,
        DashboardGroup::Idle => state.dashboard.collapsed_idle,
        DashboardGroup::Inactive => state.dashboard.collapsed_inactive,
    }
}

fn set_group_collapsed(state: &mut AppState, group: DashboardGroup, collapsed: bool) {
    match group {
        DashboardGroup::NeedsInput => state.dashboard.collapsed_needs_input = collapsed,
        DashboardGroup::Working => state.dashboard.collapsed_working = collapsed,
        DashboardGroup::Idle => state.dashboard.collapsed_idle = collapsed,
        DashboardGroup::Inactive => state.dashboard.collapsed_inactive = collapsed,
    }
}

fn toggle_group(state: &mut AppState, group: DashboardGroup) {
    let collapsed = group_collapsed(state, group);
    set_group_collapsed(state, group, !collapsed);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavItem {
    NewAgent,
    Worktree,
    OpenPrevious,
    Section(DashboardGroup),
    Row { group: DashboardGroup, index: usize },
}

fn nav_items(state: &AppState, rows: &[DashboardRow]) -> Vec<NavItem> {
    let mut items = vec![NavItem::NewAgent, NavItem::Worktree];
    if state.has_previous_sessions() {
        items.push(NavItem::OpenPrevious);
    }
    for group in [
        DashboardGroup::NeedsInput,
        DashboardGroup::Working,
        DashboardGroup::Idle,
        DashboardGroup::Inactive,
    ] {
        let count = rows.iter().filter(|row| row.group == group).count();
        if count == 0 {
            continue;
        }
        items.push(NavItem::Section(group));
        if group_collapsed(state, group) {
            continue;
        }
        for index in 0..count {
            items.push(NavItem::Row { group, index });
        }
    }
    items
}

fn cursor_from_nav(item: NavItem) -> DashboardCursor {
    match item {
        NavItem::NewAgent => DashboardCursor::NewAgent,
        NavItem::Worktree => DashboardCursor::Worktree,
        NavItem::OpenPrevious => DashboardCursor::OpenPrevious,
        NavItem::Section(group) => DashboardCursor::Section(group),
        NavItem::Row { group, index } => DashboardCursor::Row { group, index },
    }
}

fn nav_from_cursor(cursor: DashboardCursor) -> NavItem {
    match cursor {
        DashboardCursor::NewAgent => NavItem::NewAgent,
        DashboardCursor::Worktree => NavItem::Worktree,
        DashboardCursor::OpenPrevious => NavItem::OpenPrevious,
        DashboardCursor::Section(group) => NavItem::Section(group),
        DashboardCursor::Row { group, index } => NavItem::Row { group, index },
    }
}

fn move_cursor(state: &mut AppState, delta: isize) {
    let rows = build_rows(state);
    let items = nav_items(state, &rows);
    if items.is_empty() {
        state.dashboard.cursor = DashboardCursor::NewAgent;
        return;
    }
    let current = selected_cursor(state, &rows)
        .map(nav_from_cursor)
        .unwrap_or_else(|| nav_from_cursor(state.dashboard.cursor));
    let index = items.iter().position(|item| *item == current).unwrap_or(0);
    let next = if delta < 0 {
        index.saturating_sub(delta.unsigned_abs())
    } else {
        (index + delta as usize).min(items.len().saturating_sub(1))
    };
    state.dashboard.cursor = cursor_from_nav(items[next]);
    state.dashboard.selected_source =
        row_for_cursor(&rows, state.dashboard.cursor).map(|row| row.source.clone());
}

fn row_for_cursor(rows: &[DashboardRow], cursor: DashboardCursor) -> Option<&DashboardRow> {
    let DashboardCursor::Row { group, index } = cursor else {
        return None;
    };
    rows.iter().filter(|row| row.group == group).nth(index)
}

fn selected_cursor(state: &AppState, rows: &[DashboardRow]) -> Option<DashboardCursor> {
    let source = state.dashboard.selected_source.as_ref()?;
    let row = rows.iter().find(|row| &row.source == source)?;
    let index = rows
        .iter()
        .filter(|candidate| candidate.group == row.group)
        .position(|candidate| &candidate.source == source)?;
    Some(DashboardCursor::Row {
        group: row.group,
        index,
    })
}

#[cfg(test)]
fn meter(percent: u8, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let filled = (u16::from(percent) as usize * width / 100).min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

fn location_label(state: &AppState) -> String {
    let path = shorten_path(&state.cwd);
    match state
        .status_git_branch
        .as_deref()
        .filter(|branch| !branch.is_empty())
    {
        Some(branch) => format!("{branch} {path}"),
        None => path,
    }
}

pub(super) fn render_with_version_label(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &AppState,
    version_label: &str,
) {
    let theme = colors(state);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface).fg(theme.text)),
        area,
    );
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, chunks[0], state, version_label);
    render_actions(frame, chunks[1], state);
    render_roster(frame, chunks[2], state);
    render_dispatch(frame, chunks[3], state);
    render_footer(frame, chunks[4], state);
    if state.usage_modal.is_some() {
        render_usage_modal(frame, area, state);
    }
}

fn render_header(frame: &mut ratatui::Frame, area: Rect, state: &AppState, version_label: &str) {
    if area.height == 0 {
        return;
    }
    let theme = colors(state);
    let rows = build_rows(state);
    let working = rows
        .iter()
        .filter(|row| row.group == DashboardGroup::Working)
        .count();
    let left = format!(
        "{version_label} │ {}  [Choose Ctrl+l]",
        location_label(state)
    );
    let right = if working == 0 {
        String::new()
    } else {
        format!(":: {working} working")
    };
    let left = truncate_cells(&left, area.width as usize);
    let right = truncate_cells(&right, area.width as usize);
    let gap = area
        .width
        .saturating_sub(UnicodeWidthStr::width(left.as_str()) as u16)
        .saturating_sub(UnicodeWidthStr::width(right.as_str()) as u16);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(left, Style::default().fg(theme.text)),
            Span::raw(" ".repeat(gap as usize)),
            Span::styled(right, Style::default().fg(theme.working)),
        ])),
        area,
    );
}

fn action_style(state: &AppState, cursor: DashboardCursor, theme: DashboardColors) -> Style {
    if state.dashboard.cursor == cursor {
        Style::default()
            .fg(theme.accent_foreground)
            .bg(theme.accent)
    } else {
        Style::default().fg(theme.muted)
    }
}

fn render_actions(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if area.height == 0 {
        return;
    }
    let theme = colors(state);
    let new_label = if state.dashboard.worktree_next {
        "+ New Agent in Worktree"
    } else {
        "+ New Agent"
    };
    let worktree_label = if state.dashboard.worktree_next {
        "Disable Worktree"
    } else {
        "Worktree Ctrl+w"
    };
    let previous = if state.has_previous_sessions() {
        "Open Previous /resume"
    } else {
        ""
    };
    let left = Line::from(vec![
        Span::styled(
            format!(" {new_label} "),
            action_style(state, DashboardCursor::NewAgent, theme),
        ),
        Span::raw("  "),
        Span::styled(
            previous,
            action_style(state, DashboardCursor::OpenPrevious, theme),
        ),
    ]);
    let right = Span::styled(
        format!(" {worktree_label} "),
        action_style(state, DashboardCursor::Worktree, theme),
    );
    let left_width = left.width() as u16;
    let right_width = UnicodeWidthStr::width(right.content.as_ref()) as u16;
    let gap = area
        .width
        .saturating_sub(left_width)
        .saturating_sub(right_width);
    let mut spans = left.spans;
    spans.push(Span::raw(" ".repeat(gap as usize)));
    spans.push(right);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_roster(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if area.width < 90 || build_rows(state).is_empty() {
        render_roster_list(frame, area, state);
        return;
    }
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);
    render_roster_list(frame, columns[0], state);
    let rows = build_rows(state);
    let cursor = selected_cursor(state, &rows).unwrap_or(state.dashboard.cursor);
    let row = row_for_cursor(&rows, cursor);
    let preview = row
        .map(|row| row.preview.as_str())
        .filter(|text| !text.is_empty())
        .unwrap_or("Select a session or agent to preview its latest activity");
    let title = row.map(|row| row.title.as_str()).unwrap_or("Preview");
    frame.render_widget(
        Paragraph::new(preview)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .title(truncate_cells(
                        title,
                        columns[1].width.saturating_sub(4) as usize,
                    ))
                    .borders(Borders::ALL),
            )
            .style(Style::default().fg(colors(state).text)),
        columns[1],
    );
}

fn render_roster_list(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let theme = colors(state);
    let rows = build_rows(state);
    let mut lines = Vec::new();
    for group in [
        DashboardGroup::NeedsInput,
        DashboardGroup::Working,
        DashboardGroup::Idle,
        DashboardGroup::Inactive,
    ] {
        let group_rows: Vec<&DashboardRow> = rows.iter().filter(|row| row.group == group).collect();
        if group_rows.is_empty() {
            continue;
        }
        let collapsed = group_collapsed(state, group);
        let marker = if collapsed { "▸" } else { "▾" };
        let selected = state.dashboard.cursor == DashboardCursor::Section(group);
        let header = format!(" {marker} {} {} ", group.label(), group_rows.len());
        let header_width = UnicodeWidthStr::width(header.as_str()) as u16;
        let rule = "─".repeat(area.width.saturating_sub(header_width).saturating_sub(1) as usize);
        let style = if selected {
            Style::default()
                .fg(theme.accent_foreground)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme.heading)
                .add_modifier(Modifier::BOLD)
        };
        lines.push(Line::from(vec![
            Span::styled(header, style),
            Span::styled(rule, Style::default().fg(theme.border)),
        ]));
        if collapsed {
            continue;
        }
        for (index, row) in group_rows.into_iter().enumerate() {
            let selected = if let Some(source) = &state.dashboard.selected_source {
                &row.source == source
            } else {
                state.dashboard.cursor == DashboardCursor::Row { group, index }
            };
            let icon = match group {
                DashboardGroup::NeedsInput => "!",
                DashboardGroup::Working => "::",
                DashboardGroup::Idle => "○",
                DashboardGroup::Inactive => "◇",
            };
            let icon_color = match group {
                DashboardGroup::NeedsInput => theme.accent,
                DashboardGroup::Working => theme.working,
                DashboardGroup::Idle => theme.idle,
                DashboardGroup::Inactive => theme.muted,
            };
            let prefix = if selected { "▌" } else { " " };
            let title = format!("{icon} {} · {}", row.title, row.workspace);
            let age = row.age.clone();
            let row_width = area.width as usize;
            let age_width = UnicodeWidthStr::width(age.as_str());
            let title = truncate_cells(&title, row_width.saturating_sub(age_width + 3));
            let gap = " ".repeat(
                row_width
                    .saturating_sub(1)
                    .saturating_sub(UnicodeWidthStr::width(title.as_str()))
                    .saturating_sub(age_width),
            );
            let row_style = if selected {
                Style::default()
                    .fg(theme.accent_foreground)
                    .bg(theme.accent)
            } else {
                Style::default().fg(theme.text)
            };
            lines.push(Line::from(vec![
                Span::styled(prefix, Style::default().fg(theme.accent)),
                Span::styled(
                    format!("{title}{gap}{age}"),
                    row_style.fg(if selected {
                        theme.accent_foreground
                    } else {
                        icon_color
                    }),
                ),
            ]));
            if !row.subtitle.is_empty() {
                let subtitle = truncate_cells(&format!("   {}", row.subtitle), row_width);
                lines.push(Line::from(Span::styled(
                    subtitle,
                    if selected {
                        Style::default()
                            .fg(theme.accent_foreground)
                            .bg(theme.accent)
                    } else {
                        Style::default().fg(theme.muted)
                    },
                )));
            }
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No sessions yet · type a prompt to dispatch a new agent",
            Style::default().fg(theme.muted),
        )));
    }
    let visible = area.height as usize;
    let offset = match state.dashboard.cursor {
        DashboardCursor::Section(_) | DashboardCursor::Row { .. } => lines
            .len()
            .saturating_sub(visible)
            .min(lines.len().saturating_sub(1)),
        _ => 0,
    };
    let window: Vec<Line> = lines.into_iter().skip(offset).take(visible).collect();
    frame.render_widget(Paragraph::new(window), area);
}

fn approval_label(state: &AppState) -> &'static str {
    match state.approval {
        crate::app::ApprovalMode::RunEverything => "always-approve",
        crate::app::ApprovalMode::AutoReview => "auto-review",
        crate::app::ApprovalMode::Allowlist => "allowlist",
    }
}

fn render_dispatch(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let theme = colors(state);
    let focused = !state.dashboard.list_focus && state.usage_modal.is_none();
    let border = if focused { theme.accent } else { theme.border };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 {
        return;
    }
    let model = state
        .current_model
        .as_ref()
        .map(|model| model.display_name.as_str())
        .unwrap_or("ca");
    let badge = format!("{model} · {}", approval_label(state));
    let prompt = if state.input.is_empty() {
        Line::from(vec![
            Span::styled(
                "> ",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Dispatch a new agent", Style::default().fg(theme.muted)),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                "> ",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                truncate_cells(
                    &state.input.replace('\n', " "),
                    inner.width.saturating_sub(2) as usize,
                ),
                Style::default().fg(theme.text),
            ),
        ])
    };
    frame.render_widget(Paragraph::new(prompt), inner);
    if inner.width > 4 {
        let badge_width = UnicodeWidthStr::width(badge.as_str()) as u16;
        let x = inner
            .x
            .saturating_add(inner.width.saturating_sub(badge_width));
        let y = area.y.saturating_add(area.height.saturating_sub(1));
        frame.render_widget(
            Paragraph::new(Span::styled(badge, Style::default().fg(theme.muted))),
            Rect::new(x, y, badge_width.min(inner.width), 1),
        );
    }
}

fn render_footer(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if area.height == 0 {
        return;
    }
    let theme = colors(state);
    let text = match state.dashboard.cursor {
        DashboardCursor::Section(_) => "Enter:collapse  |  Esc:New Agent  |  Ctrl+.:shortcuts",
        DashboardCursor::Row { .. } => "Enter:open  |  Esc:New Agent  |  Ctrl+.:shortcuts",
        DashboardCursor::Worktree => "Enter:toggle worktree  |  Esc:New Agent  |  Ctrl+.:shortcuts",
        DashboardCursor::OpenPrevious => {
            "Enter:open previous  |  Esc:New Agent  |  Ctrl+.:shortcuts"
        }
        DashboardCursor::NewAgent => "Enter:new session  |  Esc:close  |  Ctrl+.:shortcuts",
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            state.dashboard.presence_status.as_deref().unwrap_or(text),
            Style::default().fg(theme.muted),
        )),
        area,
    );
}

pub fn render_usage_modal(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let Some(modal) = state.usage_modal.as_ref() else {
        return;
    };
    let theme = colors(state);
    let width = 88.min(area.width.saturating_sub(6)).max(40);
    let height = 22.min(area.height.saturating_sub(4)).max(12);
    let modal_area = centered_rect(width, height, area);
    frame.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(theme.surface));
    let inner = block.inner(modal_area);
    frame.render_widget(block, modal_area);
    if inner.height < 4 {
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(inner);

    let mut tabs = Vec::new();
    for tab in [UsageTab::Context, UsageTab::Limit, UsageTab::Session] {
        let style = if tab == modal.tab {
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted)
        };
        if !tabs.is_empty() {
            tabs.push(Span::raw("  "));
        }
        tabs.push(Span::styled(tab.label(), style));
    }
    let close = "[x]";
    let tab_line = Line::from(tabs);
    let tab_width = tab_line.width() as u16;
    let close_width = UnicodeWidthStr::width(close) as u16;
    let gap = chunks[0]
        .width
        .saturating_sub(tab_width)
        .saturating_sub(close_width);
    let mut spans = tab_line.spans;
    spans.push(Span::raw(" ".repeat(gap as usize)));
    spans.push(Span::styled(close, Style::default().fg(theme.muted)));
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[0]);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(chunks[1].width as usize),
            Style::default().fg(theme.border),
        )),
        chunks[1],
    );

    let mut body_lines = Vec::new();
    if let Some(percent) = modal.percent() {
        let title = match modal.tab {
            UsageTab::Context => "Context window".to_string(),
            UsageTab::Limit => {
                if modal.limit_title.is_empty() {
                    "Usage limit".to_string()
                } else {
                    modal.limit_title.clone()
                }
            }
            UsageTab::Session => String::new(),
        };
        if !title.is_empty() {
            body_lines.push(Line::from(Span::styled(
                title,
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            )));
        }
        let bar_width = chunks[2].width.saturating_sub(8).min(24) as usize;
        let filled = (usize::from(percent) * bar_width / 100).min(bar_width);
        let mut bar_spans = Vec::new();
        if filled > 0 {
            bar_spans.push(Span::styled(
                "█".repeat(filled),
                Style::default().fg(theme.accent),
            ));
        }
        if bar_width > filled {
            bar_spans.push(Span::styled(
                "░".repeat(bar_width - filled),
                Style::default().fg(theme.muted),
            ));
        }
        bar_spans.push(Span::raw("  "));
        bar_spans.push(Span::styled(
            format!("{percent}%"),
            Style::default().fg(theme.text),
        ));
        body_lines.push(Line::from(bar_spans));
        if let Some(reset) = modal
            .limit_reset
            .as_ref()
            .filter(|_| modal.tab == UsageTab::Limit)
        {
            body_lines.push(Line::from(Span::styled(
                reset.clone(),
                Style::default().fg(theme.muted),
            )));
        }
        body_lines.push(Line::from(""));
    }
    let scroll = modal.scroll as usize;
    let detail_style = if modal.tab == UsageTab::Limit {
        Style::default().fg(theme.muted)
    } else {
        Style::default().fg(theme.text)
    };
    for line in modal.body().lines().skip(scroll) {
        body_lines.push(Line::from(Span::styled(
            crate::terminal_text::sanitize_terminal_text(line),
            detail_style,
        )));
    }
    frame.render_widget(
        Paragraph::new(body_lines).wrap(Wrap { trim: false }),
        chunks[2],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "Tab switch  |  ↑/↓ scroll  |  Esc close",
            Style::default().fg(theme.muted),
        ))
        .alignment(Alignment::Center),
        chunks[3],
    );
}

pub fn handle_usage_modal(state: &mut AppState, code: KeyCode, modifiers: KeyModifiers) -> bool {
    let Some(modal) = state.usage_modal.as_mut() else {
        return false;
    };
    match (modifiers, code) {
        (KeyModifiers::NONE, KeyCode::Esc) | (KeyModifiers::NONE, KeyCode::Char('x')) => {
            state.usage_modal = None;
        }
        (KeyModifiers::NONE, KeyCode::Tab) => {
            modal.tab = modal.tab.next();
            modal.scroll = 0;
        }
        (KeyModifiers::SHIFT, KeyCode::BackTab) | (KeyModifiers::SHIFT, KeyCode::Tab) => {
            modal.tab = modal.tab.previous();
            modal.scroll = 0;
        }
        (KeyModifiers::NONE, KeyCode::Up) => {
            modal.scroll = modal.scroll.saturating_sub(1);
        }
        (KeyModifiers::NONE, KeyCode::Down) => {
            modal.scroll = modal.scroll.saturating_add(1);
        }
        (KeyModifiers::NONE, KeyCode::PageUp) => {
            modal.scroll = modal.scroll.saturating_sub(8);
        }
        (KeyModifiers::NONE, KeyCode::PageDown) => {
            modal.scroll = modal.scroll.saturating_add(8);
        }
        _ => {}
    }
    true
}

fn insert_dispatch_char(state: &mut AppState, character: char) {
    state.dashboard.list_focus = false;
    if state.input_cursor > state.input.len() {
        state.input_cursor = state.input.len();
    }
    state.input.insert(state.input_cursor, character);
    state.input_cursor += character.len_utf8();
}

fn activate_cursor(state: &mut AppState) {
    let rows = build_rows(state);
    if state.dashboard.selected_source.is_some() && selected_cursor(state, &rows).is_none() {
        state.dashboard.selected_source = None;
        state.dashboard.cursor = DashboardCursor::NewAgent;
        state.status_message = Some("Selected activity is no longer available".into());
        return;
    }
    match selected_cursor(state, &rows).unwrap_or(state.dashboard.cursor) {
        DashboardCursor::NewAgent => {
            if state.input.trim().is_empty() {
                state.pending_input_action = Some(crate::tui::commands::InputAction::NewSession);
                close(state);
            } else {
                dispatch_prompt(state);
            }
        }
        DashboardCursor::Worktree => {
            state.dashboard.worktree_next = !state.dashboard.worktree_next;
            state.status_message = Some(if state.dashboard.worktree_next {
                "Worktree mode on · the next new session uses an isolated Git worktree".into()
            } else {
                "Worktree mode off".into()
            });
        }
        DashboardCursor::OpenPrevious => {
            if state.has_previous_sessions() {
                state.session_picker_view = crate::app::SessionPickerView::Active;
                state.selected_session_idx = 0;
                state.session_filter.clear();
                state.session_picker_error = None;
                state.session_picker_busy = false;
                state.session_picker_open = true;
            } else {
                state.status_message = Some("No previous sessions".into());
            }
        }
        DashboardCursor::Section(group) => toggle_group(state, group),
        DashboardCursor::Row { group, index } => {
            if let Some(row) = rows.iter().filter(|row| row.group == group).nth(index) {
                match &row.source {
                    DashboardRowSource::Current => close(state),
                    DashboardRowSource::Pane {
                        pane_id,
                        session_id,
                    } => {
                        let valid = state
                            .pane_deck
                            .as_ref()
                            .and_then(|deck| deck.pane(state, *pane_id))
                            .is_some_and(|pane| {
                                pane.current_session.as_ref().map(|session| session.id)
                                    == *session_id
                            });
                        if valid {
                            state.pane_request = Some(crate::panes::PaneRequest::Focus(*pane_id));
                            close(state);
                        } else {
                            state.status_message =
                                Some("Selected pane is no longer available".into());
                        }
                    }
                    DashboardRowSource::Child {
                        pane_id,
                        session_id,
                        agent_id,
                        parent_turn,
                    } => {
                        state.dashboard.pending_child_focus = Some(LocalChildFocus {
                            pane_id: *pane_id,
                            session_id: *session_id,
                            agent_id: *agent_id,
                            parent_turn: *parent_turn,
                        });
                        close(state);
                    }
                    DashboardRowSource::External { .. } => state.status_message = Some(
                        "External status only · open the owning terminal to control this session"
                            .into(),
                    ),
                    DashboardRowSource::Saved { storage_key } => {
                        state.pending_input_action = Some(
                            crate::tui::commands::InputAction::LoadSession(storage_key.clone()),
                        );
                        close(state);
                    }
                }
            }
        }
    }
}

fn dispatch_prompt(state: &mut AppState) {
    let prompt = state.input.trim().to_string();
    if prompt.is_empty() {
        return;
    }
    state.input.clear();
    state.input_cursor = 0;
    let has_content = !state.messages.is_empty()
        || state
            .current_session
            .as_ref()
            .is_some_and(|session| !session.messages.is_empty());
    close(state);
    if has_content {
        state.dashboard.pending_dispatch = Some(prompt);
        state.pending_input_action = Some(crate::tui::commands::InputAction::NewSession);
    } else {
        state.input = prompt;
        state.input_cursor = state.input.len();
        state.pending_composer_submit = true;
    }
}

pub fn handle(state: &mut AppState, code: KeyCode, modifiers: KeyModifiers) -> bool {
    if state.usage_modal.is_some() {
        return handle_usage_modal(state, code, modifiers);
    }
    if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('\\')) {
        close(state);
        return true;
    }
    if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('w')) {
        state.dashboard.cursor = DashboardCursor::Worktree;
        state.dashboard.selected_source = None;
        activate_cursor(state);
        return true;
    }
    if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('l')) {
        state.status_message = Some("Use /workspace to choose another directory".into());
        return true;
    }
    if modifiers.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('u')) {
        state.input.clear();
        state.input_cursor = 0;
        return true;
    }

    match (modifiers, code) {
        (KeyModifiers::NONE, KeyCode::Esc) => {
            if !state.input.is_empty() {
                state.input.clear();
                state.input_cursor = 0;
                return true;
            }
            if !matches!(state.dashboard.cursor, DashboardCursor::NewAgent) {
                state.dashboard.cursor = DashboardCursor::NewAgent;
                state.dashboard.selected_source = None;
                state.dashboard.list_focus = true;
                return true;
            }
            close(state);
            true
        }
        (KeyModifiers::NONE, KeyCode::Tab) => {
            state.dashboard.list_focus = !state.dashboard.list_focus;
            true
        }
        (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k'))
            if state.dashboard.list_focus || state.input.is_empty() =>
        {
            state.dashboard.list_focus = true;
            move_cursor(state, -1);
            true
        }
        (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j'))
            if state.dashboard.list_focus || state.input.is_empty() =>
        {
            state.dashboard.list_focus = true;
            move_cursor(state, 1);
            true
        }
        (KeyModifiers::NONE, KeyCode::Left) | (KeyModifiers::NONE, KeyCode::Char('h'))
            if state.dashboard.list_focus =>
        {
            match state.dashboard.cursor {
                DashboardCursor::Worktree => state.dashboard.cursor = DashboardCursor::OpenPrevious,
                DashboardCursor::OpenPrevious => state.dashboard.cursor = DashboardCursor::NewAgent,
                DashboardCursor::Section(group) => set_group_collapsed(state, group, true),
                _ => {}
            }
            true
        }
        (KeyModifiers::NONE, KeyCode::Right) | (KeyModifiers::NONE, KeyCode::Char('l'))
            if state.dashboard.list_focus =>
        {
            match state.dashboard.cursor {
                DashboardCursor::NewAgent => {
                    state.dashboard.cursor = if state.has_previous_sessions() {
                        DashboardCursor::OpenPrevious
                    } else {
                        DashboardCursor::Worktree
                    };
                }
                DashboardCursor::OpenPrevious => {
                    state.dashboard.cursor = DashboardCursor::Worktree;
                }
                DashboardCursor::Section(group) => set_group_collapsed(state, group, false),
                _ => {}
            }
            true
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            if !state.input.trim().is_empty() && !state.dashboard.list_focus {
                dispatch_prompt(state);
            } else {
                activate_cursor(state);
            }
            true
        }
        (KeyModifiers::NONE, KeyCode::Backspace) if !state.dashboard.list_focus => {
            if state.input_cursor > 0 {
                let cursor = crate::tui::composer::normalize_boundary(
                    &state.input,
                    state.input_cursor.saturating_sub(1),
                );
                state.input.drain(cursor..state.input_cursor);
                state.input_cursor = cursor;
            }
            true
        }
        (KeyModifiers::NONE, KeyCode::Char(character)) if !character.is_control() => {
            insert_dispatch_char(state, character);
            true
        }
        (KeyModifiers::SHIFT, KeyCode::Char(character)) if !character.is_control() => {
            insert_dispatch_char(state, character);
            true
        }
        _ => true,
    }
}

pub fn apply_pending_dispatch(state: &mut AppState) {
    if let Some(prompt) = state.dashboard.pending_dispatch.take() {
        state.input = prompt;
        state.input_cursor = state.input.len();
        state.pending_composer_submit = true;
        if state.dashboard.worktree_next {
            state.status_message = Some(
                "New session started · /worktree create makes an isolated worktree for this agent"
                    .into(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::test_support::test_state;

    #[test]
    fn dashboard_observes_all_panes_needs_input_and_preview_without_focus_change() {
        let mut state = test_state();
        let mut second = test_state();
        second.session_name = "Second live pane".into();
        second.interaction = Some(crate::interaction::InteractionState::host(
            crate::interaction::InteractionRequest::host_masked("Waiting for answer", None),
        ));
        second.messages.push(crate::app::Message {
            role: crate::app::Role::Assistant,
            content: "Second pane preview".into(),
            tool_calls: None,
            thinking: None,
            response_items: Vec::new(),
            timestamp: chrono::Utc::now(),
        });
        let mut deck = crate::panes::PaneDeck::single();
        let id = deck
            .split(
                &state,
                crate::panes::SplitAxis::Vertical,
                second,
                Rect::new(0, 0, 120, 40),
            )
            .unwrap();
        state.pane_deck = Some(Box::new(deck));
        let before_focus = state.pane_deck.as_ref().unwrap().focused();
        let rows = build_rows(&state);
        let waiting = rows
            .iter()
            .find(
                |row| matches!(row.source, DashboardRowSource::Pane {pane_id, ..} if pane_id == id),
            )
            .unwrap();
        assert_eq!(waiting.group, DashboardGroup::NeedsInput);
        assert_eq!(waiting.preview, "Second pane preview");
        assert_eq!(state.pane_deck.as_ref().unwrap().focused(), before_focus);
        state.dashboard.open = true;
        state.dashboard.cursor = DashboardCursor::Row {
            group: DashboardGroup::NeedsInput,
            index: 0,
        };
        state.dashboard.selected_source = Some(waiting.source.clone());
        let rendered = crate::tui::test_support::render_to_text(&state, 120, 40);
        assert!(rendered.contains("Needs input"));
        assert!(rendered.contains("Second pane preview"));
        assert_eq!(state.pane_deck.as_ref().unwrap().focused(), before_focus);
        activate_cursor(&mut state);
        assert_eq!(
            state.pane_request,
            Some(crate::panes::PaneRequest::Focus(id))
        );
    }

    #[test]
    fn external_selection_is_read_only_and_stable_when_rows_reorder() {
        let mut state = test_state();
        let external = crate::dashboard_snapshot::DashboardEntry {
            identity: crate::dashboard_snapshot::DashboardIdentity {
                instance_id: uuid::Uuid::new_v4(),
                pane_id: 1,
                session_id: None,
                child_id: None,
            },
            status: crate::dashboard_snapshot::LiveStatus::Working,
            title: "External status".into(),
            workspace: "/tmp".into(),
            activity: "Working".into(),
            preview: "Status only".into(),
            updated_at: chrono::Utc::now(),
            parent_turn: None,
            external: true,
            counters: Default::default(),
        };
        state.dashboard.external_entries.push(external.clone());
        state.dashboard.selected_source = Some(DashboardRowSource::External {
            identity: external.identity.clone(),
        });
        state.dashboard.cursor = DashboardCursor::Row {
            group: DashboardGroup::Working,
            index: 0,
        };
        state.dashboard.external_entries[0].status = crate::dashboard_snapshot::LiveStatus::Stale;
        activate_cursor(&mut state);
        assert!(state
            .status_message
            .as_deref()
            .unwrap()
            .contains("External status only"));
        assert!(state.pane_request.is_none());
        assert!(state.dashboard.pending_child_focus.is_none());
        state.dashboard.external_entries.clear();
        activate_cursor(&mut state);
        assert!(state
            .status_message
            .as_deref()
            .unwrap()
            .contains("no longer available"));
    }

    fn summary(title: &str, id: u128, minutes_ago: i64) -> crate::session::SessionSummary {
        crate::session::SessionSummary {
            storage_key: title.into(),
            id: uuid::Uuid::from_u128(id),
            title: title.into(),
            pinned: false,
            workspace: "/workspace/ca".into(),
            updated_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap()
                - chrono::Duration::minutes(minutes_ago),
        }
    }

    #[test]
    fn age_uses_compact_units() {
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        assert_eq!(format_age(now - chrono::Duration::seconds(12), now), "12s");
        assert_eq!(format_age(now - chrono::Duration::minutes(18), now), "18m");
        assert_eq!(format_age(now - chrono::Duration::hours(3), now), "3h");
        assert_eq!(format_age(now - chrono::Duration::days(2), now), "2d");
    }

    #[test]
    fn shorten_path_keeps_the_leaf_and_abbreviates_parents() {
        assert_eq!(
            shorten_path("/Volumes/projects/freebsd-localagent"),
            "/V/projects/freebsd-localagent"
        );
        assert_eq!(shorten_path("/workspace/ca"), "/workspace/ca");
    }

    #[test]
    fn roster_puts_the_live_turn_in_working_and_saved_sessions_in_inactive() {
        let mut state = test_state();
        state.dashboard.now_override = DateTime::from_timestamp(1_700_000_000, 0);
        state.session_name = "bsd2 MiMo Flash official Q2_K setup".into();
        state.turn_lifecycle.begin();
        state
            .turn_lifecycle
            .set_phase(crate::activity::ActivityPhase::Waiting);
        state.session_list = vec![
            summary("bsd2 local Qwen", 2, 180),
            summary("Review FreeBSD fusion", 3, 60 * 24 * 2),
        ];
        let rows = build_rows(&state);
        assert_eq!(rows[0].group, DashboardGroup::Working);
        assert_eq!(rows[0].title, "bsd2 MiMo Flash official Q2_K setup");
        assert_eq!(rows[0].subtitle, "Waiting for response…");
        assert_eq!(
            rows.iter()
                .filter(|row| row.group == DashboardGroup::Inactive)
                .count(),
            2
        );
    }

    #[test]
    fn usage_tabs_cycle_in_screenshot_order() {
        assert_eq!(UsageTab::Context.next(), UsageTab::Limit);
        assert_eq!(UsageTab::Limit.next(), UsageTab::Session);
        assert_eq!(UsageTab::Session.next(), UsageTab::Context);
    }

    #[test]
    fn meter_fills_from_the_left() {
        assert_eq!(meter(0, 4), "░░░░");
        assert_eq!(meter(50, 4), "██░░");
        assert_eq!(meter(100, 4), "████");
        assert_eq!(meter(47, 24).chars().filter(|ch| *ch == '█').count(), 11);
    }

    #[test]
    fn limit_tab_always_exposes_a_percent_for_the_bar() {
        let modal = UsageModalState {
            tab: UsageTab::Limit,
            scroll: 0,
            context_body: String::new(),
            limit_body: String::new(),
            session_body: String::new(),
            context_percent: None,
            limit_percent: None,
            limit_title: String::new(),
            limit_reset: None,
        };
        assert_eq!(modal.percent(), Some(0));
    }
}
