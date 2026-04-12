//! Headless browser automation for the `browser_action` agent tool.
//!
//! Wraps [`chromiumoxide`] with a simple action-oriented API that persists
//! a single Chromium process per agent task. All public functions are async
//! and intended to be driven from the shared [`ai_runtime`](crate::ai_runtime).

pub mod session;

pub use session::{BrowserSession, ScrollOutcome};
