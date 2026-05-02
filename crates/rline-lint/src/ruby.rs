//! Ruby support: `rubocop` covers both formatting and linting.
//!
//! The same binary backs both [`Rubocop`] (lint) and [`RubocopFormat`]
//! (format) — they invoke it with different arguments.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::diagnostic::{Diagnostic, Position, Range, Severity};
use crate::error::LintError;
use crate::provider::{Formatter, LintProvider};
use crate::util::{run_command, stdout_string, tool_failed};

/// `rubocop -A` formatter (autocorrect-all).
#[derive(Debug, Clone)]
pub struct RubocopFormat {
    binary: String,
}

impl RubocopFormat {
    /// Build a new formatter using the given binary (typically `"rubocop"`).
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }
}

impl Default for RubocopFormat {
    fn default() -> Self {
        Self::new("rubocop")
    }
}

impl Formatter for RubocopFormat {
    fn format(&self, source: &str, path: &Path) -> Result<String, LintError> {
        // `rubocop --stdin <path> -A` runs autocorrect on stdin and emits the
        // lint report followed by a `=` separator line and the corrected
        // source. We strip everything up to and including the separator.
        let path_str = path.display().to_string();
        let resolved = resolve_binary(&self.binary, path.parent());
        let output = run_command(
            "rubocop",
            &resolved,
            &["--stdin", &path_str, "-A", "--format=quiet"],
            None,
            Some(source),
        )?;
        // Rubocop exits 1 when offences remain even after autocorrect; that's
        // not a tool failure. Treat any spawn-level failure as fatal.
        if let Some(code) = output.status.code() {
            if code != 0 && code != 1 {
                return Err(tool_failed("rubocop", &output));
            }
        } else {
            return Err(tool_failed("rubocop", &output));
        }
        let stdout = stdout_string("rubocop", &output)?;
        extract_corrected_source(&stdout)
    }

    fn name(&self) -> &'static str {
        "rubocop -A"
    }
}

/// `rubocop` lint provider.
#[derive(Debug, Clone)]
pub struct Rubocop {
    binary: String,
}

impl Rubocop {
    /// Build a new lint provider using the given binary.
    pub fn new(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }
}

impl Default for Rubocop {
    fn default() -> Self {
        Self::new("rubocop")
    }
}

impl LintProvider for Rubocop {
    fn lint_project(&self, root: &Path) -> Result<Vec<Diagnostic>, LintError> {
        let resolved = resolve_binary(&self.binary, Some(root));
        let output = run_command(
            "rubocop",
            &resolved,
            &["--format", "json"],
            Some(root),
            None,
        )?;
        let stdout = stdout_string("rubocop", &output)?;
        if stdout.trim().is_empty() {
            return Ok(Vec::new());
        }
        parse_rubocop_output(&stdout, Some(root))
    }

    fn lint_file(&self, path: &Path, source: &str) -> Result<Vec<Diagnostic>, LintError> {
        let path_str = path.display().to_string();
        let resolved = resolve_binary(&self.binary, path.parent());
        let output = run_command(
            "rubocop",
            &resolved,
            &["--stdin", &path_str, "--format", "json"],
            None,
            Some(source),
        )?;
        let stdout = stdout_string("rubocop", &output)?;
        // When `--stdin` is used, rubocop appends the (unchanged) source after
        // a `=` separator. Strip that before JSON parsing.
        let json_part = strip_trailing_source(&stdout);
        if json_part.trim().is_empty() {
            return Ok(Vec::new());
        }
        parse_rubocop_output(json_part, None)
    }

    fn name(&self) -> &'static str {
        "rubocop"
    }
}

