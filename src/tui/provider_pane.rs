//! Scroll state for the provider-copy side pane.
//!
//! The text is the request body after the filter rewrites it. Geometry is
//! recorded while painting so the wheel can hit the pane.

use ratatui::layout::{Position, Rect};
use std::cell::{Cell, RefCell};
use std::sync::Arc;

#[derive(Debug)]
pub struct ProviderPane {
    generation: Cell<u64>,
    text: RefCell<Arc<str>>,
    scroll: Cell<u16>,
    max_scroll: Cell<u16>,
    visible_rows: Cell<u16>,
    region: Cell<Rect>,
}

impl Default for ProviderPane {
    fn default() -> Self {
        Self {
            generation: Cell::new(0),
            text: RefCell::new(Arc::from("")),
            scroll: Cell::new(0),
            max_scroll: Cell::new(0),
            visible_rows: Cell::new(1),
            region: Cell::new(Rect::default()),
        }
    }
}

impl ProviderPane {
    pub fn note(&self, generation: u64, text: Arc<str>) {
        if self.generation.get() == generation {
            return;
        }
        self.generation.set(generation);
        self.scroll.set(0);
        *self.text.borrow_mut() = text;
    }

    pub fn is_empty(&self) -> bool {
        self.text.borrow().is_empty()
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
