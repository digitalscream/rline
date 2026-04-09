//! Tool: edit an existing file using search/replace blocks.

use std::path::Path;

use serde::Deserialize;

use crate::chat::types::ToolDefinition;
use crate::error::AiError;
use crate::tools::{Tool, ToolCategory, ToolResult};

/// Edit a file by applying one or more SEARCH/REPLACE blocks.
pub struct ReplaceInFileTool;

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
    diff: String,
}

/// A single search/replace operation parsed from the diff.
struct Replacement {
    search: String,
    replace: String,
}

impl Tool for ReplaceInFileTool {
    fn name(&self) -> &str {
        "replace_in_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "replace_in_file",
            "Edit an existing file using SEARCH/REPLACE blocks. Each block specifies exact text to \
             find and the replacement. Use the format:\n\
             <<<<<<< SEARCH\n\
             exact text to find\n\
             =======\n\
             replacement text\n\
             >>>>>>> REPLACE\n\n\
             Multiple blocks can be provided to make several changes in one call.",
            super::definitions::schema! {
                required: ["path", "diff"],
                properties: {
                    "path" => serde_json::json!({ "type": "string", "description": "The path of the file to edit (relative to workspace root)" }),
                    "diff" => serde_json::json!({ "type": "string", "description": "One or more SEARCH/REPLACE blocks" })
                }
            },
        )
    }

    fn execute(&self, arguments: &str, workspace_root: &Path) -> Result<ToolResult, AiError> {
        let args: Args = serde_json::from_str(arguments)?;
        let file_path = super::read_file::resolve_path(&args.path, workspace_root);

        let original = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::err(format!("Failed to read file: {e}"))),
        };

        let replacements = match parse_replacements(&args.diff) {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::err(format!("Failed to parse diff: {e}"))),
        };

        if replacements.is_empty() {
            return Ok(ToolResult::err(
                "No SEARCH/REPLACE blocks found in diff".to_owned(),
            ));
        }

        let mut content = original;
        let mut applied = 0;

        for replacement in &replacements {
            if let Some(pos) = content.find(&replacement.search) {
                content = format!(
                    "{}{}{}",
                    &content[..pos],
                    replacement.replace,
                    &content[pos + replacement.search.len()..]
                );
                applied += 1;
            } else {
                return Ok(ToolResult::err(format!(
                    "Could not find search text in file (replacement {}/{}). \
                     Make sure the SEARCH block matches the file content exactly, \
                     including whitespace and indentation.",
                    applied + 1,
                    replacements.len()
                )));
            }
        }

        match std::fs::write(&file_path, &content) {
            Ok(()) => Ok(ToolResult::ok(format!(
                "Applied {applied}/{} replacement(s) to {}",
                replacements.len(),
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

/// Parse SEARCH/REPLACE blocks from a diff string.
fn parse_replacements(diff: &str) -> Result<Vec<Replacement>, String> {
    let mut replacements = Vec::new();
    let mut remaining = diff;

    while let Some(search_start) = remaining.find("<<<<<<< SEARCH") {
        let after_marker = &remaining[search_start + "<<<<<<< SEARCH".len()..];
        // Skip the newline after the marker.
        let after_marker = after_marker.strip_prefix('\n').unwrap_or(after_marker);

        let separator = after_marker
            .find("=======")
            .ok_or("Missing ======= separator")?;
        let search = &after_marker[..separator];
        let search = search.strip_suffix('\n').unwrap_or(search);

        let after_sep = &after_marker[separator + "=======".len()..];
        let after_sep = after_sep.strip_prefix('\n').unwrap_or(after_sep);

        let end = after_sep
            .find(">>>>>>> REPLACE")
            .ok_or("Missing >>>>>>> REPLACE marker")?;
        let replace = &after_sep[..end];
        let replace = replace.strip_suffix('\n').unwrap_or(replace);

        replacements.push(Replacement {
            search: search.to_owned(),
            replace: replace.to_owned(),
        });

        remaining = &after_sep[end + ">>>>>>> REPLACE".len()..];
    }

    Ok(replacements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_replace_in_file_single_block() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        let file_path = dir.path().join("test.rs");
        {
            let mut f = std::fs::File::create(&file_path).expect("create in test");
            write!(f, "fn main() {{\n    println!(\"hello\");\n}}").expect("write in test");
        }

        let tool = ReplaceInFileTool;
        let diff = "<<<<<<< SEARCH\n    println!(\"hello\");\n=======\n    println!(\"world\");\n>>>>>>> REPLACE";
        let args = serde_json::json!({ "path": "test.rs", "diff": diff }).to_string();

        let result = tool.execute(&args, dir.path()).expect("should execute");
        assert!(result.success, "result: {}", result.output);

        let content = std::fs::read_to_string(&file_path).expect("read in test");
        assert!(content.contains("println!(\"world\")"));
        assert!(!content.contains("println!(\"hello\")"));
    }

    #[test]
    fn test_replace_in_file_no_match() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").expect("write in test");

        let tool = ReplaceInFileTool;
        let diff = "<<<<<<< SEARCH\nfoo\n=======\nbar\n>>>>>>> REPLACE";
        let args = serde_json::json!({ "path": "test.txt", "diff": diff }).to_string();

        let result = tool.execute(&args, dir.path()).expect("should execute");
        assert!(!result.success);
    }

    #[test]
    fn test_parse_replacements_multiple() {
        let diff = "\
<<<<<<< SEARCH
aaa
=======
bbb
>>>>>>> REPLACE
<<<<<<< SEARCH
ccc
=======
ddd
>>>>>>> REPLACE";
        let reps = parse_replacements(diff).expect("should parse");
        assert_eq!(reps.len(), 2);
        assert_eq!(reps[0].search, "aaa");
        assert_eq!(reps[0].replace, "bbb");
        assert_eq!(reps[1].search, "ccc");
        assert_eq!(reps[1].replace, "ddd");
    }
}
