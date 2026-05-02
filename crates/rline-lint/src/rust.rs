//! Rust support: `rustfmt` for formatting and `cargo clippy` for linting.
//!
//! The clippy implementation parses the JSON-Lines output of
//! `cargo clippy --message-format=json` directly rather than pulling in the
//! `cargo_metadata` crate, which would be a sizeable dependency for the small
//! subset of fields we actually consume.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::diagnostic::{Diagnostic, Position, Range, Severity};
use crate::error::LintError;
use crate::provider::{Formatter, LintProvider};
use crate::util::{run_command, stderr_lossy, stdout_string, tool_failed};

/// `rustfmt` formatter.
#[derive(Debug, Clone)]
pub struct RustFmt {
    binary: String,
}

impl RustFmt {
    /// Build a new formatter using the given binary (typically `"rustfmt"`).
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }
}

impl Default for RustFmt {
    fn default() -> Self {
        Self::new("rustfmt")
    }
}

impl Formatter for RustFmt {
    fn format(&self, source: &str, _path: &Path) -> Result<String, LintError> {
        // rustfmt reads stdin by default and writes to stdout. `--emit stdout`
        // is the explicit form. `--edition 2021` is conservative — when a
        // project-local `rustfmt.toml` specifies an edition, it wins.
        let output = run_command(
            "rustfmt",
            &self.binary,
            &["--emit", "stdout", "--edition", "2021"],
            None,
            Some(source),
        )?;
        if !output.status.success() {
            return Err(tool_failed("rustfmt", &output));
        }
        stdout_string("rustfmt", &output)
    }

    fn name(&self) -> &'static str {
        "rustfmt"
    }
}

/// `cargo clippy` lint provider.
#[derive(Debug, Clone)]
pub struct CargoClippy {
    binary: String,
}

impl CargoClippy {
    /// Build a new clippy provider using the given cargo binary (typically
    /// `"cargo"`).
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }
}

impl Default for CargoClippy {
    fn default() -> Self {
        Self::new("cargo")
    }
}

impl LintProvider for CargoClippy {
    fn lint_project(&self, root: &Path) -> Result<Vec<Diagnostic>, LintError> {
        let output = run_command(
            "clippy",
            &self.binary,
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--message-format=json",
                "--quiet",
            ],
            Some(root),
            None,
        )?;
        // `cargo clippy` exits non-zero when it finds errors, but the JSON
        // output is still valid. Only treat it as a tool failure if stdout is
        // empty AND stderr looks like a hard error.
        if output.stdout.is_empty() && !output.status.success() {
            return Err(LintError::ToolFailed {
                tool: "clippy".to_owned(),
                code: output.status.code(),
                stderr: stderr_lossy(&output),
            });
        }
        let stdout = stdout_string("clippy", &output)?;
        Ok(parse_clippy_output(&stdout, root))
    }

    fn lint_file(&self, _path: &Path, _source: &str) -> Result<Vec<Diagnostic>, LintError> {
        // Clippy has no per-file mode — it must compile the whole crate.
        // Returning an explicit error lets callers fall back gracefully (e.g.
        // a future lint-as-you-type implementation can skip Rust live and
        // rely on save-time project lint).
        Err(LintError::NoLintProvider("rust (per-file)".to_owned()))
    }

    fn name(&self) -> &'static str {
        "clippy"
    }
}

#[derive(Debug, Deserialize)]
struct CargoLine {
    reason: String,
    message: Option<CargoMessage>,
}

#[derive(Debug, Deserialize)]
struct CargoMessage {
    message: String,
    level: String,
    code: Option<CargoCode>,
    spans: Vec<CargoSpan>,
}

#[derive(Debug, Deserialize)]
struct CargoCode {
    code: String,
}

#[derive(Debug, Deserialize)]
struct CargoSpan {
    file_name: String,
    line_start: u32,
    column_start: u32,
    line_end: u32,
    column_end: u32,
    is_primary: bool,
}

