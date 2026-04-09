//! Tool: signal that the agent has completed the task.
//!
//! This tool does not perform I/O itself — the agent loop intercepts it
//! to end the current task run.

use std::path::Path;

use serde::Deserialize;

use crate::chat::types::ToolDefinition;
use crate::error::AiError;
use crate::tools::{Tool, ToolCategory, ToolResult};

/// Signal that the current task is complete. The agent loop handles
/// the actual completion logic.
pub struct AttemptCompletionTool;

#[derive(Debug, Deserialize)]
struct Args {
    result: String,
    #[serde(default)]
    command: Option<String>,
}

impl Tool for AttemptCompletionTool {
    fn name(&self) -> &str {
        "attempt_completion"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "attempt_completion",
            "Signal that you have completed the user's task. Provide a summary of what was done. \
             Optionally include a command the user can run to verify the result.",
            super::definitions::schema! {
                required: ["result"],
                properties: {
                    "result" => serde_json::json!({ "type": "string", "description": "Summary of what was accomplished" }),
                    "command" => serde_json::json!({ "type": "string", "description": "Optional command to verify the result" })
                }
            },
        )
    }

    fn execute(&self, arguments: &str, _workspace_root: &Path) -> Result<ToolResult, AiError> {
        let args: Args = serde_json::from_str(arguments)?;
        let mut output = args.result;
        if let Some(cmd) = args.command {
            output.push_str(&format!("\n\nVerification command: {cmd}"));
        }
        Ok(ToolResult::ok(output))
    }

    fn is_read_only(&self) -> bool {
        false // Not available in Plan mode — nothing has been executed yet.
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Interactive
    }
}
