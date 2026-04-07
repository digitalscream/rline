//! The core highlighting engine.
//!
//! Wraps tree-sitter's [`Highlighter`] to provide full and incremental
//! syntax highlighting. This module has no GTK dependency — it produces
//! [`HighlightSpan`] values that the UI layer converts to `TextTag` applications.

use tree_sitter::{InputEdit, Parser, Point, Tree};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::error::SyntaxError;
use crate::languages::{build_highlight_config, SupportedLanguage};
use crate::span::{HighlightSpan, IncrementalResult};

/// Engine that performs syntax highlighting for a single document.
///
/// Each open editor tab should have its own `HighlightEngine` instance.
/// The engine owns the tree-sitter parser state and the most recent parse tree,
/// enabling efficient incremental re-parses as the user edits.
pub struct HighlightEngine {
    parser: Parser,
    tree: Option<Tree>,
    config: HighlightConfiguration,
    highlighter: Highlighter,
    language: SupportedLanguage,
}

impl std::fmt::Debug for HighlightEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HighlightEngine")
            .field("language", &self.language)
            .field("has_tree", &self.tree.is_some())
            .finish()
    }
}

impl HighlightEngine {
    /// Create a new highlighting engine for the given language.
    ///
    /// # Errors
    ///
    /// Returns [`SyntaxError::QueryError`] if the highlight queries fail to compile,
    /// or [`SyntaxError::UnsupportedLanguage`] if the parser cannot be configured.
    pub fn new(language: SupportedLanguage) -> Result<Self, SyntaxError> {
        let config = build_highlight_config(language)?;

        let mut parser = Parser::new();
        let ts_language = crate::languages::ts_language(language);
        parser
            .set_language(&ts_language)
            .map_err(|e| SyntaxError::UnsupportedLanguage(e.to_string()))?;

        Ok(Self {
            parser,
            tree: None,
            config,
            highlighter: Highlighter::new(),
            language,
        })
    }

    /// Returns the language this engine highlights.
    pub fn language(&self) -> SupportedLanguage {
        self.language
    }

    /// Perform a full parse and highlight of the entire source text.
    ///
    /// Call this when a file is first opened or when the language changes.
    /// Subsequent edits should use [`edit_and_reparse`](Self::edit_and_reparse).
    ///
    /// # Errors
    ///
    /// Returns [`SyntaxError::ParseFailed`] if tree-sitter cannot parse the source,
    /// or [`SyntaxError::HighlightError`] if highlighting fails.
    pub fn parse_and_highlight(
        &mut self,
        source: &[u8],
    ) -> Result<Vec<HighlightSpan>, SyntaxError> {
        // Parse the source
        let tree = self
            .parser
            .parse(source, None)
            .ok_or(SyntaxError::ParseFailed)?;
        self.tree = Some(tree);

        // Run highlight queries
        self.highlight_source(source)
    }

    /// Record edits, incrementally re-parse, and re-highlight changed regions.
    ///
    /// The `edits` slice should contain one [`InputEdit`] per text change since the
    /// last parse. The `source` parameter must be the full, updated source text.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or highlighting fails.
    pub fn edit_and_reparse(
        &mut self,
        edits: &[InputEdit],
        source: &[u8],
    ) -> Result<IncrementalResult, SyntaxError> {
        let old_tree = self.tree.take();

        // Apply edits to the old tree so tree-sitter knows what changed
        let old_tree = old_tree.map(|mut t| {
            for edit in edits {
                t.edit(edit);
            }
            t
        });

        // Incremental parse
        let new_tree = self
            .parser
            .parse(source, old_tree.as_ref())
            .ok_or(SyntaxError::ParseFailed)?;

        // Determine changed byte ranges
        let changed_ranges: Vec<std::ops::Range<usize>> = match old_tree {
            Some(ref old) => new_tree
                .changed_ranges(old)
                .map(|r| r.start_byte..r.end_byte)
                .collect(),
            None => {
                let full_range = 0..source.len();
                vec![full_range]
            }
        };

        self.tree = Some(new_tree);

        // Re-highlight only the changed regions
        let spans = if changed_ranges.is_empty() {
            Vec::new()
        } else {
            self.highlight_source(source)?
                .into_iter()
                .filter(|span| {
                    changed_ranges
                        .iter()
                        .any(|r| span.byte_start < r.end && span.byte_end > r.start)
                })
                .collect()
        };

        Ok(IncrementalResult {
            changed_ranges,
            spans,
        })
    }

    /// Build a [`tree_sitter::InputEdit`] from byte-level change information.
    ///
    /// This is a convenience helper for the UI layer, which typically knows
    /// the byte offset and the old/new text lengths.
    pub fn make_input_edit(
        source_before: &[u8],
        start_byte: usize,
        old_end_byte: usize,
        new_end_byte: usize,
    ) -> InputEdit {
        let start_position = byte_offset_to_point(source_before, start_byte);
        let old_end_position = byte_offset_to_point(source_before, old_end_byte);

        // For the new end position, we need to compute it from the conceptual
        // new source. Since we only have the old source, we compute it relative
        // to the start position and the delta.
        let new_len = new_end_byte - start_byte;
        let old_len = old_end_byte - start_byte;
        let new_end_position = if new_len == old_len {
            old_end_position
        } else {
            // Approximate: assume the new text is on the same line as start
            // (the UI layer can provide a more accurate value if needed)
            Point {
                row: start_position.row,
                column: start_position.column + new_len,
            }
        };

        InputEdit {
            start_byte,
            old_end_byte,
            new_end_byte,
            start_position,
            old_end_position,
            new_end_position,
        }
    }

