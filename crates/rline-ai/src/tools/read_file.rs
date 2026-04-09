//! Tool: read the contents of a file.

use std::path::Path;

use serde::Deserialize;

use crate::chat::types::ToolDefinition;
use crate::error::AiError;
use crate::tools::{Tool, ToolCategory, ToolResult};

/// Read the contents of a file, optionally restricted to a line range.
pub struct ReadFileTool;

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
    #[serde(default)]
    start_line: Option<usize>,
    #[serde(default)]
    end_line: Option<usize>,
}

impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "read_file",
            "Read the contents of a file at the specified path. Use start_line and end_line to read only a portion of the file.",
            super::definitions::schema! {
                required: ["path"],
                properties: {
                    "path" => serde_json::json!({ "type": "string", "description": "The path of the file to read (relative to the workspace root)" }),
                    "start_line" => serde_json::json!({ "type": "integer", "description": "The 1-based line number to start reading from (inclusive)" }),
                    "end_line" => serde_json::json!({ "type": "integer", "description": "The 1-based line number to stop reading at (inclusive)" })
                }
            },
        )
    }

    fn execute(&self, arguments: &str, workspace_root: &Path) -> Result<ToolResult, AiError> {
        let args: Args = serde_json::from_str(arguments)?;
        let file_path = resolve_path(&args.path, workspace_root);

        let contents = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::err(format!("Failed to read file: {e}"))),
        };

        let output = match (args.start_line, args.end_line) {
            (Some(start), Some(end)) => {
                let lines: Vec<&str> = contents.lines().collect();
                let start = start.saturating_sub(1); // Convert to 0-based.
                let end = end.min(lines.len());
                lines[start..end]
                    .iter()
                    .enumerate()
                    .map(|(i, l)| format!("{}\t{l}", start + i + 1))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            (Some(start), None) => {
                let lines: Vec<&str> = contents.lines().collect();
                let start = start.saturating_sub(1);
                lines[start..]
                    .iter()
                    .enumerate()
                    .map(|(i, l)| format!("{}\t{l}", start + i + 1))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            _ => contents,
        };

        Ok(ToolResult::ok(output))
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadFile
    }
}

/// Resolve a potentially relative path against the workspace root.
pub(crate) fn resolve_path(path: &str, workspace_root: &Path) -> std::path::PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        workspace_root.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_read_file_full() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        let file_path = dir.path().join("test.txt");
        let mut f = std::fs::File::create(&file_path).expect("create file in test");
        write!(f, "line1\nline2\nline3").expect("write in test");

        let tool = ReadFileTool;
        let args = serde_json::json!({ "path": "test.txt" }).to_string();
        let result = tool.execute(&args, dir.path()).expect("should execute");
        assert!(result.success);
        assert!(result.output.contains("line1"));
        assert!(result.output.contains("line3"));
    }

    #[test]
    fn test_read_file_range() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        let file_path = dir.path().join("test.txt");
        let mut f = std::fs::File::create(&file_path).expect("create file in test");
        write!(f, "line1\nline2\nline3\nline4").expect("write in test");

        let tool = ReadFileTool;
        let args =
            serde_json::json!({ "path": "test.txt", "start_line": 2, "end_line": 3 }).to_string();
        let result = tool.execute(&args, dir.path()).expect("should execute");
        assert!(result.success);
        assert!(result.output.contains("line2"));
        assert!(result.output.contains("line3"));
        assert!(!result.output.contains("line1"));
        assert!(!result.output.contains("line4"));
    }

    #[test]
    fn test_read_file_not_found() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        let tool = ReadFileTool;
        let args = serde_json::json!({ "path": "nonexistent.txt" }).to_string();
        let result = tool.execute(&args, dir.path()).expect("should execute");
        assert!(!result.success);
    }
}
