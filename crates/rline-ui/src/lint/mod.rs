//! Linting and formatting integration for the editor UI.
//!
//! - [`lint_worker`] spawns external lint/format tools on background threads
//!   and marshals results back to the GTK main loop.
//! - [`format_action`] wires the manual "format buffer" action and the
//!   format-on-save flow into editor tabs.
//! - [`problems_panel`] is the left-pane "Problems" tab that displays
//!   diagnostics grouped by file.

pub mod format_action;
pub mod lint_worker;
pub mod problems_panel;

pub use problems_panel::ProblemsPanel;
