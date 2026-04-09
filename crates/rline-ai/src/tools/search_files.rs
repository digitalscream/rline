//! Tool: search file contents by regex.

use std::path::Path;

use regex::Regex;
use serde::Deserialize;

use crate::chat::types::ToolDefinition;
use crate::error::AiError;
use crate::tools::{Tool, ToolCategory, ToolResult};

/// Search files in the workspace for a regex pattern.
pub struct SearchFilesTool;

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
    regex: String,
    #[serde(default = "default_max_results")]
    max_results: usize,
}

fn default_max_results() -> usize {
    100
}

/// Directories to skip during search.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "__pycache__",
    ".venv",
    "dist",
    "build",
];

/// Extensions considered binary (skip these files).
const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "svg", "woff", "woff2", "ttf", "otf", "eot",
    "zip", "tar", "gz", "bz2", "xz", "7z", "exe", "dll", "so", "dylib", "o", "a", "pdf", "doc",
    "docx", "ppt", "pptx", "mp3", "mp4", "avi", "mov", "wav",
];

impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "search_files",
            "Search for a regex pattern across files in the workspace. Returns matching lines grouped by file.",
            super::definitions::schema! {
                required: ["path", "regex"],
                properties: {
                    "path" => serde_json::json!({ "type": "string", "description": "Directory to search in (relative to workspace root)" }),
                    "regex" => serde_json::json!({ "type": "string", "description": "Regex pattern to search for" }),
                    "max_results" => serde_json::json!({ "type": "integer", "description": "Maximum number of matching lines to return (default: 100)" })
                }
            },
        )
    }

    fn execute(&self, arguments: &str, workspace_root: &Path) -> Result<ToolResult, AiError> {
        let args: Args = serde_json::from_str(arguments)?;
        let search_dir = super::read_file::resolve_path(&args.path, workspace_root);
        let pattern = Regex::new(&args.regex)?;

        let mut results = Vec::new();
        let mut total = 0;

        search_recursive(
            &search_dir,
            workspace_root,
            &pattern,
            args.max_results,
            &mut results,
            &mut total,
        );

        if results.is_empty() {
            return Ok(ToolResult::ok("No matches found.".to_owned()));
        }

        let mut output = String::new();
        let mut current_file = String::new();

        for (file, line_num, line) in &results {
            if *file != current_file {
                if !current_file.is_empty() {
                    output.push('\n');
                }
                output.push_str(file);
                output.push('\n');
                current_file.clone_from(file);
            }
            output.push_str(&format!("  {line_num}: {line}\n"));
        }

        if total > args.max_results {
            output.push_str(&format!("\n(showing {}/{total} matches)", args.max_results));
        }

        Ok(ToolResult::ok(output))
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadFile
    }
}

fn search_recursive(
    dir: &Path,
    workspace_root: &Path,
    pattern: &Regex,
    max_results: usize,
    results: &mut Vec<(String, usize, String)>,
    total: &mut usize,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if results.len() >= max_results {
            break;
        }

        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            if SKIP_DIRS.contains(&name_str.as_ref()) {
                continue;
            }
            search_recursive(&path, workspace_root, pattern, max_results, results, total);
        } else {
            // Skip binary files.
            if let Some(ext) = path.extension() {
                if BINARY_EXTENSIONS.contains(&ext.to_string_lossy().as_ref()) {
                    continue;
                }
            }

            let rel = path
                .strip_prefix(workspace_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue, // Skip files that can't be read as UTF-8.
            };

            for (i, line) in content.lines().enumerate() {
                if pattern.is_match(line) {
                    *total += 1;
                    if results.len() < max_results {
                        results.push((rel.clone(), i + 1, line.to_owned()));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_files_basic() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        std::fs::write(
            dir.path().join("a.txt"),
            "hello world\nfoo bar\nhello again",
        )
        .expect("write in test");
        std::fs::write(dir.path().join("b.txt"), "no match here").expect("write in test");

        let tool = SearchFilesTool;
        let args = serde_json::json!({ "path": ".", "regex": "hello" }).to_string();
        let result = tool.execute(&args, dir.path()).expect("should execute");
        assert!(result.success);
        assert!(result.output.contains("a.txt"));
        assert!(result.output.contains("hello world"));
        assert!(result.output.contains("hello again"));
        assert!(!result.output.contains("b.txt"));
    }

    #[test]
    fn test_search_files_no_match() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        std::fs::write(dir.path().join("a.txt"), "hello world").expect("write in test");

        let tool = SearchFilesTool;
        let args = serde_json::json!({ "path": ".", "regex": "zzz" }).to_string();
        let result = tool.execute(&args, dir.path()).expect("should execute");
        assert!(result.success);
        assert!(result.output.contains("No matches"));
    }
}
