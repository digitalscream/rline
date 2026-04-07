//! Search result types shared across the workspace.

use std::path::PathBuf;

use crate::position::LineIndex;

/// A single match found during a project-wide text search.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// The file path containing the match.
    pub path: PathBuf,
    /// The line number where the match was found.
    pub line_number: LineIndex,
    /// The full text of the matching line.
    pub line_text: String,
    /// Byte offset of the match start within the line.
    pub match_start: usize,
    /// Byte offset of the match end within the line.
    pub match_end: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_search_result_fields_are_accessible() {
        let result = SearchResult {
            path: PathBuf::from("/tmp/example.rs"),
            line_number: LineIndex(10),
            line_text: "let x = 42;".to_owned(),
            match_start: 4,
            match_end: 5,
        };

        assert_eq!(
            result.path,
            PathBuf::from("/tmp/example.rs"),
            "path should match"
        );
        assert_eq!(
            result.line_number,
            LineIndex(10),
            "line_number should match"
        );
        assert_eq!(result.line_text, "let x = 42;", "line_text should match");
        assert_eq!(result.match_start, 4, "match_start should match");
        assert_eq!(result.match_end, 5, "match_end should match");
    }

    #[test]
    fn test_search_result_clone() {
        let result = SearchResult {
            path: PathBuf::from("/src/main.rs"),
            line_number: LineIndex(0),
            line_text: "fn main() {}".to_owned(),
            match_start: 3,
            match_end: 7,
        };

        let cloned = result.clone();
        assert_eq!(
            cloned.path, result.path,
            "cloned path should equal original"
        );
        assert_eq!(
            cloned.line_number, result.line_number,
            "cloned line_number should equal original"
        );
        assert_eq!(
            cloned.line_text, result.line_text,
            "cloned line_text should equal original"
        );
    }

    #[test]
    fn test_search_result_debug_format() {
        let result = SearchResult {
            path: PathBuf::from("test.rs"),
            line_number: LineIndex(1),
            line_text: "hello".to_owned(),
            match_start: 0,
            match_end: 5,
        };

        let debug = format!("{result:?}");
        assert!(
            debug.contains("SearchResult"),
            "debug output should contain type name"
        );
    }
}
