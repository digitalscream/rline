//! JavaScript / TypeScript support: `prettier` for formatting and `eslint`
//! for linting.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::diagnostic::{Diagnostic, Position, Range, Severity};
use crate::error::LintError;
use crate::provider::{Formatter, LintProvider};
use crate::util::{run_command, stdout_string, tool_failed};

/// `prettier` formatter.
#[derive(Debug, Clone)]
pub struct Prettier {
    binary: String,
}

impl Prettier {
    /// Build a new formatter using the given binary (typically `"prettier"`).
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }
}

impl Default for Prettier {
    fn default() -> Self {
        Self::new("prettier")
    }
}

impl Formatter for Prettier {
    fn format(&self, source: &str, path: &Path) -> Result<String, LintError> {
        let path_str = path.display().to_string();
        let output = run_command(
            "prettier",
            &self.binary,
            &["--stdin-filepath", &path_str],
            None,
            Some(source),
        )?;
        if !output.status.success() {
            return Err(tool_failed("prettier", &output));
        }
        stdout_string("prettier", &output)
    }

    fn name(&self) -> &'static str {
        "prettier"
    }
}

/// `eslint` lint provider.
#[derive(Debug, Clone)]
pub struct Eslint {
    binary: String,
}

impl Eslint {
    /// Build a new lint provider using the given binary.
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }
}

impl Default for Eslint {
    fn default() -> Self {
        Self::new("eslint")
    }
}

impl LintProvider for Eslint {
    fn lint_project(&self, root: &Path) -> Result<Vec<Diagnostic>, LintError> {
        let output = run_command(
            "eslint",
            &self.binary,
            &["--format", "json", "."],
            Some(root),
            None,
        )?;
        let stdout = stdout_string("eslint", &output)?;
        if stdout.trim().is_empty() {
            return Ok(Vec::new());
        }
        parse_eslint_output(&stdout, Some(root))
    }

    fn lint_file(&self, path: &Path, source: &str) -> Result<Vec<Diagnostic>, LintError> {
        let path_str = path.display().to_string();
        let output = run_command(
            "eslint",
            &self.binary,
            &["--format", "json", "--stdin", "--stdin-filename", &path_str],
            None,
            Some(source),
        )?;
        let stdout = stdout_string("eslint", &output)?;
        if stdout.trim().is_empty() {
            return Ok(Vec::new());
        }
        parse_eslint_output(&stdout, None)
    }

    fn name(&self) -> &'static str {
        "eslint"
    }
}

#[derive(Debug, Deserialize)]
struct EslintFile {
    #[serde(rename = "filePath")]
    file_path: String,
    messages: Vec<EslintMessage>,
}

#[derive(Debug, Deserialize)]
struct EslintMessage {
    #[serde(rename = "ruleId")]
    rule_id: Option<String>,
    severity: u8,
    message: String,
    line: Option<u32>,
    column: Option<u32>,
    #[serde(rename = "endLine")]
    end_line: Option<u32>,
    #[serde(rename = "endColumn")]
    end_column: Option<u32>,
}

fn parse_eslint_output(stdout: &str, root: Option<&Path>) -> Result<Vec<Diagnostic>, LintError> {
    let files: Vec<EslintFile> =
        serde_json::from_str(stdout).map_err(|e| LintError::ParseError {
            tool: "eslint".to_owned(),
            message: e.to_string(),
        })?;
    let mut diagnostics = Vec::new();
    for file in files {
        let path = {
            let p = PathBuf::from(&file.file_path);
            if p.is_absolute() {
                p
            } else if let Some(r) = root {
                r.join(p)
            } else {
                p
            }
        };
        for m in file.messages {
            let line = m.line.unwrap_or(1).saturating_sub(1);
            let column = m.column.unwrap_or(1).saturating_sub(1);
            let end_line = m.end_line.unwrap_or(m.line.unwrap_or(1)).saturating_sub(1);
            let end_column = m
                .end_column
                .unwrap_or(m.column.unwrap_or(1))
                .saturating_sub(1);
            let severity = match m.severity {
                2 => Severity::Error,
                1 => Severity::Warning,
                _ => Severity::Info,
            };
            diagnostics.push(Diagnostic {
                path: path.clone(),
                range: Range {
                    start: Position::new(line, column),
                    end: Position::new(end_line, end_column),
                },
                severity,
                message: m.message,
                code: m.rule_id,
                source: "eslint".to_owned(),
            });
        }
    }
    Ok(diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_eslint_json() {
        let json = r#"[
            {"filePath":"/abs/foo.js","messages":[
                {"ruleId":"no-unused-vars","severity":2,"message":"x is defined but never used",
                 "line":3,"column":7,"endLine":3,"endColumn":8}
            ]}
        ]"#;
        let diags = parse_eslint_output(json, None).expect("parse ok");
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.path, PathBuf::from("/abs/foo.js"));
        assert_eq!(d.range.start.line, 2);
        assert_eq!(d.range.start.column, 6);
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code.as_deref(), Some("no-unused-vars"));
    }

    #[test]
    fn empty_eslint_array() {
        let diags = parse_eslint_output("[]", None).expect("parse ok");
        assert!(diags.is_empty());
    }

    #[test]
    fn warning_severity_maps_correctly() {
        let json = r#"[{"filePath":"a.js","messages":[
            {"ruleId":null,"severity":1,"message":"hmm","line":1,"column":1}
        ]}]"#;
        let diags = parse_eslint_output(json, Some(Path::new("/r"))).expect("parse ok");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].path, PathBuf::from("/r/a.js"));
        assert!(diags[0].code.is_none());
    }

    #[test]
    fn malformed_json_errors() {
        let err = parse_eslint_output("not json", None).expect_err("should fail");
        assert!(matches!(err, LintError::ParseError { .. }));
    }
}
