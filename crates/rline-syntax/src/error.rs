//! Error types for the syntax highlighting engine.

/// Errors that can occur during syntax highlighting operations.
#[derive(Debug, thiserror::Error)]
pub enum SyntaxError {
    /// No tree-sitter grammar is available for the requested language.
    #[error("no tree-sitter grammar available for language: {0}")]
    UnsupportedLanguage(String),

    /// Failed to compile a tree-sitter highlight query.
    #[error("failed to compile highlight query: {0}")]
    QueryError(String),

    /// Tree-sitter parsing failed.
    #[error("parsing failed")]
    ParseFailed,

    /// Tree-sitter highlighting failed.
    #[error("highlighting failed: {0}")]
    HighlightError(String),
}