fn parse_clippy_output(stdout: &str, root: &Path) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: CargoLine = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if parsed.reason != "compiler-message" {
            continue;
        }
        let Some(msg) = parsed.message else { continue };
        // Pick the primary span; if none is marked primary, take the first.
        let span = msg
            .spans
            .iter()
            .find(|s| s.is_primary)
            .or(msg.spans.first());
        let Some(span) = span else { continue };
        let severity = match msg.level.as_str() {
            "error" | "error: internal compiler error" => Severity::Error,
            "warning" => Severity::Warning,
            "note" | "help" => Severity::Hint,
            _ => Severity::Info,
        };
        let code = msg.code.as_ref().map(|c| c.code.clone());
        let path = resolve_span_path(root, &span.file_name);
        // cargo's columns are 1-indexed; we store 0-indexed.
        let range = Range {
            start: Position::new(
                span.line_start.saturating_sub(1),
                span.column_start.saturating_sub(1),
            ),
            end: Position::new(
                span.line_end.saturating_sub(1),
                span.column_end.saturating_sub(1),
            ),
        };
        diagnostics.push(Diagnostic {
            path,
            range,
            severity,
            message: msg.message,
            code,
            source: "clippy".to_owned(),
        });
    }
    diagnostics
}

fn resolve_span_path(root: &Path, file_name: &str) -> PathBuf {
    let p = PathBuf::from(file_name);
    if p.is_absolute() {
        p
    } else {
        root.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_compiler_message_line() {
        let line = r#"{"reason":"compiler-message","message":{"message":"unused variable: `x`","level":"warning","code":{"code":"unused_variables"},"spans":[{"file_name":"src/lib.rs","line_start":3,"column_start":9,"line_end":3,"column_end":10,"is_primary":true}]}}"#;
        let diags = parse_clippy_output(line, Path::new("/tmp/proj"));
        assert_eq!(diags.len(), 1, "should parse a single diagnostic");
        let d = &diags[0];
        assert_eq!(d.path, PathBuf::from("/tmp/proj/src/lib.rs"));
        assert_eq!(
            d.range.start.line, 2,
            "1-indexed lines should become 0-indexed"
        );
        assert_eq!(d.range.start.column, 8);
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.code.as_deref(), Some("unused_variables"));
        assert_eq!(d.source, "clippy");
        assert!(d.message.contains("unused variable"));
    }

    #[test]
    fn skips_non_message_lines() {
        let lines = "\
{\"reason\":\"compiler-artifact\",\"target\":{}}\n\
{\"reason\":\"build-finished\",\"success\":true}\n";
        let diags = parse_clippy_output(lines, Path::new("/tmp"));
        assert!(diags.is_empty(), "non-message reasons must be skipped");
    }

    #[test]
    fn handles_message_without_spans() {
        let line = r#"{"reason":"compiler-message","message":{"message":"build failed","level":"error","spans":[]}}"#;
        let diags = parse_clippy_output(line, Path::new("/tmp"));
        assert!(diags.is_empty(), "spanless messages are skipped");
    }

    #[test]
    fn picks_primary_span_when_multiple() {
        // Single line — clippy emits one JSON object per line on stdout.
        let line = r#"{"reason":"compiler-message","message":{"message":"borrow","level":"error","code":{"code":"E0502"},"spans":[{"file_name":"src/a.rs","line_start":10,"column_start":1,"line_end":10,"column_end":2,"is_primary":false},{"file_name":"src/b.rs","line_start":20,"column_start":3,"line_end":20,"column_end":4,"is_primary":true}]}}"#;
        let diags = parse_clippy_output(line, Path::new("/tmp"));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].path, PathBuf::from("/tmp/src/b.rs"));
    }

    #[test]
    fn ignores_garbage_lines() {
        let lines = "this is not json\n\n{not valid}\n";
        let diags = parse_clippy_output(lines, Path::new("/tmp"));
        assert!(diags.is_empty(), "garbage lines are silently skipped");
    }
}
