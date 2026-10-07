pub mod commands;
pub mod composer;
pub mod dashboard;
pub mod event;
pub mod input;
pub(crate) mod output_layout;
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