/// Resolve the rubocop binary against a starting directory.
///
/// If `binary` is a bare command name (no path separators — e.g. `"rubocop"`),
/// walk up from `start` looking for a `bin/<binary>` file. This matches the
/// Bundler binstub convention (`bundle binstubs rubocop` writes
/// `bin/rubocop`). The first match wins; otherwise the original binary is
/// returned and PATH lookup proceeds normally.
///
/// If `binary` already contains a path separator the caller has been explicit
/// — return it untouched.
fn resolve_binary(binary: &str, start: Option<&Path>) -> String {
    if binary.contains(std::path::MAIN_SEPARATOR) || binary.contains('/') {
        return binary.to_owned();
    }
    let Some(start) = start else {
        return binary.to_owned();
    };
    for ancestor in start.ancestors() {
        let candidate = ancestor.join("bin").join(binary);
        if candidate.is_file() {
            return candidate.display().to_string();
        }
    }
    binary.to_owned()
}

/// Rubocop's stdin separator. A run of `=` characters on a line of its own
/// (rubocop emits 20 of them).
const SEPARATOR_PREFIX: &str = "====================";

/// When rubocop is invoked with `--stdin`, its stdout is structured as:
///
/// ```text
/// <lint report or JSON>
/// ====================
/// <corrected source>
/// ```
///
/// This helper extracts the corrected source by finding the separator line
/// and returning everything after it.
fn extract_corrected_source(stdout: &str) -> Result<String, LintError> {
    for (idx, line) in stdout.lines().enumerate() {
        if line.starts_with(SEPARATOR_PREFIX) {
            // Reconstruct the tail. Walking by line lets us preserve the
            // exact line endings rubocop emitted.
            let consumed: usize = stdout
                .lines()
                .take(idx + 1)
                .map(|l| l.len() + 1) // +1 for '\n'
                .sum();
            if consumed >= stdout.len() {
                return Ok(String::new());
            }
            return Ok(stdout[consumed..].to_owned());
        }
    }
    Err(LintError::ParseError {
        tool: "rubocop".to_owned(),
        message: "no '=' separator in --stdin output".to_owned(),
    })
}

/// Inverse of [`extract_corrected_source`]: returns everything *before* the
/// separator (the JSON report) and discards the trailing source.
fn strip_trailing_source(stdout: &str) -> &str {
    if let Some(idx) = stdout.find(SEPARATOR_PREFIX) {
        // Walk back to the start of that line.
        let prefix = &stdout[..idx];
        prefix.trim_end()
    } else {
        stdout
    }
}

#[derive(Debug, Deserialize)]
struct RubocopReport {
    files: Vec<RubocopFile>,
}

#[derive(Debug, Deserialize)]
struct RubocopFile {
    path: String,
    offenses: Vec<RubocopOffense>,
}

#[derive(Debug, Deserialize)]
struct RubocopOffense {
    severity: String,
    message: String,
    cop_name: String,
    location: RubocopLocation,
}

#[derive(Debug, Deserialize)]
struct RubocopLocation {
    start_line: u32,
    start_column: u32,
    last_line: u32,
    last_column: u32,
}

