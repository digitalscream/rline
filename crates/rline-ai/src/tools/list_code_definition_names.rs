//! Tool: extract code structure (function/class/struct names) from files.

use std::path::Path;

use regex::Regex;
use serde::Deserialize;

use crate::chat::types::ToolDefinition;
use crate::error::AiError;
use crate::tools::{Tool, ToolCategory, ToolResult};

/// Extract top-level code definition names from a file using regex heuristics.
pub struct ListCodeDefinitionNamesTool;

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
}

impl Tool for ListCodeDefinitionNamesTool {
    fn name(&self) -> &str {
        "list_code_definition_names"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "list_code_definition_names",
            "List top-level code definitions (functions, classes, structs, interfaces, etc.) in a file. \
             Useful for understanding the structure of a file before reading it in full.",
            super::definitions::schema! {
                required: ["path"],
                properties: {
                    "path" => serde_json::json!({ "type": "string", "description": "File path to analyze (relative to workspace root)" })
                }
            },
        )
    }

    fn execute(&self, arguments: &str, workspace_root: &Path) -> Result<ToolResult, AiError> {
        let args: Args = serde_json::from_str(arguments)?;
        let file_path = super::read_file::resolve_path(&args.path, workspace_root);

        let content = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::err(format!("Failed to read file: {e}"))),
        };

        let definitions = extract_definitions(&content);

        if definitions.is_empty() {
            return Ok(ToolResult::ok("No code definitions found.".to_owned()));
        }

        let output = definitions
            .iter()
            .map(|(line, kind, name)| format!("{line}: [{kind}] {name}"))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(ToolResult::ok(output))
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadFile
    }
}

/// Extract definitions from source code using regex patterns.
///
/// Returns tuples of (line_number, kind, name).
fn extract_definitions(content: &str) -> Vec<(usize, &'static str, String)> {
    let patterns: &[(&str, Regex)] = &[
        // Rust
        (
            "fn",
            Regex::new(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)").expect("valid regex"),
        ),
        (
            "struct",
            Regex::new(r"^\s*(?:pub\s+)?struct\s+(\w+)").expect("valid regex"),
        ),
        (
            "enum",
            Regex::new(r"^\s*(?:pub\s+)?enum\s+(\w+)").expect("valid regex"),
        ),
        (
            "trait",
            Regex::new(r"^\s*(?:pub\s+)?trait\s+(\w+)").expect("valid regex"),
        ),
        (
            "impl",
            Regex::new(r"^\s*impl(?:<[^>]*>)?\s+(\w+)").expect("valid regex"),
        ),
        (
            "mod",
            Regex::new(r"^\s*(?:pub\s+)?mod\s+(\w+)").expect("valid regex"),
        ),
        // Python
        ("class", Regex::new(r"^class\s+(\w+)").expect("valid regex")),
        (
            "def",
            Regex::new(r"^(?:async\s+)?def\s+(\w+)").expect("valid regex"),
        ),
        // JavaScript/TypeScript
        (
            "function",
            Regex::new(r"^(?:export\s+)?(?:async\s+)?function\s+(\w+)").expect("valid regex"),
        ),
        (
            "class",
            Regex::new(r"^(?:export\s+)?class\s+(\w+)").expect("valid regex"),
        ),
        (
            "interface",
            Regex::new(r"^(?:export\s+)?interface\s+(\w+)").expect("valid regex"),
        ),
        (
            "type",
            Regex::new(r"^(?:export\s+)?type\s+(\w+)").expect("valid regex"),
        ),
        // Go
        (
            "func",
            Regex::new(r"^func\s+(?:\([^)]+\)\s+)?(\w+)").expect("valid regex"),
        ),
        (
            "type",
            Regex::new(r"^type\s+(\w+)\s+struct").expect("valid regex"),
        ),
        // C/C++
        (
            "class",
            Regex::new(r"^(?:class|struct)\s+(\w+)\s*[{:]").expect("valid regex"),
        ),
    ];

    let mut results = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        for (kind, pattern) in patterns {
            if let Some(caps) = pattern.captures(line) {
                if let Some(name) = caps.get(1) {
                    results.push((line_num + 1, *kind, name.as_str().to_owned()));
                    break; // Only match one pattern per line.
                }
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_definitions_rust() {
        let content = "\
pub struct Foo {
    bar: i32,
}

impl Foo {
    pub fn new() -> Self {
        Self { bar: 0 }
    }

    pub async fn process(&self) {}
}

enum Color {
    Red,
    Blue,
}";
        let defs = extract_definitions(content);
        let names: Vec<&str> = defs.iter().map(|(_, _, n)| n.as_str()).collect();
        assert!(names.contains(&"Foo"), "should find struct Foo");
        assert!(names.contains(&"new"), "should find fn new");
        assert!(names.contains(&"process"), "should find fn process");
        assert!(names.contains(&"Color"), "should find enum Color");
    }
}
