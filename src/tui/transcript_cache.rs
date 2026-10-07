//! Pane-local styled layout, with linear row indexes and bounded visible blocks.
use super::*;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::sync::{mpsc, Arc, Mutex};

const PAYLOAD_BUDGET: usize = 16 * 1024 * 1024;
const BLOCK_LINES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    revision: u64,
    identity: (crate::panes::PaneId, Option<uuid::Uuid>),
    generation: u64,
    width: u16,
    theme: ThemePalette,
    focused: bool,
    messages: usize,
    endpoints: (
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
    ),
    stream: Option<(usize, usize, usize, u64)>,
    expanded: Vec<TranscriptBlockKey>,
    flags: (bool, bool, bool, bool),
    annotations: (usize, usize, usize),
    history: Option<crate::app::SessionHistoryState>,
    agent_focus: Option<crate::app::AgentActivityFocusState>,
}

impl Key {
    fn for_state(state: &AppState, width: u16, focused: bool) -> Self {
        Self {
            revision: state.transcript_cache.revision.get(),
            identity: (state.pane_id, state.current_session.as_ref().map(|s| s.id)),
            generation: state.stream_generation,
            width,
            theme: palette(state),
            focused,
            messages: state.messages.len(),
            endpoints: (
                state.messages.first().map(|m| m.timestamp),
                state.messages.last().map(|m| m.timestamp),
            ),
            stream: state.streaming.as_ref().map(|s| {
                (
                    s.content.len(),
                    s.thinking.as_deref().map_or(0, str::len),
                    s.tool_calls.len(),
                    s.visible_reasoning_duration()
                        .map_or(0, |d| d.as_millis() as u64 / 100),
                )
            }),
            expanded: state.expanded_blocks.clone(),
            flags: (
                state.show_thinking,
                state.show_line_numbers,
                state.show_transcript_timestamps,
                state.show_assistant_footers,
            ),
            annotations: (
                state.transcript_annotations.len(),
                state.transcript_agent_summaries.len(),
                state.live_agent_activity.len(),
            ),
            history: state.session_history.clone(),
            agent_focus: state.agent_activity_focus.clone(),
        }
    }

    fn compatible(&self, next: &Self) -> bool {
        self.identity == next.identity
            && self.width == next.width
            && self.theme == next.theme
            && self.focused == next.focused
            && self.flags == next.flags
            && self.expanded == next.expanded
    }
}

#[derive(Debug)]
struct Block {
    lines: Vec<Line<'static>>,
    rows: Vec<u32>,
    bytes: usize,
    last_used: std::sync::atomic::AtomicU64,
}

#[derive(Debug)]
struct Layout {
    starts: Vec<u32>,
    blocks: BTreeMap<usize, Arc<Block>>,
    headers: Vec<ActivityHeaderLine>,
    links: Vec<crate::markdown::MarkdownLink>,
    bytes: usize,
    fallback: (u16, ratatui::buffer::Buffer),
}

fn line_bytes(line: &Line<'_>) -> usize {
    std::mem::size_of::<Line>()
        + line
            .spans
            .iter()
            .map(|s| std::mem::size_of::<Span>() + s.content.len())
            .sum::<usize>()
}

