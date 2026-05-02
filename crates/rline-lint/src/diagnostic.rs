//! Plain data type representing a single lint diagnostic.
//!
//! A `Diagnostic` is intentionally renderer-agnostic: the same value can be
//! displayed in the Problems panel, drawn as a gutter mark, rendered as an
//! inline squiggle, or exported as JSON. Future lint-as-you-type and LSP
//! integration plug into this same type.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Severity of a diagnostic, mirroring LSP's `DiagnosticSeverity` for forward
/// compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A hard error — the code is broken.
    Error,
    /// A warning — the code may work but is suspect.
    Warning,
    /// Informational — usually style or convention guidance.
    Info,
    /// A hint — typically suggests an improvement.
    Hint,
}

impl Severity {
    /// Return a short label suitable for grouping or filtering UI.
    pub fn label(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
            Severity::Hint => "hint",
        }
    }
}

/// A 0-indexed `(line, column)` position within a source file. Line numbers
/// are zero-based to match GTK `TextIter`; UI code should display them
/// one-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    /// Zero-indexed line number.
    pub line: u32,
    /// Zero-indexed column (in UTF-16 code units, matching LSP).
    pub column: u32,
}

impl Position {
    /// Build a new position.
    pub fn new(line: u32, column: u32) -> Self {
        Self { line, column }
    }
}

/// A half-open `[start, end)` range within a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    /// Start position (inclusive).
    pub start: Position,
    /// End position (exclusive).
    pub end: Position,
}

impl Range {
    /// A zero-width range at the given position.
    pub fn point(pos: Position) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }
}

/// A single diagnostic emitted by a linter or formatter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Absolute path to the file the diagnostic applies to.
    pub path: PathBuf,
    /// Range within the file.
    pub range: Range,
    /// Severity of the diagnostic.
    pub severity: Severity,
    /// Human-readable message.
    pub message: String,
    /// Linter-specific code (e.g. `clippy::needless_clone`, `E0502`,
    /// `Style/IndentationWidth`).
    pub code: Option<String>,
    /// Linter that produced this diagnostic (e.g. `clippy`, `ruff`,
    /// `rubocop`).
    pub source: String,
}

impl Diagnostic {
    /// Convenience constructor for a single-line diagnostic.
    pub fn at_line(
        path: PathBuf,
        line: u32,
        column: u32,
        severity: Severity,
        message: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        let pos = Position::new(line, column);
        Self {
            path,
            range: Range::point(pos),
            severity,
            message: message.into(),
            code: None,
            source: source.into(),
        }
    }
}
