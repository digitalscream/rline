//! Tool: create or overwrite a file.

use std::path::Path;

use serde::Deserialize;

use crate::chat::types::ToolDefinition;
use crate::error::AiError;
use crate::tools::{Tool, ToolCategory, ToolResult};

/// Create or overwrite a file with the given content.
pub struct WriteToFileTool;

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
    content: String,
}

impl Tool for WriteToFileTool {
    fn name(&self) -> &str {
        "write_to_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "write_to_file",
            "Create a new file or completely overwrite an existing file with the provided content.",
            super::definitions::schema! {
                required: ["path", "content"],
                properties: {
                    "path" => serde_json::json!({ "type": "string", "description": "The path of the file to write (relative to the workspace root)" }),
                    "content" => serde_json::json!({ "type": "string", "description": "The complete content to write to the file" })
                }
            },
        )
    }

    fn execute(&self, arguments: &str, workspace_root: &Path) -> Result<ToolResult, AiError> {
        let args: Args = serde_json::from_str(arguments)?;
        let file_path = super::read_file::resolve_path(&args.path, workspace_root);

        // Create parent directories if needed.
        if let Some(parent) = file_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Ok(ToolResult::err(format!(
                    "Failed to create directory {}: {e}",
                    parent.display()
                )));
            }
        }

        match std::fs::write(&file_path, &args.content) {
            Ok(()) => Ok(ToolResult::ok(format!(
                "Successfully wrote {} bytes to {}",
                args.content.len(),
                args.path
            ))),
            Err(e) => Ok(ToolResult::err(format!("Failed to write file: {e}"))),
        }
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::EditFile
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_to_file_new() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        let tool = WriteToFileTool;
        let args = serde_json::json!({
            "path": "new_file.txt",
            "content": "hello world"
        })
        .to_string();

        let result = tool.execute(&args, dir.path()).expect("should execute");
        assert!(result.success);

        let content = std::fs::read_to_string(dir.path().join("new_file.txt"))
            .expect("file should exist in test");
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_write_to_file_creates_dirs() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        let tool = WriteToFileTool;
        let args = serde_json::json!({
            "path": "sub/dir/file.txt",
            "content": "nested"
        })
        .to_string();

        let result = tool.execute(&args, dir.path()).expect("should execute");
        assert!(result.success);
        assert!(dir.path().join("sub/dir/file.txt").exists());
    }
}
