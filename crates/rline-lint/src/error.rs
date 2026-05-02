//! Error type for the lint crate.

use std::path::PathBuf;
use std::string::FromUtf8Error;

/// Errors produced by formatters and lint providers.
#[derive(Debug, thiserror::Error)]
pub enum LintError {
    /// The configured tool binary was not found on `PATH`.
    #[error("tool '{0}' not found on PATH")]
    ToolNotFound(String),

    /// The tool exited with a non-zero status and produced no useful output.
    #[error("tool '{tool}' failed (exit {code:?}): {stderr}")]
    ToolFailed {
        /// The tool that was invoked.
        tool: String,
        /// The exit code, or `None` if the process was killed by a signal.
        code: Option<i32>,
        /// Captured stderr (trimmed).
        stderr: String,
    },

    /// The tool produced output that could not be parsed.
    #[error("failed to parse output from '{tool}': {message}")]
    ParseError {
        /// The tool whose output failed to parse.
        tool: String,
        /// A short description of what went wrong.
        message: String,
    },

    /// I/O error while spawning a tool or reading its output.
    #[error("I/O error invoking '{tool}': {source}")]
    Io {
        /// The tool that was being invoked.
        tool: String,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Output bytes were not valid UTF-8.
    #[error("invalid UTF-8 in '{tool}' output")]
    InvalidUtf8 {
        /// The tool whose output was invalid.
        tool: String,
        /// The underlying error.
        #[source]
        source: FromUtf8Error,
    },

    /// No formatter is registered for the given language.
    #[error("no formatter registered for language '{0}'")]
    NoFormatter(String),

    /// No lint provider is registered for the given language.
    #[error("no lint provider registered for language '{0}'")]
    NoLintProvider(String),

    /// The given path is not under any project root we can lint.
    #[error("path is outside any known project root: {0}")]
    PathOutsideProject(PathBuf),
}
