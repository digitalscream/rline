//! Python support: `ruff format` and `ruff check` (single binary).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::diagnostic::{Diagnostic, Position, Range, Severity};
use crate::error::LintError;
use crate::provider::{Formatter, LintProvider};
use crate::util::{run_command, stdout_string, tool_failed};

/// `ruff format` formatter.
#[derive(Debug, Clone)]
pub struct RuffFormat {
    binary: String,
}

impl RuffFormat {
    /// Build a new formatter using the given binary (typically `"ruff"`).
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }
}

impl Default for RuffFormat {
    fn default() -> Self {
        Self::new("ruff")
    }
}

impl Formatter for RuffFormat {
    fn format(&self, source: &str, path: &Path) -> Result<String, LintError> {
        // `ruff format -` reads from stdin and writes to stdout. The
        // `--stdin-filename` argument lets ruff resolve `pyproject.toml`
        // relative to the buffer's location.
        let path_str = path.display().to_string();
        let output = run_command(
            "ruff",
            &self.binary,
            &["format", "--stdin-filename", &path_str, "-"],
            None,
            Some(source),
        )?;
        if !output.status.success() {
            return Err(tool_failed("ruff", &output));
        }
        stdout_string("ruff", &output)
    }

    fn name(&self) -> &'static str {
        "ruff format"
    }
}

/// `ruff check` lint provider.
#[derive(Debug, Clone)]
pub struct RuffCheck {
    binary: String,
}

impl RuffCheck {
    /// Build a new lint provider using the given binary.
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }
}

impl Default for RuffCheck {
    fn default() -> Self {
        Self::new("ruff")
    }
}

impl LintProvider for RuffCheck {
    fn lint_project(&self, root: &Path) -> Result<Vec<Diagnostic>, LintError> {
        let output = run_command(
            "ruff",
            &self.binary,
            &["check", "--output-format=json", "."],
            Some(root),
            None,
        )?;
        // `ruff check` exits 1 when issues are found; that's not a tool
        // failure.
        let stdout = stdout_string("ruff", &output)?;
        if stdout.trim().is_empty() {
            return Ok(Vec::new());
        }
        parse_ruff_output(&stdout, Some(root))
    }

    fn lint_file(&self, path: &Path, source: &str) -> Result<Vec<Diagnostic>, LintError> {
        let path_str = path.display().to_string();
        let output = run_command(
            "ruff",
            &self.binary,
            &[
                "check",
                "--output-format=json",
                "--stdin-filename",
                &path_str,
                "-",
            ],
            None,
            Some(source),
        )?;
        let stdout = stdout_string("ruff", &output)?;
        if stdout.trim().is_empty() {
            return Ok(Vec::new());
        }
        parse_ruff_output(&stdout, None)
    }

    fn name(&self) -> &'static str {
        "ruff"
    }
}

#[derive(Debug, Deserialize)]
struct RuffEntry {
    code: Option<String>,
    message: String,
    location: RuffLoc,
    end_location: Option<RuffLoc>,
    filename: String,
}

#[derive(Debug, Deserialize)]
struct RuffLoc {
    row: u32,
    column: u32,
}

fn parse_ruff_output(stdout: &str, root: Option<&Path>) -> Result<Vec<Diagnostic>, LintError> {
    let entries: Vec<RuffEntry> =
        serde_json::from_str(stdout).map_err(|e| LintError::ParseError {
            tool: "ruff".to_owned(),
            message: e.to_string(),
        })?;
    Ok(entries
        .into_iter()
        .map(|e| {
            let path = {
                let p = PathBuf::from(&e.filename);
                if p.is_absolute() {
                    p
                } else if let Some(r) = root {
                    r.join(p)
                } else {
                    p
                }
            };
            let start = Position::new(
                e.location.row.saturating_sub(1),
                e.location.column.saturating_sub(1),
            );
            let end = e
                .end_location
                .map(|el| Position::new(el.row.saturating_sub(1), el.column.saturating_sub(1)))
                .unwrap_or(start);
            // Ruff doesn't carry severity; treat all findings as warnings.
            Diagnostic {
                path,
                range: Range { start, end },
                severity: Severity::Warning,
                message: e.message,
                code: e.code,
                source: "ruff".to_owned(),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_ruff_json() {
        let json = r#"[
            {"code":"E501","message":"line too long","location":{"row":3,"column":1},
             "end_location":{"row":3,"column":80},"filename":"src/foo.py"}
        ]"#;
        let diags = parse_ruff_output(json, Some(Path::new("/tmp/proj"))).expect("parse ok");
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.path, PathBuf::from("/tmp/proj/src/foo.py"));
        assert_eq!(d.range.start.line, 2, "1-indexed → 0-indexed");
        assert_eq!(d.range.start.column, 0);
        assert_eq!(d.range.end.column, 79);
        assert_eq!(d.code.as_deref(), Some("E501"));
        assert_eq!(d.source, "ruff");
        assert_eq!(d.severity, Severity::Warning);
    }

    #[test]
    fn empty_array_yields_no_diagnostics() {
        let diags = parse_ruff_output("[]", None).expect("parse ok");
        assert!(diags.is_empty());
    }

    #[test]
    fn missing_end_location_collapses_to_point() {
        let json = r#"[{"code":"F401","message":"unused","location":{"row":1,"column":5},"filename":"a.py"}]"#;
        let diags = parse_ruff_output(json, None).expect("parse ok");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].range.start, diags[0].range.end);
    }

    #[test]
    fn malformed_json_returns_parse_error() {
        let err = parse_ruff_output("not json", None).expect_err("should fail");
        assert!(matches!(err, LintError::ParseError { .. }));
    }
}
