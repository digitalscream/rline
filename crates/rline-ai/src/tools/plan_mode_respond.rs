//! Tool: present a plan or analysis in Plan mode.
//!
//! This is the Plan mode equivalent of `attempt_completion`. The agent
//! calls it to present its plan to the user, ending the current Plan
//! mode run. The user can then switch to Act mode to execute the plan.

use std::path::Path;

use serde::Deserialize;

use crate::chat::types::ToolDefinition;
use crate::error::AiError;
use crate::tools::{Tool, ToolCategory, ToolResult};

/// Present a plan or analysis to the user. Only available in Plan mode.
pub struct PlanModeRespondTool;

#[derive(Debug, Deserialize)]
struct Args {
    response: String,
}

impl Tool for PlanModeRespondTool {
    fn name(&self) -> &str {
        "plan_mode_respond"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "plan_mode_respond",
            "Present your plan, analysis, or response to the user. Use this when you have \
             finished analyzing the codebase and are ready to present your plan. This ends \
             the current planning session — the user will then switch to Act mode to execute.",
            super::definitions::schema! {
                required: ["response"],
                properties: {
                    "response" => serde_json::json!({ "type": "string", "description": "Your plan or analysis to present to the user" })
                }
            },
        )
    }

    fn execute(&self, arguments: &str, _workspace_root: &Path) -> Result<ToolResult, AiError> {
        let args: Args = serde_json::from_str(arguments)?;
        Ok(ToolResult::ok(args.response))
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Interactive
    }
}