impl Layout {
    fn build(
        content: ConversationContent,
        width: u16,
        offset: u16,
        height: u16,
        trailing_rows: u16,
        previous: BTreeMap<usize, Arc<Block>>,
    ) -> Self {
        let mut starts = Vec::with_capacity(content.lines.len() + 1);
        starts.push(0u32);
        let mut blocks = BTreeMap::new();
        for (id, lines) in content.lines.chunks(BLOCK_LINES).enumerate() {
            let block = previous
                .get(&id)
                .filter(|old| old.lines == lines)
                .cloned()
                .unwrap_or_else(|| {
                    let rows = lines
                        .iter()
                        .map(|line| {
                            Paragraph::new(line.clone())
                                .wrap(Wrap { trim: false })
                                .line_count(width.max(1))
                                .min(u32::MAX as usize) as u32
                        })
                        .collect();
                    Arc::new(Block {
                        bytes: lines.iter().map(line_bytes).sum(),
                        lines: lines.to_vec(),
                        rows,
                        last_used: std::sync::atomic::AtomicU64::new(0),
                    })
                });
            for rows in &block.rows {
                starts.push(starts.last().unwrap().saturating_add(*rows));
            }
            blocks.insert(id, block);
        }
        let maximum = starts.last().copied().unwrap_or(0).min(u16::MAX as u32) as u16;
        let top = maximum
            .saturating_add(trailing_rows)
            .saturating_sub(height)
            .saturating_sub(offset);
        let mut fallback = ratatui::buffer::Buffer::empty(Rect::new(0, 0, width, height));
        // Materialize the requested viewport before evicting any oversized block.
        render_indexed(
            &content.lines,
            &starts,
            top,
            &mut fallback,
            Rect::new(0, 0, width, height),
        );
        let mut layout = Self {
            starts,
            blocks,
            headers: content.activity_headers,
            links: content.links,
            bytes: 0,
            fallback: (top, fallback),
        };
        layout.bytes = layout.blocks.values().map(|block| block.bytes).sum();
        let first_visible = layout
            .starts
            .partition_point(|row| *row <= u32::from(top))
            .saturating_sub(1)
            / BLOCK_LINES;
        let next_clock = layout
            .blocks
            .values()
            .map(|b| b.last_used.load(std::sync::atomic::Ordering::Relaxed))
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        if let Some(block) = layout.blocks.get(&first_visible) {
            block
                .last_used
                .store(next_clock, std::sync::atomic::Ordering::Relaxed);
        }
        while layout.bytes > PAYLOAD_BUDGET {
            let Some((&id, _)) = layout
                .blocks
                .iter()
                .min_by_key(|(_, b)| b.last_used.load(std::sync::atomic::Ordering::Relaxed))
            else {
                break;
            };
            layout.bytes -= layout.blocks.remove(&id).unwrap().bytes;
        }
        layout
    }

    fn rows(&self) -> u16 {
        self.starts
            .last()
            .copied()
            .unwrap_or(0)
            .min(u16::MAX as u32) as u16
    }

    fn has_view(&self, top: u16, height: u16) -> bool {
        if self.fallback.0 == top && self.fallback.1.area.height == height {
            return true;
        }
        let begin = self
            .starts
            .partition_point(|row| *row <= u32::from(top))
            .saturating_sub(1);
        let end = self
            .starts
            .partition_point(|row| *row < u32::from(top) + u32::from(height));
        (begin..end.min(self.starts.len().saturating_sub(1)))
            .all(|line| self.blocks.contains_key(&(line / BLOCK_LINES)))
    }

    fn draw(&self, frame: &mut ratatui::Frame, area: Rect, top: u16, clock: u64) {
        let begin = self
            .starts
            .partition_point(|row| *row <= u32::from(top))
            .saturating_sub(1);
        let end = self
            .starts
            .partition_point(|row| *row < u32::from(top) + u32::from(area.height));
        if (begin..end.min(self.starts.len().saturating_sub(1)))
            .all(|line| self.blocks.contains_key(&(line / BLOCK_LINES)))
        {
            for index in begin..end.min(self.starts.len().saturating_sub(1)) {
                let block = &self.blocks[&(index / BLOCK_LINES)];
                block
                    .last_used
                    .store(clock, std::sync::atomic::Ordering::Relaxed);
                draw_line(
                    frame.buffer_mut(),
                    area,
                    &block.lines[index % BLOCK_LINES],
                    self.starts[index],
                    self.starts[index + 1],
                    top,
                );
            }
        } else if self.fallback.0 == top && self.fallback.1.area.height == area.height {
            for y in 0..area.height {
                for x in 0..area.width {
                    frame.buffer_mut()[(area.x + x, area.y + y)] = self.fallback.1[(x, y)].clone();
                }
            }
        }
    }
}

fn draw_line(
    buffer: &mut ratatui::buffer::Buffer,
    area: Rect,
    line: &Line<'static>,
    start: u32,
    end: u32,
    top: u16,
) {
    let viewport_end = u32::from(top) + u32::from(area.height);
    if end <= u32::from(top) || start >= viewport_end {
        return;
    }
    let y = start.saturating_sub(u32::from(top)) as u16;
    let skip = u32::from(top).saturating_sub(start).min(u16::MAX as u32) as u16;
    let height = end
        .min(viewport_end)
        .saturating_sub(start.max(u32::from(top))) as u16;
    ratatui::widgets::Widget::render(
        Paragraph::new(line.clone())
            .wrap(Wrap { trim: false })
            .scroll((skip, 0)),
        Rect::new(area.x, area.y + y, area.width, height),
        buffer,
    );
}

