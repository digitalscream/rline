//! Data types for highlight results.

use std::ops::Range;

/// A single highlighted region in the source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    /// Byte offset of the start of this span in the source text.
    pub byte_start: usize,
    /// Byte offset of the end of this span in the source text.
    pub byte_end: usize,
    /// Index into the highlight names array (maps to a capture name like "keyword").
    pub highlight_index: usize,
}

/// Result of an incremental re-parse and re-highlight.
#[derive(Debug, Clone)]
pub struct IncrementalResult {
    /// Byte ranges in the source that changed and were re-highlighted.
    pub changed_ranges: Vec<Range<usize>>,
    /// All highlight spans within the changed ranges.
    pub spans: Vec<HighlightSpan>,
}
