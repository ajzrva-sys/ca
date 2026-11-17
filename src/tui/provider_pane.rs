//! Scroll state for the provider-copy side pane.
//!
//! The text is the request body after the filter rewrites it. Geometry is
//! recorded while painting so the wheel can hit the pane.

use ratatui::layout::{Position, Rect};
use std::cell::{Cell, RefCell};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Legacy,
    Privacy,
}

#[derive(Debug)]
pub struct ProviderPane {
    /// Explicit presentation state; filtering never opens this pane.
    pub visible: Cell<bool>,
    source: Cell<Source>,
    generation: Cell<u64>,
    text: RefCell<Arc<str>>,
    scroll: Cell<u16>,
    max_scroll: Cell<u16>,
    visible_rows: Cell<u16>,
    region: Cell<Rect>,
    /// True when this frame allocated the side pane.
    frame_open: Cell<bool>,
}

impl Default for ProviderPane {
    fn default() -> Self {
        Self {
            visible: Cell::new(false),
            source: Cell::new(Source::Legacy),
            generation: Cell::new(0),
            text: RefCell::new(Arc::from("")),
            scroll: Cell::new(0),
            max_scroll: Cell::new(0),
            visible_rows: Cell::new(1),
            region: Cell::new(Rect::default()),
            frame_open: Cell::new(false),
        }
    }
}

impl ProviderPane {
    pub fn note(&self, generation: u64, text: Arc<str>) {
        self.note_source(Source::Legacy, generation, text);
    }

    pub(crate) fn note_privacy(&self, generation: u64, text: Arc<str>) {
        self.note_source(Source::Privacy, generation, text);
    }

    fn note_source(&self, source: Source, generation: u64, text: Arc<str>) {
        // Legacy generations identify immutable requests. Privacy generations
        // belong to their local context, so another context's snapshot or a
        // mode switch can replace the source even at the same generation.
        if self.source.get() == source
            && self.generation.get() == generation
            && (source == Source::Legacy || Arc::ptr_eq(&self.text.borrow(), &text))
        {
            return;
        }
        self.source.set(source);
        self.generation.set(generation);
        self.scroll.set(0);
        *self.text.borrow_mut() = text;
    }

    pub(crate) fn clear_content(&self) {
        self.source.set(Source::Legacy);
        if !self.is_empty() || self.generation.get() != 0 {
            self.generation.set(0);
            self.scroll.set(0);
            *self.text.borrow_mut() = Arc::from("");
        }
    }

    pub(crate) fn set_frame_open(&self, open: bool) {
        self.frame_open.set(open);
    }

    pub(crate) fn frame_open(&self) -> bool {
        self.frame_open.get()
    }

    pub fn is_empty(&self) -> bool {
        self.text.borrow().is_empty()
    }

    pub fn generation(&self) -> u64 {
        self.generation.get()
    }

    pub fn text(&self) -> Arc<str> {
        Arc::clone(&self.text.borrow())
    }

    pub fn scroll(&self) -> u16 {
        self.scroll.get().min(self.max_scroll.get())
    }

    pub fn set_viewport(&self, region: Rect, visible_rows: u16, line_count: usize) {
        let visible = usize::from(visible_rows.max(1));
        let max_scroll = u16::try_from(line_count.saturating_sub(visible)).unwrap_or(u16::MAX);
        if self.scroll.get() > max_scroll {
            self.scroll.set(max_scroll);
        }
        self.max_scroll.set(max_scroll);
        self.visible_rows.set(visible_rows.max(1));
        self.region.set(region);
    }

    pub fn clear_viewport(&self) {
        self.region.set(Rect::default());
        self.max_scroll.set(0);
        self.visible_rows.set(1);
    }

    pub fn contains(&self, column: u16, row: u16) -> bool {
        let region = self.region.get();
        !region.is_empty() && region.contains(Position::new(column, row))
    }

    pub fn scroll_by(&self, delta: i32) {
        let max = i32::from(self.max_scroll.get());
        let next = i32::from(self.scroll.get())
            .saturating_add(delta)
            .clamp(0, max);
        self.scroll.set(u16::try_from(next).unwrap_or(0));
    }

    pub fn scroll_page(&self, delta_pages: i32) {
        let page = i32::from(self.visible_rows.get().max(1));
        self.scroll_by(delta_pages.saturating_mul(page));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scroll_three(pane: &ProviderPane) {
        pane.set_viewport(Rect::new(60, 2, 40, 20), 18, 80);
        pane.scroll_by(3);
        assert_eq!(pane.scroll(), 3);
    }

    #[test]
    fn provider_pane_legacy_same_generation_retains_body_and_scroll() {
        let pane = ProviderPane::default();
        pane.note(2, Arc::from("LATEST-REQUEST\nold history"));
        scroll_three(&pane);
        pane.note(2, Arc::from("same request is still working"));
        assert_eq!(pane.scroll(), 3);
        assert_eq!(pane.text().as_ref(), "LATEST-REQUEST\nold history");
    }

    #[test]
    fn provider_pane_privacy_source_switch_and_new_request_follow_latest() {
        let pane = ProviderPane::default();
        pane.note(1, Arc::from("legacy request"));
        scroll_three(&pane);
        let first: Arc<str> = Arc::from("first privacy context");
        pane.note_privacy(1, first.clone());
        assert_eq!(pane.scroll(), 0);
        assert_eq!(pane.text().as_ref(), "first privacy context");

        scroll_three(&pane);
        pane.note_privacy(1, first);
        assert_eq!(
            pane.scroll(),
            3,
            "unchanged pending snapshot keeps scrolling"
        );
        pane.note_privacy(1, Arc::from("another privacy context"));
        assert_eq!(pane.scroll(), 0);
        assert_eq!(pane.text().as_ref(), "another privacy context");

        scroll_three(&pane);
        pane.note_privacy(2, Arc::from("newest pending privacy request"));
        assert_eq!(pane.scroll(), 0);
        assert_eq!(pane.text().as_ref(), "newest pending privacy request");

        scroll_three(&pane);
        pane.note(2, Arc::from("legacy request after privacy"));
        assert_eq!(pane.scroll(), 0);
        assert_eq!(pane.text().as_ref(), "legacy request after privacy");
        scroll_three(&pane);
        pane.note(2, Arc::from("same legacy request still working"));
        assert_eq!(pane.scroll(), 3);
    }
}
