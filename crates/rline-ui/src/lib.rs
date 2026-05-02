//! rline-ui — GTK4 widgets, editor view, panels, and theming for rline.

pub mod agent;
mod app;
pub mod editor;
pub mod error;
pub mod file_browser;
pub mod git;
pub mod lint;
mod menu;
pub mod search;
mod shortcuts;
mod shortcuts_dialog;
pub mod status_bar;
pub mod terminal;
pub mod theming;
mod window;

pub use app::RlineApplication;

/// Run the rline GTK application, returning the exit code.
pub fn run() -> i32 {
    use gtk4::prelude::*;
    let app = RlineApplication::new();
    app.run().into()
}
