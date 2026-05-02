//! Traits for formatters and lint providers.
//!
//! These are deliberately narrow and LSP-shaped so that today's external-tool
//! wrappers can be swapped for in-process LSP clients later without touching
//! the UI.

use std::path::Path;

use crate::diagnostic::Diagnostic;
use crate::error::LintError;

/// Reformat source text. Implementations typically pipe `source` to an
/// external tool's stdin and return the tool's stdout.
///
/// The `path` argument is a hint used by tools that pick configuration based
/// on file location (e.g. `prettier` walking up to `.prettierrc`, `rustfmt`
/// finding `rustfmt.toml`). It need not exist on disk.
pub trait Formatter: Send + Sync {
    /// Format `source` and return the reformatted text.
    fn format(&self, source: &str, path: &Path) -> Result<String, LintError>;

    /// A short, stable identifier used in logs and diagnostics (e.g.
    /// `"rustfmt"`, `"prettier"`).
    fn name(&self) -> &'static str;
}

/// Lint a project tree or a single file.
///
/// `lint_file` is intentionally separate from `lint_project` so that a future
/// lint-as-you-type implementation can call it on every keystroke (debounced)
/// without rerunning a whole-project scan. Implementations that only support
/// project-level lint (e.g. `cargo clippy`) should return
/// [`LintError::NoLintProvider`] from `lint_file`.
pub trait LintProvider: Send + Sync {
    /// Lint the entire project rooted at `root`.
    fn lint_project(&self, root: &Path) -> Result<Vec<Diagnostic>, LintError>;

    /// Lint a single file. `source` is the in-memory buffer contents (which
    /// may differ from what's on disk); implementations that pipe via stdin
    /// should use it directly.
    fn lint_file(&self, path: &Path, source: &str) -> Result<Vec<Diagnostic>, LintError>;

    /// A short, stable identifier used in logs and the `Diagnostic.source`
    /// field (e.g. `"clippy"`, `"ruff"`, `"rubocop"`).
    fn name(&self) -> &'static str;
}