fn parse_rubocop_output(stdout: &str, root: Option<&Path>) -> Result<Vec<Diagnostic>, LintError> {
    let report: RubocopReport =
        serde_json::from_str(stdout).map_err(|e| LintError::ParseError {
            tool: "rubocop".to_owned(),
            message: e.to_string(),
        })?;
    let mut diagnostics = Vec::new();
    for file in report.files {
        let path = {
            let p = PathBuf::from(&file.path);
            if p.is_absolute() {
                p
            } else if let Some(r) = root {
                r.join(p)
            } else {
                p
            }
        };
        for off in file.offenses {
            let severity = match off.severity.as_str() {
                "fatal" | "error" => Severity::Error,
                "warning" => Severity::Warning,
                "convention" | "refactor" => Severity::Info,
                _ => Severity::Hint,
            };
            diagnostics.push(Diagnostic {
                path: path.clone(),
                range: Range {
                    start: Position::new(
                        off.location.start_line.saturating_sub(1),
                        off.location.start_column.saturating_sub(1),
                    ),
                    end: Position::new(
                        off.location.last_line.saturating_sub(1),
                        off.location.last_column.saturating_sub(1),
                    ),
                },
                severity,
                message: off.message,
                code: Some(off.cop_name),
                source: "rubocop".to_owned(),
            });
        }
    }
    Ok(diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_source_after_separator() {
        let stdout = "Inspecting 1 file\nC\n\n====================\ndef foo\n  1\nend\n";
        let extracted = extract_corrected_source(stdout).expect("found separator");
        assert_eq!(extracted, "def foo\n  1\nend\n");
    }

    #[test]
    fn no_separator_is_a_parse_error() {
        let err = extract_corrected_source("no separator here").expect_err("should fail");
        assert!(matches!(err, LintError::ParseError { .. }));
    }

    #[test]
    fn strip_trailing_keeps_only_report() {
        let s = "{\"files\":[]}\n====================\nsource here\n";
        assert_eq!(strip_trailing_source(s), "{\"files\":[]}");
    }

    #[test]
    fn strip_trailing_no_separator_returns_input() {
        let s = "{\"files\":[]}";
        assert_eq!(strip_trailing_source(s), s);
    }

    #[test]
    fn parses_rubocop_json() {
        let json = r#"{
            "files":[{
                "path":"lib/foo.rb",
                "offenses":[{
                    "severity":"convention",
                    "message":"Use 2 (not 4) spaces for indentation.",
                    "cop_name":"Layout/IndentationWidth",
                    "location":{"start_line":3,"start_column":1,"last_line":3,"last_column":4}
                }]
            }],
            "summary":{}
        }"#;
        let diags = parse_rubocop_output(json, Some(Path::new("/proj"))).expect("parse ok");
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.path, PathBuf::from("/proj/lib/foo.rb"));
        assert_eq!(d.severity, Severity::Info, "convention → Info");
        assert_eq!(d.code.as_deref(), Some("Layout/IndentationWidth"));
        assert_eq!(d.range.start.line, 2);
    }

    #[test]
    fn rubocop_severity_mapping() {
        let json = r#"{
            "files":[{"path":"a.rb","offenses":[
                {"severity":"error","message":"boom","cop_name":"Lint/Syntax",
                 "location":{"start_line":1,"start_column":1,"last_line":1,"last_column":1}},
                {"severity":"warning","message":"hmm","cop_name":"Lint/X",
                 "location":{"start_line":2,"start_column":1,"last_line":2,"last_column":1}},
                {"severity":"refactor","message":"meh","cop_name":"Style/X",
                 "location":{"start_line":3,"start_column":1,"last_line":3,"last_column":1}}
            ]}],
            "summary":{}
        }"#;
        let diags = parse_rubocop_output(json, None).expect("parse ok");
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[1].severity, Severity::Warning);
        assert_eq!(diags[2].severity, Severity::Info);
    }

    #[test]
    fn resolve_binary_prefers_project_binstub() {
        let root = std::env::temp_dir().join(format!(
            "rline-rubocop-test-{}-{}",
            std::process::id(),
            "binstub"
        ));
        let _ = std::fs::remove_dir_all(&root);
        let bin_dir = root.join("bin");
        std::fs::create_dir_all(&bin_dir).expect("mkdir bin");
        let stub = bin_dir.join("rubocop");
        std::fs::write(&stub, "#!/bin/sh\n").expect("write stub");
        let nested = root.join("lib/sub");
        std::fs::create_dir_all(&nested).expect("mkdir nested");

        let resolved = resolve_binary("rubocop", Some(&nested));
        assert_eq!(resolved, stub.display().to_string());

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn resolve_binary_respects_explicit_path() {
        let resolved = resolve_binary("/usr/bin/rubocop", Some(Path::new("/tmp")));
        assert_eq!(resolved, "/usr/bin/rubocop");
    }

    #[test]
    fn empty_files_array_yields_no_diagnostics() {
        let json = r#"{"files":[],"summary":{}}"#;
        let diags = parse_rubocop_output(json, None).expect("parse ok");
        assert!(diags.is_empty());
    }
}
