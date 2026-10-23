pub mod commands;
pub mod composer;
pub mod dashboard;
pub mod event;
pub mod input;
pub(crate) mod output_layout;
pub(crate) mod personality;
pub(crate) mod provider_pane;
pub mod render;
pub mod selection;
pub mod transcript;

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
#[path = "input_tests.rs"]
mod input_tests;

#[cfg(test)]
#[path = "render_tests.rs"]
mod render_tests;

pub use event::Event;
pub use render::render;

pub(crate) const VERSION_LABEL: &str = concat!("ca v", env!("CARGO_PKG_VERSION"));
