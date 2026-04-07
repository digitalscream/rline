//! Error types for rline-core.

/// Errors that can occur in core document operations.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// A position was out of bounds for the document.
    #[error("position {0} out of bounds for document of length {1}")]
    PositionOutOfBounds(usize, usize),

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
