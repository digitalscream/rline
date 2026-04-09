//! Tool: ask the user a follow-up question.
//!
//! This tool does not perform I/O itself — the agent loop intercepts it
//! and routes the question to the UI for user input.

use std::path::Path;

use serde::Deserialize;

use crate::chat::types::ToolDefinition;
use crate::error::AiError;
use crate::tools::{Tool, ToolCategory, ToolResult};

/// Ask the user a clarifying question. The agent loop handles the actual
/// UI interaction; this tool just validates the arguments.
pub struct AskFollowupQuestionTool;

#[derive(Debug, Deserialize)]
struct Args {
    question: String,
}

impl Tool for AskFollowupQuestionTool {
    fn name(&self) -> &str {
        "ask_followup_question"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "ask_followup_question",
            "Ask the user a question to gather more information needed to complete the task. \
             Use this when you need clarification or additional context from the user.",
            super::definitions::schema! {
                required: ["question"],
                properties: {
                    "question" => serde_json::json!({ "type": "string", "description": "The question to ask the user" })
                }
            },
        )
    }

    fn execute(&self, arguments: &str, _workspace_root: &Path) -> Result<ToolResult, AiError> {
        // Validate the arguments parse correctly. The actual question display
        // is handled by the agent loop, which intercepts this tool.
        let args: Args = serde_json::from_str(arguments)?;
        Ok(ToolResult::ok(args.question))
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Interactive
    }
}
