//! rline-syntax — Tree-sitter integration for incremental syntax highlighting.
//!
//! This crate provides a language-aware highlighting engine built on
//! [tree-sitter](https://tree-sitter.github.io/tree-sitter/). It has no GTK
//! dependency — the UI layer in `rline-ui` converts the [`HighlightSpan`] output
//! into GTK `TextTag` applications on a `sourceview5::Buffer`.
//!
//! # Architecture
//!
//! - [`languages`] — maps file extensions to tree-sitter grammars
//! - [`scope_map`] — maps tree-sitter capture names to GtkSourceView style IDs
//! - [`engine`] — owns the parser and produces highlight spans
//!
//! # Example
//!
//! ```no_run
//! use rline_syntax::engine::HighlightEngine;
//! use rline_syntax::languages::{language_for_extension, SupportedLanguage};
//! use rline_syntax::scope_map;
//!
//! // Look up the language for a .rs file
//! if let Some(lang) = language_for_extension("rs") {
//!     let mut engine = HighlightEngine::new(lang).unwrap();
//!     let spans = engine.parse_and_highlight(b"fn main() {}").unwrap();
//!     for span in &spans {
//!         if let Some(style_id) = scope_map::highlight_to_style_id(span.highlight_index) {
//!             println!("bytes {}..{} -> {}", span.byte_start, span.byte_end, style_id);
//!         }
//!     }
//! }
//! ```

pub mod engine;
pub mod error;
pub mod languages;
pub mod scope_map;
pub mod span;

// Re-export key types at the crate root for convenience.
pub use engine::HighlightEngine;
pub use error::SyntaxError;
pub use languages::{language_for_extension, SupportedLanguage};
pub use span::{HighlightSpan, IncrementalResult};
