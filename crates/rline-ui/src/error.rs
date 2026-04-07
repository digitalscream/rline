//! Error types for rline-ui.

/// Errors that can occur in UI operations.
#[derive(Debug, thiserror::Error)]
pub enum UiError {
    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A configuration error occurred.
    #[error("configuration error: {0}")]
    Config(#[from] rline_config::ConfigError),

    /// A file could not be opened or read.
    #[error("failed to open file: {path}")]
    FileOpen {
        /// The path that failed to open.
        path: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },

    /// A git operation failed.
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
}
