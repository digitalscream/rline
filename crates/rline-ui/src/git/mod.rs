//! Git integration — status panel, staging actions, and diff views.

mod diff_tab;
mod git_panel;
mod git_status_row;
pub mod git_worker;

pub use diff_tab::DiffTab;
pub use git_panel::GitPanel;