    /// Run the highlight query on the full source and collect spans.
    fn highlight_source(&mut self, source: &[u8]) -> Result<Vec<HighlightSpan>, SyntaxError> {
        let events = self
            .highlighter
            .highlight(&self.config, source, None, |_| None)
            .map_err(|e| SyntaxError::HighlightError(e.to_string()))?;

        let mut spans = Vec::new();
        let mut highlight_stack: Vec<usize> = Vec::new();

        for event in events {
            let event = event.map_err(|e| SyntaxError::HighlightError(e.to_string()))?;
            match event {
                HighlightEvent::Source { start, end } => {
                    if let Some(&highlight_index) = highlight_stack.last() {
                        spans.push(HighlightSpan {
                            byte_start: start,
                            byte_end: end,
                            highlight_index,
                        });
                    }
                }
                HighlightEvent::HighlightStart(h) => {
                    highlight_stack.push(h.0);
                }
                HighlightEvent::HighlightEnd => {
                    highlight_stack.pop();
                }
            }
        }

        Ok(spans)
    }
}

/// Convert a byte offset in source text to a tree-sitter `Point` (row, column).
fn byte_offset_to_point(source: &[u8], byte_offset: usize) -> Point {
    let offset = byte_offset.min(source.len());
    let mut row = 0;
    let mut last_newline = 0;

    for (i, &byte) in source[..offset].iter().enumerate() {
        if byte == b'\n' {
            row += 1;
            last_newline = i + 1;
        }
    }

    Point {
        row,
        column: offset - last_newline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_offset_to_point_first_line() {
        let source = b"hello world";
        let point = byte_offset_to_point(source, 5);
        assert_eq!(point.row, 0);
        assert_eq!(point.column, 5);
    }

    #[test]
    fn test_byte_offset_to_point_second_line() {
        let source = b"hello\nworld";
        let point = byte_offset_to_point(source, 8);
        assert_eq!(point.row, 1, "should be on second line");
        assert_eq!(point.column, 2, "should be 2 bytes into second line");
    }

    #[test]
    fn test_byte_offset_to_point_at_newline() {
        let source = b"abc\ndef\nghi";
        let point = byte_offset_to_point(source, 4);
        assert_eq!(point.row, 1);
        assert_eq!(point.column, 0);
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn test_parse_and_highlight_rust() {
        let mut engine =
            HighlightEngine::new(SupportedLanguage::Rust).expect("should create Rust engine");

        let source = b"fn main() { let x: u32 = 42; }";
        let spans = engine
            .parse_and_highlight(source)
            .expect("should highlight Rust source");

        assert!(
            !spans.is_empty(),
            "should produce highlight spans for Rust code"
        );

        // Check that we got a keyword span for "fn"
        let has_keyword = spans.iter().any(|s| {
            let text = &source[s.byte_start..s.byte_end];
            text == b"fn"
        });
        assert!(has_keyword, "should highlight 'fn' keyword");
    }

    #[cfg(feature = "lang-python")]
    #[test]
    fn test_parse_and_highlight_python() {
        let mut engine =
            HighlightEngine::new(SupportedLanguage::Python).expect("should create Python engine");

        let source = b"def hello(name: str) -> None:\n    print(name)";
        let spans = engine
            .parse_and_highlight(source)
            .expect("should highlight Python source");

        assert!(
            !spans.is_empty(),
            "should produce highlight spans for Python code"
        );
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn test_incremental_reparse() {
        let mut engine =
            HighlightEngine::new(SupportedLanguage::Rust).expect("should create Rust engine");

        let source = b"fn main() { let x = 1; }";
        engine
            .parse_and_highlight(source)
            .expect("initial parse should succeed");

        // Simulate adding a new statement — a structural change that ensures
        // tree-sitter reports changed ranges.
        let new_source = b"fn main() { let x = 1; let y = \"hello\"; }";
        let edit = HighlightEngine::make_input_edit(source, 23, 23, new_source.len() - 1);

        let result = engine
            .edit_and_reparse(&[edit], new_source)
            .expect("incremental reparse should succeed");

        // The result should contain spans (the newly added string literal etc.)
        // Note: changed_ranges may be empty for trivial edits where the tree
        // structure doesn't change, but the re-highlight should still succeed.
        assert!(
            !result.spans.is_empty() || result.changed_ranges.is_empty(),
            "should produce spans for changed regions or report no structural changes"
        );
    }

    #[cfg(feature = "lang-json")]
    #[test]
    fn test_json_highlighting() {
        let mut engine =
            HighlightEngine::new(SupportedLanguage::Json).expect("should create JSON engine");

        let source = br#"{"key": "value", "num": 42, "bool": true}"#;
        let spans = engine
            .parse_and_highlight(source)
            .expect("should highlight JSON source");

        assert!(!spans.is_empty(), "should produce highlight spans for JSON");
    }

    #[test]
    fn test_empty_source() {
        #[cfg(feature = "lang-rust")]
        {
            let mut engine =
                HighlightEngine::new(SupportedLanguage::Rust).expect("should create engine");
            let spans = engine
                .parse_and_highlight(b"")
                .expect("empty source should not fail");
            assert!(spans.is_empty(), "empty source should produce no spans");
        }
    }

    #[test]
    fn test_make_input_edit() {
        let source = b"hello\nworld";
        let edit = HighlightEngine::make_input_edit(source, 5, 5, 9);
        assert_eq!(edit.start_byte, 5);
        assert_eq!(edit.old_end_byte, 5);
        assert_eq!(edit.new_end_byte, 9);
        assert_eq!(edit.start_position.row, 0);
        assert_eq!(edit.start_position.column, 5);
    }
}