fn render_indexed(
    lines: &[Line<'static>],
    starts: &[u32],
    top: u16,
    buffer: &mut ratatui::buffer::Buffer,
    area: Rect,
) {
    let begin = starts
        .partition_point(|row| *row <= u32::from(top))
        .saturating_sub(1);
    let end = starts.partition_point(|row| *row < u32::from(top) + u32::from(area.height));
    for index in begin..end.min(lines.len()) {
        draw_line(
            buffer,
            area,
            &lines[index],
            starts[index],
            starts[index + 1],
            top,
        );
    }
}

#[derive(Debug, Default)]
struct Inner {
    key: Option<Key>,
    layout: Option<Layout>,
    worker: Option<mpsc::Receiver<(Key, Layout)>>,
    clock: u64,
    builds: u64,
}

#[derive(Debug, Default)]
pub(crate) struct TranscriptCache {
    revision: Cell<u64>,
    inner: RefCell<Inner>,
    memo: Arc<Mutex<MarkdownMemo>>,
    #[cfg(test)]
    async_build: Cell<bool>,
}

impl TranscriptCache {
    pub(crate) fn invalidate(&self) {
        self.revision.set(self.revision.get().wrapping_add(1));
    }
    pub(crate) fn pending(&self) -> bool {
        self.inner.borrow().worker.is_some()
    }

    pub(crate) fn debug_stats(&self) -> (u64, usize, Option<usize>, bool) {
        let inner = self.inner.borrow();
        let layout = inner.layout.as_ref();
        let bytes = self
            .memo
            .try_lock()
            .ok()
            .map(|memo| memo.bytes + layout.map_or(0, |layout| layout.bytes));
        (
            inner.builds,
            layout.map_or(0, |layout| layout.blocks.len()),
            bytes,
            inner.worker.is_some(),
        )
    }

    pub(super) fn render(
        &self,
        frame: &mut ratatui::Frame,
        area: Rect,
        state: &AppState,
        focused: bool,
    ) -> u16 {
        if area.is_empty() {
            return 0;
        }
        let key = Key::for_state(state, area.width, focused);
        let mut inner = self.inner.borrow_mut();
        if let Some(result) = inner.worker.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(Some(result)),
            Err(mpsc::TryRecvError::Disconnected) => Some(None),
            Err(mpsc::TryRecvError::Empty) => None,
        }) {
            inner.worker = None;
            if let Some((built_key, layout)) =
                result.filter(|(k, _)| k.compatible(&key) && k.generation == key.generation)
            {
                inner.key = Some(built_key);
                inner.layout = Some(layout);
                inner.builds += 1;
            }
        }
        if inner.key.as_ref().is_some_and(|old| !old.compatible(&key)) {
            inner.layout = None;
        }
        let offset = state.transcript_scroll.bottom_offset();
        let mut controls = Vec::new();
        append_conversation_controls(&mut controls, state, area.width, true);
        let control_rows = controls
            .iter()
            .map(|line| {
                Paragraph::new(line.clone())
                    .wrap(Wrap { trim: false })
                    .line_count(area.width)
            })
            .sum::<usize>()
            .min(u16::MAX as usize) as u16;
        let missing_view = inner.layout.as_ref().is_some_and(|layout| {
            let top = layout
                .rows()
                .saturating_add(control_rows)
                .saturating_sub(area.height)
                .saturating_sub(offset);
            !layout.has_view(top, area.height)
        });
        if (inner.key.as_ref() != Some(&key) || missing_view) && inner.worker.is_none() {
            let build = snapshot(state, area.width, focused);
            let memo = self.memo.clone();
            let previous = inner
                .layout
                .as_ref()
                .map_or_else(BTreeMap::new, |l| l.blocks.clone());
            #[cfg(test)]
            let inline = !self.async_build.get();
            #[cfg(not(test))]
            let inline = false;
            if inline || tokio::runtime::Handle::try_current().is_err() {
                inner.layout = Some(with_memo(memo, || {
                    Layout::build(
                        build(),
                        area.width,
                        offset,
                        area.height,
                        control_rows,
                        previous,
                    )
                }));
                inner.key = Some(key.clone());
                inner.builds += 1;
            } else {
                let (tx, rx) = mpsc::channel();
                let width = area.width;
                let height = area.height;
                tokio::task::spawn_blocking(move || {
                    let _ = tx.send((
                        key,
                        with_memo(memo, || {
                            Layout::build(build(), width, offset, height, control_rows, previous)
                        }),
                    ));
                });
                inner.worker = Some(rx);
            }
        }
        inner.clock = inner.clock.saturating_add(1);
        let base_rows = inner.layout.as_ref().map_or(0, Layout::rows);
        let maximum = base_rows
            .saturating_add(control_rows)
            .saturating_sub(area.height);
        let scroll =
            maximum.saturating_sub(state.transcript_scroll.observe_rendered_maximum(maximum));
        if let Some(layout) = &inner.layout {
            layout.draw(frame, area, scroll, inner.clock);
            let targets = layout
                .headers
                .iter()
                .filter(|header| {
                    let row = layout.starts[header.line_index];
                    row >= u32::from(scroll) && row < u32::from(scroll) + u32::from(area.height)
                })
                .map(|header| crate::tui::transcript::ActivityHitTarget {
                    key: header.key.clone(),
                    row: layout.starts[header.line_index],
                    left: area.x.saturating_add(header.left).min(area.right()),
                    right: area
                        .x
                        .saturating_add(header.left)
                        .saturating_add(header.width)
                        .min(area.right()),
                })
                .collect();
            state.transcript_scroll.set_activity_hit_targets(targets);
            if state.terminal_hyperlinks_supported {
                apply_hyperlinks(frame.buffer_mut(), area, &layout.links, &state.workspace);
            }
        } else {
            state.transcript_scroll.clear_activity_hit_targets();
        }
        let mut row = u32::from(base_rows);
        for line in &controls {
            let next = row.saturating_add(
                Paragraph::new(line.clone())
                    .wrap(Wrap { trim: false })
                    .line_count(area.width) as u32,
            );
            draw_line(frame.buffer_mut(), area, line, row, next, scroll);
            row = next;
        }
        // Cursor blink changes only the visible cells, never markdown/layout.
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let cell = &mut frame.buffer_mut()[(x, y)];
                if cell.symbol() == "▌" && cell.fg == Color::DarkGray {
                    if state.cursor_blink_visible {
                        cell.modifier.remove(Modifier::HIDDEN);
                    } else {
                        cell.modifier.insert(Modifier::HIDDEN);
                    }
                }
            }
        }
        scroll
    }
}

