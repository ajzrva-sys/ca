use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, Event as CEvent, KeyCode, KeyEventKind, KeyModifiers,
    KeyboardEnhancementFlags, MouseButton, MouseEvent, MouseEventKind, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, stdout, IsTerminal, Stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set terminal cursor color via OSC 12 to match the theme accent.
pub fn set_cursor_color(r: u8, g: u8, b: u8) {
    if stdout().is_terminal() {
        let _ = write!(
            io::stdout(),
            "\x1b]12;rgb:{:02x}/{:02x}/{:02x}\x1b\\",
            r,
            g,
            b
        );
        let _ = io::stdout().flush();
    }
}

/// Reset terminal cursor color to default via OSC 112.
pub fn reset_cursor_color() {
    if stdout().is_terminal() {
        let _ = write!(io::stdout(), "\x1b]112\x1b\\");
        let _ = io::stdout().flush();
    }
}
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

trait LifecycleControl {
    fn enable_raw(&mut self) -> io::Result<()>;
    fn enter_alternate_screen(&mut self) -> io::Result<()>;
    fn enable_mouse_capture(&mut self) -> io::Result<()>;
    fn disable_mouse_capture(&mut self) -> io::Result<()>;
    fn enable_keyboard_enhancement(&mut self) -> io::Result<()>;
    fn disable_keyboard_enhancement(&mut self) -> io::Result<()>;
    fn enable_bracketed_paste(&mut self) -> io::Result<()>;
    fn disable_bracketed_paste(&mut self) -> io::Result<()>;
    fn leave_alternate_screen(&mut self) -> io::Result<()>;
    fn disable_raw(&mut self) -> io::Result<()>;
    fn enable_focus_change(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn disable_focus_change(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn clear_title(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn restoration_step(result: io::Result<()>) -> io::Result<()> {
    if result.is_err() {
        crate::runtime_diagnostics::process().terminal_restoration_failed();
    }
    result
}

struct CrosstermControl;

impl LifecycleControl for CrosstermControl {
    fn enable_raw(&mut self) -> io::Result<()> {
        crossterm::terminal::enable_raw_mode()
    }

    fn enter_alternate_screen(&mut self) -> io::Result<()> {
        crossterm::execute!(stdout(), crossterm::terminal::EnterAlternateScreen)
    }

    fn enable_bracketed_paste(&mut self) -> io::Result<()> {
        crossterm::execute!(stdout(), EnableBracketedPaste)
    }

    fn enable_mouse_capture(&mut self) -> io::Result<()> {
        crossterm::execute!(stdout(), EnableMouseCapture)
    }

    fn disable_mouse_capture(&mut self) -> io::Result<()> {
        crossterm::execute!(stdout(), DisableMouseCapture)
    }

    fn enable_keyboard_enhancement(&mut self) -> io::Result<()> {
        crossterm::execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
    }

    fn disable_keyboard_enhancement(&mut self) -> io::Result<()> {
        crossterm::execute!(stdout(), PopKeyboardEnhancementFlags)
    }

    fn disable_bracketed_paste(&mut self) -> io::Result<()> {
        crossterm::execute!(stdout(), DisableBracketedPaste)
    }

    fn leave_alternate_screen(&mut self) -> io::Result<()> {
        crossterm::execute!(stdout(), crossterm::terminal::LeaveAlternateScreen)
    }

    fn disable_raw(&mut self) -> io::Result<()> {
        crossterm::terminal::disable_raw_mode()
    }

    fn enable_focus_change(&mut self) -> io::Result<()> {
        crossterm::execute!(stdout(), EnableFocusChange)
    }

    fn disable_focus_change(&mut self) -> io::Result<()> {
        crossterm::execute!(stdout(), DisableFocusChange)
    }

    fn clear_title(&mut self) -> io::Result<()> {
        let mut output = stdout().lock();
        output.write_all(crate::terminal_awareness::clear_title_sequence().as_bytes())?;
        output.flush()
    }
}

struct TerminalLifecycle<C: LifecycleControl> {
    control: C,
    raw_enabled: bool,
    alternate_screen_entered: bool,
    mouse_mode: crate::ui_settings::MouseMode,
    mouse_capture_enabled: bool,
    mouse_warning: Option<String>,
    keyboard_enhancement_enabled: bool,
    bracketed_paste_enabled: bool,
    focus_change_enabled: bool,
}

impl<C: LifecycleControl> TerminalLifecycle<C> {
    fn enter(mut control: C, mouse_mode: crate::ui_settings::MouseMode) -> io::Result<Self> {
        control.enable_raw()?;
        let mut lifecycle = Self {
            control,
            raw_enabled: true,
            alternate_screen_entered: false,
            mouse_mode,
            mouse_capture_enabled: false,
            mouse_warning: None,
            keyboard_enhancement_enabled: false,
            bracketed_paste_enabled: false,
            focus_change_enabled: false,
        };
        lifecycle.control.enter_alternate_screen()?;
        lifecycle.alternate_screen_entered = true;
        lifecycle.acquire_focus_change_best_effort();
        lifecycle.acquire_keyboard_enhancement_best_effort();
        lifecycle.acquire_mouse_best_effort();
        if let Err(error) = lifecycle.control.enable_bracketed_paste() {
            let _ = lifecycle.control.disable_bracketed_paste();
            if lifecycle.mouse_capture_enabled {
                let _ = lifecycle.control.disable_mouse_capture();
                lifecycle.mouse_capture_enabled = false;
            }
            if lifecycle.keyboard_enhancement_enabled {
                let _ = lifecycle.control.disable_keyboard_enhancement();
                lifecycle.keyboard_enhancement_enabled = false;
            }
            if lifecycle.focus_change_enabled {
                let _ = lifecycle.control.disable_focus_change();
                lifecycle.focus_change_enabled = false;
            }
            return Err(error);
        }
        lifecycle.bracketed_paste_enabled = true;
        Ok(lifecycle)
    }

    fn suspend(&mut self) -> io::Result<()> {
        if self.bracketed_paste_enabled {
            restoration_step(self.control.disable_bracketed_paste())?;
            self.bracketed_paste_enabled = false;
        }
        if self.mouse_capture_enabled {
            restoration_step(self.control.disable_mouse_capture())?;
            self.mouse_capture_enabled = false;
        }
        if self.keyboard_enhancement_enabled {
            restoration_step(self.control.disable_keyboard_enhancement())?;
            self.keyboard_enhancement_enabled = false;
        }
        if self.focus_change_enabled {
            restoration_step(self.control.disable_focus_change())?;
            self.focus_change_enabled = false;
        }
        if self.alternate_screen_entered {
            let _ = restoration_step(self.control.clear_title());
            restoration_step(self.control.leave_alternate_screen())?;
            self.alternate_screen_entered = false;
        }
        if self.raw_enabled {
            restoration_step(self.control.disable_raw())?;
            self.raw_enabled = false;
        }
        Ok(())
    }

    fn resume(&mut self) -> io::Result<()> {
        if !self.raw_enabled {
            self.control.enable_raw()?;
            self.raw_enabled = true;
        }
        if !self.alternate_screen_entered {
            if let Err(error) = self.control.enter_alternate_screen() {
                let _ = self.control.disable_raw();
                self.raw_enabled = false;
                return Err(error);
            }
            self.alternate_screen_entered = true;
        }
        self.acquire_focus_change_best_effort();
        self.acquire_keyboard_enhancement_best_effort();
        self.acquire_mouse_best_effort();
        if !self.bracketed_paste_enabled {
            if let Err(error) = self.control.enable_bracketed_paste() {
                let _ = self.control.disable_bracketed_paste();
                if self.mouse_capture_enabled {
                    let _ = self.control.disable_mouse_capture();
                    self.mouse_capture_enabled = false;
                }
                if self.keyboard_enhancement_enabled {
                    let _ = self.control.disable_keyboard_enhancement();
                    self.keyboard_enhancement_enabled = false;
                }
                if self.focus_change_enabled {
                    let _ = self.control.disable_focus_change();
                    self.focus_change_enabled = false;
                }
                if self.alternate_screen_entered {
                    let _ = self.control.leave_alternate_screen();
                    self.alternate_screen_entered = false;
                }
                if self.raw_enabled {
                    let _ = self.control.disable_raw();
                    self.raw_enabled = false;
                }
                return Err(error);
            }
            self.bracketed_paste_enabled = true;
        }
        Ok(())
    }

    fn set_mouse_mode(&mut self, mode: crate::ui_settings::MouseMode) -> io::Result<bool> {
        self.mouse_mode = mode;
        self.mouse_warning = None;
        match mode {
            crate::ui_settings::MouseMode::Off => {
                if self.mouse_capture_enabled {
                    self.control.disable_mouse_capture()?;
                    self.mouse_capture_enabled = false;
                }
                Ok(true)
            }
            crate::ui_settings::MouseMode::Scroll => {
                self.acquire_mouse_best_effort();
                Ok(self.mouse_capture_enabled)
            }
        }
    }

    fn acquire_mouse_best_effort(&mut self) {
        if self.mouse_mode == crate::ui_settings::MouseMode::Off
            || self.mouse_capture_enabled
            || !self.alternate_screen_entered
        {
            return;
        }
        match self.control.enable_mouse_capture() {
            Ok(()) => self.mouse_capture_enabled = true,
            Err(error) => {
                self.mouse_warning = Some(format!(
                    "Mouse capture is unsupported ({error}); keyboard scrolling remains available"
                ));
            }
        }
    }

    fn acquire_keyboard_enhancement_best_effort(&mut self) {
        if self.keyboard_enhancement_enabled || !self.alternate_screen_entered {
            return;
        }
        if self.control.enable_keyboard_enhancement().is_ok() {
            self.keyboard_enhancement_enabled = true;
        }
    }

    fn acquire_focus_change_best_effort(&mut self) {
        if self.focus_change_enabled || !self.alternate_screen_entered {
            return;
        }
        if self.control.enable_focus_change().is_ok() {
            self.focus_change_enabled = true;
        }
    }
}

impl<C: LifecycleControl> Drop for TerminalLifecycle<C> {
    fn drop(&mut self) {
        if self.bracketed_paste_enabled {
            let _ = restoration_step(self.control.disable_bracketed_paste());
            self.bracketed_paste_enabled = false;
        }
        if self.mouse_capture_enabled {
            let _ = restoration_step(self.control.disable_mouse_capture());
            self.mouse_capture_enabled = false;
        }
        if self.keyboard_enhancement_enabled {
            let _ = restoration_step(self.control.disable_keyboard_enhancement());
            self.keyboard_enhancement_enabled = false;
        }
        if self.focus_change_enabled {
            let _ = restoration_step(self.control.disable_focus_change());
            self.focus_change_enabled = false;
        }
        if self.alternate_screen_entered {
            reset_cursor_color();
            let _ = restoration_step(self.control.clear_title());
            let _ = restoration_step(self.control.leave_alternate_screen());
            self.alternate_screen_entered = false;
        }
        if self.raw_enabled {
            let _ = restoration_step(self.control.disable_raw());
            self.raw_enabled = false;
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Key(KeyCode, KeyModifiers),
    #[allow(dead_code)]
    Resize(u16, u16),
    Paste(String),
    MouseScroll(MouseScrollEvent),
    MouseSelection(MouseSelectionEvent),
    Focus(bool),
    ThemeDetected(Option<crate::terminal_awareness::TerminalAppearance>),
    Tick,
    Redraw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseScrollEvent {
    pub direction: ScrollDirection,
    pub column: u16,
    pub row: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseSelectionPhase {
    Start,
    Drag,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseSelectionEvent {
    pub phase: MouseSelectionPhase,
    pub column: u16,
    pub row: u16,
}

fn translate_mouse_event(mouse: MouseEvent) -> Option<Event> {
    match mouse.kind {
        MouseEventKind::ScrollUp => Some(Event::MouseScroll(MouseScrollEvent {
            direction: ScrollDirection::Up,
            column: mouse.column,
            row: mouse.row,
        })),
        MouseEventKind::ScrollDown => Some(Event::MouseScroll(MouseScrollEvent {
            direction: ScrollDirection::Down,
            column: mouse.column,
            row: mouse.row,
        })),
        MouseEventKind::Down(MouseButton::Left) => {
            Some(Event::MouseSelection(MouseSelectionEvent {
                phase: MouseSelectionPhase::Start,
                column: mouse.column,
                row: mouse.row,
            }))
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            Some(Event::MouseSelection(MouseSelectionEvent {
                phase: MouseSelectionPhase::Drag,
                column: mouse.column,
                row: mouse.row,
            }))
        }
        MouseEventKind::Up(MouseButton::Left) => Some(Event::MouseSelection(MouseSelectionEvent {
            phase: MouseSelectionPhase::End,
            column: mouse.column,
            row: mouse.row,
        })),
        _ => None,
    }
}

/// Minimum interval between terminal draws (~60fps).
const MIN_FRAME_DURATION: Duration = Duration::from_millis(16);

fn frame_due(last: std::time::Instant, now: std::time::Instant) -> bool {
    now.saturating_duration_since(last) >= MIN_FRAME_DURATION
}

struct QueuedEvent {
    event: Event,
    queued_at: std::time::Instant,
}

#[derive(Clone)]
struct InputSender(mpsc::UnboundedSender<QueuedEvent>);

impl InputSender {
    fn is_closed(&self) -> bool {
        self.0.is_closed()
    }
    fn send(&self, event: Event) -> Result<(), mpsc::error::SendError<Event>> {
        self.0
            .send(QueuedEvent {
                event,
                queued_at: std::time::Instant::now(),
            })
            .map_err(|error| mpsc::error::SendError(error.0.event))
    }
}

fn dequeue(event: Option<QueuedEvent>, remaining: usize) -> io::Result<Event> {
    let event =
        event.ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "event channel closed"))?;
    crate::runtime_diagnostics::process().input_dequeued(event.queued_at.elapsed(), remaining);
    Ok(event.event)
}

async fn scheduled_event(
    rx: &mut mpsc::UnboundedReceiver<QueuedEvent>,
    ticks: &mut tokio::time::Interval,
    redraw_at: Option<tokio::time::Instant>,
    input_batch: usize,
) -> io::Result<Event> {
    let deadline = redraw_at.unwrap_or_else(tokio::time::Instant::now);
    if input_batch >= 32 {
        tokio::select! {
            biased;
            _ = ticks.tick() => Ok(Event::Tick),
            _ = tokio::time::sleep_until(deadline), if redraw_at.is_some() => Ok(Event::Redraw),
            event = rx.recv() => dequeue(event, rx.len()),
        }
    } else {
        tokio::select! {
            biased;
            event = rx.recv() => dequeue(event, rx.len()),
            _ = tokio::time::sleep_until(deadline), if redraw_at.is_some() => Ok(Event::Redraw),
            _ = ticks.tick() => Ok(Event::Tick),
        }
    }
}

fn publish_filter_banner(state: &crate::app::AppState) {
    state
        .filter_banner
        .store(crate::provider_filter::banner_active(), Ordering::Relaxed);
    if let Ok(mut summary) = state.filter_summary.lock() {
        *summary = crate::provider_filter::banner_detail();
    }
    if let Some((generation, text)) = crate::provider_filter::outbound_view() {
        state.provider_pane.note(generation, text);
    }
}

pub struct Tui {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    event_rx: mpsc::UnboundedReceiver<QueuedEvent>,
    event_tx: InputSender,
    _event_handle: tokio::task::JoinHandle<()>,
    lifecycle: TerminalLifecycle<CrosstermControl>,
    input_suspended: Arc<AtomicBool>,
    ticks: tokio::time::Interval,
    input_batch: usize,
    last_theme_probe: Option<std::time::Instant>,
    last_title: Option<String>,
    last_draw: std::time::Instant,
    pending_draw: bool,
}

impl Tui {
    pub fn new(mouse_mode: crate::ui_settings::MouseMode) -> io::Result<Self> {
        let lifecycle = TerminalLifecycle::enter(CrosstermControl, mouse_mode)?;
        let backend = CrosstermBackend::new(stdout());
        let terminal = Terminal::new(backend)?;

        // Spawn a dedicated blocking thread for crossterm event polling
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let event_tx = InputSender(event_tx);
        let input_suspended = Arc::new(AtomicBool::new(false));
        let poll_suspended = input_suspended.clone();
        let poll_event_tx = event_tx.clone();
        let _event_handle = tokio::task::spawn_blocking(move || {
            let mut coalesced = None;
            let mut coalesced_since = std::time::Instant::now();
            loop {
                if poll_event_tx.is_closed() {
                    break;
                }
                if poll_suspended.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                if coalesced.is_some()
                    && coalesced_since.elapsed() >= MIN_FRAME_DURATION
                    && poll_event_tx.send(coalesced.take().unwrap()).is_err()
                {
                    break;
                }
                if event::poll(Duration::from_millis(if coalesced.is_some() {
                    1
                } else {
                    50
                }))
                .unwrap_or(false)
                {
                    // Publish pending pointer/geometry updates before ordered input.
                    let read = event::read();
                    if let Ok(CEvent::Resize(w, h)) = &read {
                        if !matches!(coalesced, None | Some(Event::Resize(..))) {
                            if let Some(pending) = coalesced.take() {
                                let _ = poll_event_tx.send(pending);
                            }
                        }
                        if coalesced.is_none() {
                            coalesced_since = std::time::Instant::now();
                        }
                        coalesced = Some(Event::Resize(*w, *h));
                        continue;
                    }
                    if let Ok(CEvent::Mouse(mouse)) = &read {
                        if matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left)) {
                            if !matches!(coalesced, None | Some(Event::MouseSelection(..))) {
                                if let Some(pending) = coalesced.take() {
                                    let _ = poll_event_tx.send(pending);
                                }
                            }
                            if coalesced.is_none() {
                                coalesced_since = std::time::Instant::now();
                            }
                            coalesced = translate_mouse_event(*mouse);
                            continue;
                        }
                    }
                    if let Some(pending) = coalesced.take() {
                        if poll_event_tx.send(pending).is_err() {
                            break;
                        }
                    }
                    match read {
                        Ok(CEvent::Key(key)) if key.kind != KeyEventKind::Release => {
                            if poll_event_tx
                                .send(Event::Key(key.code, key.modifiers))
                                .is_err()
                            {
                                break;
                            }
                        }
                        Ok(CEvent::Resize(w, h)) => {
                            if poll_event_tx.send(Event::Resize(w, h)).is_err() {
                                break;
                            }
                        }
                        Ok(CEvent::Paste(pasted)) => {
                            if poll_event_tx.send(Event::Paste(pasted)).is_err() {
                                break;
                            }
                        }
                        Ok(CEvent::Mouse(mouse)) => {
                            if let Some(mouse) = translate_mouse_event(mouse) {
                                if poll_event_tx.send(mouse).is_err() {
                                    break;
                                }
                            }
                        }
                        Ok(CEvent::FocusGained) => {
                            if poll_event_tx.send(Event::Focus(true)).is_err() {
                                break;
                            }
                        }
                        Ok(CEvent::FocusLost)
                            if poll_event_tx.send(Event::Focus(false)).is_err() =>
                        {
                            break;
                        }
                        _ => {}
                    }
                } else {
                    if let Some(pending) = coalesced.take() {
                        if poll_event_tx.send(pending).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let mut ticks = tokio::time::interval(Duration::from_millis(50));
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Ok(Self {
            terminal,
            event_rx,
            event_tx,
            _event_handle,
            lifecycle,
            input_suspended,
            ticks,
            input_batch: 0,
            last_theme_probe: None,
            last_title: None,
            // Set to past so the first draw always goes through immediately.
            last_draw: std::time::Instant::now() - MIN_FRAME_DURATION,
            pending_draw: false,
        })
    }

    pub async fn suspend_for<R, F>(&mut self, operation: F) -> io::Result<R>
    where
        R: Send + 'static,
        F: FnOnce() -> R + Send + 'static,
    {
        self.input_suspended.store(true, Ordering::Release);
        tokio::time::sleep(Duration::from_millis(60)).await;
        if let Err(error) = self.lifecycle.suspend() {
            let _ = self.lifecycle.resume();
            self.input_suspended.store(false, Ordering::Release);
            return Err(error);
        }
        let task = tokio::task::spawn_blocking(operation).await;
        let resume = self.lifecycle.resume();
        self.input_suspended.store(false, Ordering::Release);
        resume?;
        self.terminal.clear()?;
        task.map_err(|error| io::Error::other(format!("attached terminal task failed: {error}")))
    }

    pub fn draw(&mut self, state: &crate::app::AppState) -> io::Result<()> {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_draw);
        if elapsed < MIN_FRAME_DURATION {
            // Too soon — mark that a draw is pending and skip.
            self.pending_draw = true;
            return Ok(());
        }
        self.pending_draw = false;
        self.last_draw = now;
        publish_filter_banner(state);
        let title_state = state
            .pane_deck
            .as_ref()
            .and_then(|deck| deck.pane(state, deck.focused()))
            .unwrap_or(state);
        self.update_title(title_state)?;
        state.runtime_diagnostics.heartbeat();
        let started = std::time::Instant::now();
        match self.terminal.draw(|frame| crate::tui::render(frame, state)) {
            Ok(_) => {
                state
                    .runtime_diagnostics
                    .render_succeeded(started.elapsed(), 1);
                Ok(())
            }
            Err(error) => {
                state.runtime_diagnostics.render_failed();
                Err(error)
            }
        }
    }

    /// Drain any pending draw that was deferred by the frame-rate limiter.
    /// Call this at the end of each event loop iteration to ensure
    /// coalesced state changes eventually get rendered.
    pub fn flush_pending_draw(&mut self, state: &crate::app::AppState) -> io::Result<()> {
        if self.pending_draw && frame_due(self.last_draw, std::time::Instant::now()) {
            self.pending_draw = false;
            self.last_draw = std::time::Instant::now();
            publish_filter_banner(state);
            let title_state = state
                .pane_deck
                .as_ref()
                .and_then(|deck| deck.pane(state, deck.focused()))
                .unwrap_or(state);
            self.update_title(title_state)?;
            state.runtime_diagnostics.heartbeat();
            let started = std::time::Instant::now();
            match self.terminal.draw(|frame| crate::tui::render(frame, state)) {
                Ok(_) => {
                    state
                        .runtime_diagnostics
                        .render_succeeded(started.elapsed(), 1);
                    Ok(())
                }
                Err(error) => {
                    state.runtime_diagnostics.render_failed();
                    Err(error)
                }
            }
        } else {
            Ok(())
        }
    }

    pub fn request_theme_detection(&mut self) -> bool {
        let now = std::time::Instant::now();
        if self
            .last_theme_probe
            .is_some_and(|last| now.duration_since(last) < Duration::from_secs(2))
        {
            return false;
        }
        self.last_theme_probe = Some(now);
        self.event_tx
            .send(Event::ThemeDetected(
                crate::terminal_awareness::probe_terminal_appearance(),
            ))
            .is_ok()
    }

    fn update_title(&mut self, state: &crate::app::AppState) -> io::Result<()> {
        let next = state.status_indicators_enabled.then(|| {
            if state.interaction.is_some() {
                "ca · approval"
            } else if state.streaming.is_some() {
                "ca · working"
            } else if !matches!(state.soft_pause, crate::activity::SoftPauseState::Inactive) {
                "ca · paused"
            } else {
                "ca · ready"
            }
            .to_string()
        });
        if next == self.last_title {
            return Ok(());
        }
        let sequence = next
            .as_deref()
            .map(crate::terminal_awareness::title_sequence)
            .unwrap_or_else(|| crate::terminal_awareness::clear_title_sequence().into());
        let mut output = stdout().lock();
        if output.is_terminal() {
            output.write_all(sequence.as_bytes())?;
            output.flush()?;
        }
        self.last_title = next;
        Ok(())
    }

    pub async fn suspend_for_future<R, F>(&mut self, operation: F) -> io::Result<R>
    where
        F: std::future::Future<Output = R>,
    {
        self.input_suspended.store(true, Ordering::Release);
        tokio::time::sleep(Duration::from_millis(60)).await;
        if let Err(error) = self.lifecycle.suspend() {
            let _ = self.lifecycle.resume();
            self.input_suspended.store(false, Ordering::Release);
            return Err(error);
        }
        let result = operation.await;
        let resume = self.lifecycle.resume();
        self.input_suspended.store(false, Ordering::Release);
        resume?;
        self.terminal.clear()?;
        Ok(result)
    }

    /// Async event receive — never blocks the tokio runtime
    pub async fn next_event_async(&mut self) -> io::Result<Event> {
        let redraw_at = tokio::time::Instant::from_std(self.last_draw + MIN_FRAME_DURATION);
        let event = scheduled_event(
            &mut self.event_rx,
            &mut self.ticks,
            self.pending_draw.then_some(redraw_at),
            self.input_batch,
        )
        .await?;
        if matches!(event, Event::Tick | Event::Redraw) {
            self.input_batch = 0;
        } else {
            self.input_batch += 1;
        }
        Ok(event)
    }

    pub fn mouse_capture_warning(&self) -> Option<&str> {
        self.lifecycle.mouse_warning.as_deref()
    }

    pub fn set_mouse_mode(&mut self, mode: crate::ui_settings::MouseMode) -> io::Result<bool> {
        self.lifecycle.set_mouse_mode(mode)
    }

    pub fn emit_notification(
        &mut self,
        style: crate::ui_settings::NotificationStyle,
    ) -> io::Result<bool> {
        let passthrough = std::env::var_os("TMUX").is_some() || std::env::var_os("STY").is_some();
        let mut output = stdout().lock();
        let is_terminal = output.is_terminal();
        write_notification(&mut output, style, is_terminal, passthrough)
    }
}

fn write_notification<W: Write>(
    output: &mut W,
    style: crate::ui_settings::NotificationStyle,
    is_terminal: bool,
    passthrough: bool,
) -> io::Result<bool> {
    if !is_terminal {
        return Ok(false);
    }
    let Some(sequence) = crate::ui_settings::notification_sequence(style, passthrough) else {
        return Ok(false);
    };
    output.write_all(sequence.as_bytes())?;
    output.flush()?;
    Ok(true)
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn deferred_frames_obey_the_same_interval() {
        let start = std::time::Instant::now();
        for millis in 0..16 {
            assert!(!frame_due(start, start + Duration::from_millis(millis)));
        }
        assert!(frame_due(start, start + MIN_FRAME_DURATION));
    }

    #[tokio::test]
    async fn eight_hours_of_missed_ticks_do_not_queue_input() {
        let period = Duration::from_millis(50);
        let mut ticks = tokio::time::interval_at(
            tokio::time::Instant::now() - Duration::from_secs(8 * 3600),
            period,
        );
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticks.tick().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(Event::Key(KeyCode::Char('x'), KeyModifiers::NONE))
            .unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            Event::Key(KeyCode::Char('x'), _)
        ));
        assert!(rx.try_recv().is_err());
        assert!(tokio::time::timeout(Duration::from_millis(5), ticks.tick())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn bug257_input_precedes_missed_ticks_but_frames_get_a_bounded_slot() {
        let mut ticks = tokio::time::interval_at(
            tokio::time::Instant::now() - Duration::from_secs(8 * 3600),
            Duration::from_millis(50),
        );
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let tx = InputSender(tx);
        for index in 0..128 {
            tx.send(Event::Paste(index.to_string())).unwrap();
        }
        for index in 0..32 {
            assert!(
                matches!(scheduled_event(&mut rx, &mut ticks, None, index).await.unwrap(), Event::Paste(text) if text == index.to_string())
            );
        }
        assert!(matches!(
            scheduled_event(&mut rx, &mut ticks, None, 32)
                .await
                .unwrap(),
            Event::Tick
        ));
        assert_eq!(rx.len(), 96);
        let deadline = Some(tokio::time::Instant::now() - Duration::from_millis(16));
        assert!(matches!(
            scheduled_event(&mut rx, &mut ticks, deadline, 32)
                .await
                .unwrap(),
            Event::Redraw
        ));
        assert!(
            matches!(scheduled_event(&mut rx, &mut ticks, deadline, 0).await.unwrap(), Event::Paste(text) if text == "32")
        );
        for index in 33..128 {
            assert!(
                matches!(scheduled_event(&mut rx, &mut ticks, None, 0).await.unwrap(), Event::Paste(text) if text == index.to_string())
            );
        }
        assert!(rx.is_empty());
    }

    #[derive(Clone)]
    struct FakeControl {
        trace: Arc<Mutex<Vec<&'static str>>>,
        fail_alt: bool,
        fail_mouse: bool,
        fail_keyboard: bool,
        fail_paste_attempt: Option<usize>,
        fail_disable_paste: bool,
        paste_attempts: usize,
    }

    impl LifecycleControl for FakeControl {
        fn enable_raw(&mut self) -> io::Result<()> {
            self.trace.lock().unwrap().push("raw-on");
            Ok(())
        }

        fn enter_alternate_screen(&mut self) -> io::Result<()> {
            self.trace.lock().unwrap().push("alt-on");
            if self.fail_alt {
                Err(io::Error::other("fake alternate-screen failure"))
            } else {
                Ok(())
            }
        }

        fn enable_mouse_capture(&mut self) -> io::Result<()> {
            self.trace.lock().unwrap().push("mouse-on");
            if self.fail_mouse {
                Err(io::Error::other("fake mouse-capture failure"))
            } else {
                Ok(())
            }
        }

        fn disable_mouse_capture(&mut self) -> io::Result<()> {
            self.trace.lock().unwrap().push("mouse-off");
            Ok(())
        }

        fn enable_keyboard_enhancement(&mut self) -> io::Result<()> {
            self.trace.lock().unwrap().push("key-on");
            if self.fail_keyboard {
                Err(io::Error::other("fake keyboard-enhancement failure"))
            } else {
                Ok(())
            }
        }

        fn disable_keyboard_enhancement(&mut self) -> io::Result<()> {
            self.trace.lock().unwrap().push("key-off");
            Ok(())
        }

        fn enable_bracketed_paste(&mut self) -> io::Result<()> {
            self.trace.lock().unwrap().push("paste-on");
            self.paste_attempts += 1;
            if self.fail_paste_attempt == Some(self.paste_attempts) {
                Err(io::Error::other("fake bracketed-paste failure"))
            } else {
                Ok(())
            }
        }

        fn disable_bracketed_paste(&mut self) -> io::Result<()> {
            self.trace.lock().unwrap().push("paste-off");
            if self.fail_disable_paste {
                Err(io::Error::other("fake bracketed-paste cleanup failure"))
            } else {
                Ok(())
            }
        }

        fn leave_alternate_screen(&mut self) -> io::Result<()> {
            self.trace.lock().unwrap().push("alt-off");
            Ok(())
        }

        fn disable_raw(&mut self) -> io::Result<()> {
            self.trace.lock().unwrap().push("raw-off");
            Ok(())
        }
    }

    fn control(
        fail_alt: bool,
        fail_paste_attempt: Option<usize>,
    ) -> (FakeControl, Arc<Mutex<Vec<&'static str>>>) {
        let trace = Arc::new(Mutex::new(Vec::new()));
        (
            FakeControl {
                trace: trace.clone(),
                fail_alt,
                fail_mouse: false,
                fail_keyboard: false,
                fail_paste_attempt,
                fail_disable_paste: false,
                paste_attempts: 0,
            },
            trace,
        )
    }

    #[test]
    fn notification_writer_is_tty_only_and_byte_exact() {
        let mut piped = Vec::new();
        assert!(!write_notification(
            &mut piped,
            crate::ui_settings::NotificationStyle::Bel,
            false,
            false,
        )
        .unwrap());
        assert!(piped.is_empty());

        let mut terminal = Vec::new();
        assert!(write_notification(
            &mut terminal,
            crate::ui_settings::NotificationStyle::Osc9,
            true,
            true,
        )
        .unwrap());
        assert_eq!(terminal, b"\x1bPtmux;\x1b\x1b]9;ca: ready\x07\x1b\\");
    }

    #[test]
    fn mouse_events_keep_wheel_scrolling_and_forward_left_drag_selection() {
        let mouse = |kind, column, row| crossterm::event::MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        assert!(matches!(
            translate_mouse_event(mouse(MouseEventKind::ScrollUp, 3, 4)),
            Some(Event::MouseScroll(MouseScrollEvent {
                direction: ScrollDirection::Up,
                column: 3,
                row: 4,
            }))
        ));
        for (kind, phase) in [
            (
                MouseEventKind::Down(crossterm::event::MouseButton::Left),
                MouseSelectionPhase::Start,
            ),
            (
                MouseEventKind::Drag(crossterm::event::MouseButton::Left),
                MouseSelectionPhase::Drag,
            ),
            (
                MouseEventKind::Up(crossterm::event::MouseButton::Left),
                MouseSelectionPhase::End,
            ),
        ] {
            assert!(matches!(
                translate_mouse_event(mouse(kind, 7, 9)),
                Some(Event::MouseSelection(MouseSelectionEvent {
                    phase: actual,
                    column: 7,
                    row: 9,
                })) if actual == phase
            ));
        }
        assert!(translate_mouse_event(mouse(
            MouseEventKind::Down(crossterm::event::MouseButton::Right),
            7,
            9,
        ))
        .is_none());
    }

    #[test]
    fn successful_guard_drop_releases_in_reverse_order() {
        let (control, trace) = control(false, None);
        let guard = TerminalLifecycle::enter(control, crate::ui_settings::MouseMode::Off).unwrap();
        assert_eq!(
            *trace.lock().unwrap(),
            ["raw-on", "alt-on", "key-on", "paste-on"]
        );
        drop(guard);
        assert_eq!(
            *trace.lock().unwrap(),
            [
                "raw-on",
                "alt-on",
                "key-on",
                "paste-on",
                "paste-off",
                "key-off",
                "alt-off",
                "raw-off"
            ]
        );
    }

    #[test]
    fn restoration_failure_is_contained_and_remaining_cleanup_continues() {
        let (mut control, trace) = control(false, None);
        control.fail_disable_paste = true;
        let guard = TerminalLifecycle::enter(control, crate::ui_settings::MouseMode::Off).unwrap();
        drop(guard);
        assert_eq!(
            *trace.lock().unwrap(),
            [
                "raw-on",
                "alt-on",
                "key-on",
                "paste-on",
                "paste-off",
                "key-off",
                "alt-off",
                "raw-off"
            ]
        );
    }

    #[test]
    fn partial_setup_failure_releases_only_acquired_state() {
        let (control, trace) = control(true, None);
        assert!(TerminalLifecycle::enter(control, crate::ui_settings::MouseMode::Off).is_err());
        assert_eq!(*trace.lock().unwrap(), ["raw-on", "alt-on", "raw-off"]);
    }

    #[test]
    fn bracketed_paste_setup_failure_releases_raw_and_alternate_screen() {
        let (control, trace) = control(false, Some(1));
        assert!(TerminalLifecycle::enter(control, crate::ui_settings::MouseMode::Off).is_err());
        assert_eq!(
            *trace.lock().unwrap(),
            [
                "raw-on",
                "alt-on",
                "key-on",
                "paste-on",
                "paste-off",
                "key-off",
                "alt-off",
                "raw-off"
            ]
        );
    }

    #[test]
    fn suspend_and_resume_release_then_reacquire_terminal() {
        let (control, trace) = control(false, None);
        let mut guard =
            TerminalLifecycle::enter(control, crate::ui_settings::MouseMode::Off).unwrap();
        assert!(guard.focus_change_enabled);
        guard.suspend().unwrap();
        assert!(!guard.focus_change_enabled);
        assert_eq!(
            *trace.lock().unwrap(),
            [
                "raw-on",
                "alt-on",
                "key-on",
                "paste-on",
                "paste-off",
                "key-off",
                "alt-off",
                "raw-off"
            ]
        );
        guard.resume().unwrap();
        assert!(guard.focus_change_enabled);
        assert_eq!(
            *trace.lock().unwrap(),
            [
                "raw-on",
                "alt-on",
                "key-on",
                "paste-on",
                "paste-off",
                "key-off",
                "alt-off",
                "raw-off",
                "raw-on",
                "alt-on",
                "key-on",
                "paste-on"
            ]
        );
        drop(guard);
        assert_eq!(
            *trace.lock().unwrap(),
            [
                "raw-on",
                "alt-on",
                "key-on",
                "paste-on",
                "paste-off",
                "key-off",
                "alt-off",
                "raw-off",
                "raw-on",
                "alt-on",
                "key-on",
                "paste-on",
                "paste-off",
                "key-off",
                "alt-off",
                "raw-off"
            ]
        );
    }

    #[test]
    fn resume_paste_failure_releases_every_reacquired_mode() {
        let (control, trace) = control(false, Some(2));
        let mut guard =
            TerminalLifecycle::enter(control, crate::ui_settings::MouseMode::Off).unwrap();
        guard.suspend().unwrap();

        assert!(guard.resume().is_err());
        assert!(!guard.bracketed_paste_enabled);
        assert!(!guard.alternate_screen_entered);
        assert!(!guard.raw_enabled);
        assert_eq!(
            *trace.lock().unwrap(),
            [
                "raw-on",
                "alt-on",
                "key-on",
                "paste-on",
                "paste-off",
                "key-off",
                "alt-off",
                "raw-off",
                "raw-on",
                "alt-on",
                "key-on",
                "paste-on",
                "paste-off",
                "key-off",
                "alt-off",
                "raw-off"
            ]
        );
        drop(guard);
        assert_eq!(trace.lock().unwrap().len(), 16);
    }

    #[test]
    fn panic_unwinding_releases_terminal_state() {
        let (control, trace) = control(false, None);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard =
                TerminalLifecycle::enter(control, crate::ui_settings::MouseMode::Off).unwrap();
            panic!("fake panic");
        }));
        assert_eq!(
            *trace.lock().unwrap(),
            [
                "raw-on",
                "alt-on",
                "key-on",
                "paste-on",
                "paste-off",
                "key-off",
                "alt-off",
                "raw-off"
            ]
        );
    }

    #[test]
    fn mouse_capture_is_ordered_toggleable_and_best_effort() {
        let (control_impl, trace) = control(false, None);
        let mut guard =
            TerminalLifecycle::enter(control_impl, crate::ui_settings::MouseMode::Scroll).unwrap();
        assert_eq!(
            *trace.lock().unwrap(),
            ["raw-on", "alt-on", "key-on", "mouse-on", "paste-on"]
        );
        assert!(guard
            .set_mouse_mode(crate::ui_settings::MouseMode::Off)
            .unwrap());
        assert_eq!(trace.lock().unwrap().last(), Some(&"mouse-off"));
        assert!(guard
            .set_mouse_mode(crate::ui_settings::MouseMode::Scroll)
            .unwrap());
        drop(guard);
        let trace_snapshot = trace.lock().unwrap().clone();
        assert_eq!(
            &trace_snapshot[trace_snapshot.len().saturating_sub(5)..],
            ["paste-off", "mouse-off", "key-off", "alt-off", "raw-off"]
        );

        let (mut unsupported, unsupported_trace) = control(false, None);
        unsupported.fail_mouse = true;
        let guard =
            TerminalLifecycle::enter(unsupported, crate::ui_settings::MouseMode::Scroll).unwrap();
        assert!(!guard.mouse_capture_enabled);
        assert!(guard.mouse_warning.is_some());
        assert_eq!(
            *unsupported_trace.lock().unwrap(),
            ["raw-on", "alt-on", "key-on", "mouse-on", "paste-on"]
        );
    }

    #[test]
    fn keyboard_enhancement_is_best_effort_and_only_popped_after_success() {
        let (mut unsupported, trace) = control(false, None);
        unsupported.fail_keyboard = true;
        let guard =
            TerminalLifecycle::enter(unsupported, crate::ui_settings::MouseMode::Off).unwrap();
        assert!(!guard.keyboard_enhancement_enabled);
        drop(guard);
        assert_eq!(
            *trace.lock().unwrap(),
            [
                "raw-on",
                "alt-on",
                "key-on",
                "paste-on",
                "paste-off",
                "alt-off",
                "raw-off"
            ]
        );
    }
}
