//! Tool definitions and execution for the AI agent.
//!
//! Each tool implements the [`Tool`] trait and is registered in a
//! [`ToolRegistry`]. The agent loop uses the registry to dispatch
//! tool calls from the AI model.

pub mod ask_followup_question;
pub mod attempt_completion;
pub mod definitions;
pub mod execute_command;
pub mod list_code_definition_names;
pub mod list_files;
pub mod plan_mode_respond;
pub mod read_file;
pub mod replace_in_file;
pub mod search_files;
pub mod write_to_file;

use std::path::Path;

use crate::chat::types::ToolDefinition;
use crate::error::AiError;

/// The result of executing a tool.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Whether the tool executed successfully.
    pub success: bool,
    /// The output to send back to the AI model.
    pub output: String,
}

impl ToolResult {
    /// Create a successful tool result.
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
        }
    }

    /// Create a failed tool result.
    pub fn err(output: impl Into<String>) -> Self {
        Self {
            success: false,
            output: output.into(),
        }
    }
}

/// Categories of tools for permission grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// Tools that only read files or list directories.
    ReadFile,
    /// Tools that create or modify files.
    EditFile,
    /// Tools that execute shell commands.
    ExecuteCommand,
    /// Tools that interact with the user (ask questions, completion).
    Interactive,
}

/// A tool that the AI agent can invoke.
pub trait Tool: Send + Sync {
    /// The tool name as sent to the API.
    fn name(&self) -> &str;

    /// The OpenAI function-calling tool definition.
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with the given JSON arguments string.
    ///
    /// `workspace_root` constrains filesystem access for security.
    fn execute(&self, arguments: &str, workspace_root: &Path) -> Result<ToolResult, AiError>;

    /// Whether this tool is read-only (safe for Plan mode).
    fn is_read_only(&self) -> bool;

    /// The permission category for this tool.
    fn category(&self) -> ToolCategory;
}

/// Registry of available tools.
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    /// Create a registry with all built-in tools.
    pub fn new() -> Self {
        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(read_file::ReadFileTool),
            Box::new(write_to_file::WriteToFileTool),
            Box::new(replace_in_file::ReplaceInFileTool),
            Box::new(list_files::ListFilesTool),
            Box::new(search_files::SearchFilesTool),
            Box::new(execute_command::ExecuteCommandTool::default()),
            Box::new(list_code_definition_names::ListCodeDefinitionNamesTool),
            Box::new(ask_followup_question::AskFollowupQuestionTool),
            Box::new(attempt_completion::AttemptCompletionTool),
            Box::new(plan_mode_respond::PlanModeRespondTool),
        ];
        Self { tools }
    }

    /// Get tool definitions filtered by mode.
    ///
    /// - Plan mode (`plan_mode=true`): read-only tools + `plan_mode_respond`
    /// - Act mode (`plan_mode=false`): all tools except `plan_mode_respond`
    pub fn definitions(&self, plan_mode: bool) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .filter(|t| {
                if plan_mode {
                    t.is_read_only()
                } else {
                    t.name() != "plan_mode_respond"
                }
            })
            .map(|t| t.definition())
            .collect()
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| &**t)
    }

    /// Execute a named tool. Returns an error if the tool is not found.
    pub fn execute(
        &self,
        name: &str,
        arguments: &str,
        workspace_root: &Path,
    ) -> Result<ToolResult, AiError> {
        let tool = self
            .get(name)
            .ok_or_else(|| AiError::ToolNotFound(name.to_owned()))?;
        tool.execute(arguments, workspace_root)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("tool_count", &self.tools.len())
            .finish()
    }
}
