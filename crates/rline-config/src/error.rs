//! Error types for rline-config.

/// Errors that can occur during configuration operations.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Failed to read or write the configuration file.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to parse or serialize configuration.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Could not determine the configuration directory.
    #[error("could not determine configuration directory")]
    NoConfigDir,

    /// Could not determine the XDG data directory.
    #[error("could not determine data directory")]
    NoDataDir,

    /// No VS Code extensions directory found on this system.
    #[error("no VS Code extensions found")]
    NoVscodeExtensions,

    /// A VS Code theme file was invalid or could not be converted.
    #[error("invalid VS Code theme: {0}")]
    InvalidVscodeTheme(String),
}
