//! rline-lint — Formatter and lint provider abstractions plus
//! per-language wrappers around mature external tools (`rustfmt`, `clippy`,
//! `ruff`, `prettier`, `eslint`, `rubocop`).
//!
//! # Design
//!
//! Two narrow traits — [`Formatter`] and [`LintProvider`] — front every
//! language. Concrete implementations spawn external processes; future LSP
//! clients can implement the same traits without changes to the UI.
//!
//! [`LintRegistry`] maps a [`SupportedLanguage`](rline_syntax::languages::SupportedLanguage)
//! onto a [`LanguageEntry`](registry::LanguageEntry) carrying the configured
//! formatter and linter. The registry is built from a [`LintSettings`]
//! snapshot and can be cheaply rebuilt when settings change.
//!
//! # Concurrency
//!
//! All trait methods are blocking and must be invoked from a worker thread —
//! never from the GTK main thread.

pub mod diagnostic;
pub mod error;
pub mod javascript;
pub mod provider;
pub mod python;
pub mod registry;
pub mod ruby;
pub mod rust;
pub mod settings;
mod util;

pub use diagnostic::{Diagnostic, Position, Range, Severity};
pub use error::LintError;
pub use provider::{Formatter, LintProvider};
pub use registry::{LanguageEntry, LintRegistry};
pub use settings::LintSettings;
