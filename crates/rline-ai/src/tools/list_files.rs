//! Tool: list directory contents recursively.

use std::path::Path;

use serde::Deserialize;

use crate::chat::types::ToolDefinition;
use crate::error::AiError;
use crate::tools::{Tool, ToolCategory, ToolResult};

/// List files and directories recursively up to a configurable depth.
pub struct ListFilesTool;

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
    #[serde(default = "default_depth")]
    max_depth: usize,
}

fn default_depth() -> usize {
    3
}

/// Directories to skip during traversal.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "__pycache__",
    ".venv",
    "dist",
    "build",
];

impl Tool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "list_files",
            "List files and directories recursively at the given path. Results are indented to show hierarchy.",
            super::definitions::schema! {
                required: ["path"],
                properties: {
                    "path" => serde_json::json!({ "type": "string", "description": "Directory path to list (relative to workspace root)" }),
                    "max_depth" => serde_json::json!({ "type": "integer", "description": "Maximum directory depth to recurse (default: 3)" })
                }
            },
        )
    }

    fn execute(&self, arguments: &str, workspace_root: &Path) -> Result<ToolResult, AiError> {
        let args: Args = serde_json::from_str(arguments)?;
        let dir_path = super::read_file::resolve_path(&args.path, workspace_root);

        if !dir_path.is_dir() {
            return Ok(ToolResult::err(format!("{} is not a directory", args.path)));
        }

        let mut output = String::new();
        list_recursive(&dir_path, workspace_root, 0, args.max_depth, &mut output);

        if output.is_empty() {
            output = "(empty directory)".to_owned();
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

fn list_recursive(
    dir: &Path,
    _workspace_root: &Path,
    depth: usize,
    max_depth: usize,
    output: &mut String,
) {
    if depth > max_depth {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut items: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    items.sort_by_key(|e| e.file_name());

    let indent = "  ".repeat(depth);

    for entry in items {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden and ignored directories.
        if entry.path().is_dir() && SKIP_DIRS.contains(&name_str.as_ref()) {
            continue;
        }

        if entry.path().is_dir() {
            output.push_str(&format!("{indent}{name_str}/\n"));
            list_recursive(&entry.path(), _workspace_root, depth + 1, max_depth, output);
        } else {
            output.push_str(&format!("{indent}{name_str}\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_files_basic() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        std::fs::write(dir.path().join("a.txt"), "").expect("write in test");
        std::fs::create_dir(dir.path().join("sub")).expect("mkdir in test");
        std::fs::write(dir.path().join("sub/b.txt"), "").expect("write in test");

        let tool = ListFilesTool;
        let args = serde_json::json!({ "path": "." }).to_string();
        let result = tool.execute(&args, dir.path()).expect("should execute");
        assert!(result.success);
        assert!(result.output.contains("a.txt"));
        assert!(result.output.contains("sub/"));
        assert!(result.output.contains("b.txt"));
    }

    #[test]
    fn test_list_files_skips_git() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        std::fs::create_dir(dir.path().join(".git")).expect("mkdir in test");
        std::fs::write(dir.path().join("a.txt"), "").expect("write in test");

        let tool = ListFilesTool;
        let args = serde_json::json!({ "path": "." }).to_string();
        let result = tool.execute(&args, dir.path()).expect("should execute");
        assert!(result.success);
        assert!(!result.output.contains(".git"));
    }
}