fn snapshot(
    state: &AppState,
    width: u16,
    focused: bool,
) -> impl FnOnce() -> ConversationContent + Send + 'static {
    let theme = active_theme(state);
    let auto = state.theme_manager.is_none() && state.terminal_theme_auto;
    macro_rules! capture {
        ($($field:ident),* $(,)?) => {{
            $(let $field = state.$field.clone();)*
            move || {
                let mut copy = AppState::new(crate::context::ShellHistory::in_memory());
                $(copy.$field = $field;)*
                copy.ui_theme = theme; copy.terminal_theme_auto = auto;
                copy.cursor_blink_visible = true;
                conversation_content(&copy, width, true, focused)
            }
        }};
    }
    capture!(
        messages,
        streaming,
        transcript_annotations,
        transcript_agent_summaries,
        live_agent_activity,
        agent_activity_focus,
        active_agent_turn_id,
        context_checkpoint,
        fallback_provenance,
        session_history,
        show_thinking,
        show_line_numbers,
        show_transcript_timestamps,
        show_assistant_footers,
        expanded_blocks,
        workspace,
        command_presentations,
        file_activity_presentations,
        terminal_appearance,
        keymap,
        cwd
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MemoKey {
    text: [u8; 32],
    width: u16,
    numbers: bool,
    theme: ThemePalette,
}

#[derive(Debug, Default)]
struct MarkdownMemo {
    documents: HashMap<MemoKey, (crate::markdown::MarkdownDocument, usize, u64)>,
    bytes: usize,
    clock: u64,
}

thread_local! { static ACTIVE_MEMO: RefCell<Option<Arc<Mutex<MarkdownMemo>>>> = const { RefCell::new(None) }; }

fn with_memo<T>(memo: Arc<Mutex<MarkdownMemo>>, operation: impl FnOnce() -> T) -> T {
    struct Restore(Option<Arc<Mutex<MarkdownMemo>>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE_MEMO.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(ACTIVE_MEMO.with(|slot| slot.replace(Some(memo))));
    operation()
}

fn memo_key(input: &str, width: u16, numbers: bool, theme: ThemePalette) -> MemoKey {
    use sha2::{Digest, Sha256};
    MemoKey {
        text: Sha256::digest(input.as_bytes()).into(),
        width,
        numbers,
        theme,
    }
}

pub(super) fn memo_document(
    input: &str,
    width: u16,
    numbers: bool,
    theme: ThemePalette,
) -> Option<crate::markdown::MarkdownDocument> {
    let memo = ACTIVE_MEMO.with(|slot| slot.borrow().clone())?;
    let key = memo_key(input, width, numbers, theme);
    let mut memo = memo.lock().unwrap_or_else(|p| p.into_inner());
    memo.clock = memo.clock.saturating_add(1);
    let clock = memo.clock;
    memo.documents.get_mut(&key).map(|(doc, _, age)| {
        *age = clock;
        doc.clone()
    })
}

pub(super) fn memo_insert(
    input: &str,
    width: u16,
    numbers: bool,
    theme: ThemePalette,
    doc: &crate::markdown::MarkdownDocument,
) {
    let Some(memo) = ACTIVE_MEMO.with(|slot| slot.borrow().clone()) else {
        return;
    };
    let bytes = doc.lines.iter().map(line_bytes).sum::<usize>()
        + doc
            .links
            .iter()
            .map(|l| l.label.len() + l.destination.len())
            .sum::<usize>();
    if bytes > PAYLOAD_BUDGET {
        return;
    }
    let key = memo_key(input, width, numbers, theme);
    let mut memo = memo.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((_, previous, _)) = memo.documents.remove(&key) {
        memo.bytes -= previous;
    }
    while memo.bytes + bytes > PAYLOAD_BUDGET {
        let Some(old) = memo
            .documents
            .iter()
            .min_by_key(|(_, (_, _, age))| *age)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        memo.bytes -= memo.documents.remove(&old).unwrap().1;
    }
    let clock = memo.clock;
    memo.documents.insert(key, (doc.clone(), bytes, clock));
    memo.bytes += bytes;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_rows_match_whole_paragraph_with_unicode_and_empty_lines() {
        for width in [1, 7, 40, 100] {
            let lines = vec![
                Line::from("你好 👩🏽‍💻 and a long word wraps cleanly"),
                Line::from(""),
                Line::from("    indented text\tand more"),
                Line::from(vec![
                    Span::styled("bold ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw("ordinary"),
                ]),
            ];
            let rows = Paragraph::new(Text::from(lines.clone()))
                .wrap(Wrap { trim: false })
                .line_count(width);
            let layout = Layout::build(
                ConversationContent {
                    lines: lines.clone(),
                    activity_headers: Vec::new(),
                    links: Vec::new(),
                },
                width,
                0,
                5,
                0,
                BTreeMap::new(),
            );
            assert_eq!(usize::from(layout.rows()), rows);
            for top in 0..rows.saturating_sub(1) {
                let area = Rect::new(0, 0, width, 5);
                let mut expected = ratatui::buffer::Buffer::empty(area);
                let mut actual = expected.clone();
                ratatui::widgets::Widget::render(
                    Paragraph::new(Text::from(lines.clone()))
                        .wrap(Wrap { trim: false })
                        .scroll((top as u16, 0)),
                    area,
                    &mut expected,
                );
                render_indexed(&lines, &layout.starts, top as u16, &mut actual, area);
                assert_eq!(actual, expected, "width={width}, top={top}");
            }
        }
    }

    #[test]
    fn composer_and_blink_reuse_layout_and_expansion_invalidates_it() {
        let mut state = crate::tui::test_support::test_state();
        state.messages.push(crate::app::Message {
            role: crate::app::Role::Assistant,
            content: "history ".repeat(1000),
            thinking: None,
            tool_calls: None,
            response_items: Vec::new(),
            timestamp: chrono::Utc::now(),
        });
        crate::tui::test_support::render_to_text(&state, 100, 30);
        let builds = state.transcript_cache.inner.borrow().builds;
        state.input = "typed".into();
        state.cursor_blink_visible = false;
        crate::tui::test_support::render_to_text(&state, 100, 30);
        assert_eq!(state.transcript_cache.inner.borrow().builds, builds);
        state.show_thinking = !state.show_thinking;
        crate::tui::test_support::render_to_text(&state, 100, 30);
        assert_eq!(state.transcript_cache.inner.borrow().builds, builds + 1);
    }

    #[test]
    fn bug257_four_thousand_messages_have_bounded_warm_composer_latency() {
        let mut state = crate::tui::test_support::test_state();
        state.messages = (0..4096)
            .map(|index| crate::app::Message {
                role: crate::app::Role::Assistant,
                content: format!("Row {index}: **Unicode** 你好 `code` and normal text."),
                thinking: None,
                tool_calls: None,
                response_items: Vec::new(),
                timestamp: chrono::Utc::now(),
            })
            .collect();
        crate::tui::test_support::render_to_text(&state, 100, 30);
        let builds = state.transcript_cache.inner.borrow().builds;
        let mut samples = Vec::new();
        for index in 0..64 {
            state.input.push(char::from(b'a' + index % 26));
            state.input_cursor = state.input.len();
            let start = std::time::Instant::now();
            crate::tui::test_support::render_to_text(&state, 100, 30);
            samples.push(start.elapsed());
        }
        samples.sort();
        let p95 = samples[samples.len() * 95 / 100];
        eprintln!("BUG-257 warm 4096-message render p95: {p95:?}");
        assert!(
            p95 < std::time::Duration::from_millis(100),
            "warm p95: {p95:?}"
        );
        assert_eq!(state.transcript_cache.inner.borrow().builds, builds);
        assert!(
            state
                .transcript_cache
                .inner
                .borrow()
                .layout
                .as_ref()
                .unwrap()
                .bytes
                <= PAYLOAD_BUDGET
        );
        assert!(state.transcript_cache.memo.lock().unwrap().bytes <= PAYLOAD_BUDGET);
    }

    #[test]
    fn bug257_blocks_reuse_rows_and_evict_without_discarding_canonical_layout() {
        let lines: Vec<_> = (0..4096)
            .map(|index| Line::from(format!("row {index} 你好")))
            .collect();
        let build = |lines| ConversationContent {
            lines,
            activity_headers: Vec::new(),
            links: Vec::new(),
        };
        let layout = Layout::build(build(lines.clone()), 80, 0, 24, 0, BTreeMap::new());
        let first = layout.blocks[&0].clone();
        let mut changed = lines;
        changed.push(Line::from("new tail"));
        let next = Layout::build(build(changed), 80, 0, 24, 0, layout.blocks);
        assert!(Arc::ptr_eq(&first, &next.blocks[&0]));
        assert_eq!(next.starts.len(), 4098);
        let oversized = Layout::build(
            build(vec![Line::from("x".repeat(PAYLOAD_BUDGET + 1024))]),
            80,
            0,
            24,
            3,
            BTreeMap::new(),
        );
        assert!(oversized.bytes <= PAYLOAD_BUDGET);
        assert!(oversized.blocks.is_empty());
        let top = oversized.rows().saturating_add(3).saturating_sub(24);
        assert!(oversized.has_view(top, 24));
        assert_eq!(
            oversized.starts.len(),
            2,
            "row index survives styled-payload eviction"
        );
    }

    #[tokio::test]
    async fn bug257_workers_keep_history_visible_and_discard_other_session_results() {
        let mut state = crate::tui::test_support::test_state();
        let message = |text: &str| crate::app::Message {
            role: crate::app::Role::Assistant,
            content: text.into(),
            thinking: None,
            tool_calls: None,
            response_items: Vec::new(),
            timestamp: chrono::Utc::now(),
        };
        state.messages.push(message("previous visible history"));
        crate::tui::test_support::render_to_text(&state, 100, 30);
        state.transcript_cache.async_build.set(true);
        state.stream_generation += 1;
        state.messages.push(message("new tail"));
        state.transcript_cache.invalidate();
        let while_building = crate::tui::test_support::render_to_text(&state, 100, 30);
        assert!(state.transcript_cache.pending());
        assert!(while_building.contains("previous visible history"));
        state.current_session = Some(crate::session::SessionV2::new(
            "different",
            ".",
            "test",
            "model",
            Vec::new(),
        ));
        state.messages = vec![message("different session visible")];
        state.stream_generation += 1;
        let switched = crate::tui::test_support::render_to_text(&state, 100, 30);
        assert!(!switched.contains("previous visible history"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            let rendered = crate::tui::test_support::render_to_text(&state, 100, 30);
            if !state.transcript_cache.pending() {
                assert!(rendered.contains("different session visible"));
                assert!(!rendered.contains("previous visible history"));
                break;
            }
            assert!(std::time::Instant::now() < deadline);
        }
    }
}
