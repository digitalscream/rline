//! Tool definitions and execution for the AI agent.
//!
//! Each tool implements the [`Tool`] trait and is registered in a
//! [`ToolRegistry`]. The agent loop uses the registry to dispatch
//! tool calls from the AI model.

pub mod ask_followup_question;
pub mod attempt_completion;
pub mod browser_action;
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
use std::sync::Arc;

use crate::chat::types::ToolDefinition;
use crate::error::AiError;

/// The result of executing a tool.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Whether the tool executed successfully.
    pub success: bool,
    /// The text output to send back to the AI model.
    pub output: String,
    /// Optional PNG screenshot bytes. When present and the agent is configured
    /// for multimodal models, the image is attached to the tool result message.
    /// Otherwise the tool is expected to reference the image through `output`
    /// (e.g. by saving it to disk and including the path).
    pub image_png: Option<Vec<u8>>,
}

impl ToolResult {
    /// Create a successful tool result.
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
            image_png: None,
        }
    }

    /// Create a failed tool result.
    pub fn err(output: impl Into<String>) -> Self {
        Self {
            success: false,
            output: output.into(),
            image_png: None,
        }
    }

    /// Create a successful tool result carrying a PNG screenshot.
    pub fn ok_with_image(output: impl Into<String>, png: Vec<u8>) -> Self {
        Self {
            success: true,
            output: output.into(),
            image_png: Some(png),
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
    /// Tools from a trusted MCP server (auto-approved).
    McpTrusted,
    /// Tools from an untrusted MCP server (always requires user approval).
    McpUntrusted,
    /// Tools that automate a web browser.
    Browser,
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
///
/// Wraps tools in an `Arc` so the registry can be cheaply cloned and shared
/// across threads (e.g. into `spawn_blocking` closures).
#[derive(Clone)]
pub struct ToolRegistry {
    tools: Arc<Vec<Box<dyn Tool>>>,
}

/// Configuration for the browser_action tool.
#[derive(Debug, Clone)]
pub struct BrowserConfig {
    /// Tokio runtime handle the browser tool will use for async CDP calls.
    pub runtime: tokio::runtime::Handle,
    /// Viewport (width, height) in pixels.
    pub viewport: (u32, u32),
    /// Whether the agent model accepts multimodal (image) input.
    pub multimodal: bool,
}

impl ToolRegistry {
    /// Create a registry with all built-in tools, using defaults for the
    /// browser tool. Prefer [`ToolRegistry::builder`] when the caller can
    /// supply a configured tokio runtime handle.
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// Start building a customised registry.
    pub fn builder() -> ToolRegistryBuilder {
        ToolRegistryBuilder::default()
    }

    /// Create a registry with built-in tools plus additional tools (e.g. MCP tools).
    pub fn with_extra_tools(extra: Vec<Box<dyn Tool>>) -> Self {
        Self::builder().extra_tools(extra).build()
    }

    /// Get tool definitions filtered by mode.
    ///
    /// - Plan mode (`plan_mode=true`): read-only built-in tools + `plan_mode_respond` + all MCP tools
    /// - Act mode (`plan_mode=false`): all tools except `plan_mode_respond`
    ///
    /// MCP tools are always included regardless of mode because their
    /// read-only status cannot be determined. The permission system
    /// (`McpTrusted` / `McpUntrusted`) handles approval separately.
    pub fn definitions(&self, plan_mode: bool) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .filter(|t| {
                if plan_mode {
                    t.is_read_only()
                        || matches!(
                            t.category(),
                            ToolCategory::McpTrusted | ToolCategory::McpUntrusted
                        )
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

/// Builder for [`ToolRegistry`].
#[derive(Default)]
pub struct ToolRegistryBuilder {
    extra: Vec<Box<dyn Tool>>,
    browser: Option<BrowserConfig>,
    command_timeout_secs: Option<u64>,
}

impl ToolRegistryBuilder {
    /// Append tools from an MCP server or other external source.
    pub fn extra_tools(mut self, extra: Vec<Box<dyn Tool>>) -> Self {
        self.extra.extend(extra);
        self
    }

    /// Configure the browser_action tool. Omit to skip registering it.
    pub fn browser(mut self, config: BrowserConfig) -> Self {
        self.browser = Some(config);
        self
    }

    /// Override the shell command execution timeout.
    pub fn command_timeout_secs(mut self, secs: u64) -> Self {
        self.command_timeout_secs = Some(secs);
        self
    }

    /// Finalise and build the registry.
    pub fn build(self) -> ToolRegistry {
        let exec = match self.command_timeout_secs {
            Some(secs) => execute_command::ExecuteCommandTool::with_timeout(secs),
            None => execute_command::ExecuteCommandTool::default(),
        };
        let mut tools: Vec<Box<dyn Tool>> = vec![
            Box::new(read_file::ReadFileTool),
            Box::new(write_to_file::WriteToFileTool),
            Box::new(replace_in_file::ReplaceInFileTool),
            Box::new(list_files::ListFilesTool),
            Box::new(search_files::SearchFilesTool),
            Box::new(exec),
            Box::new(list_code_definition_names::ListCodeDefinitionNamesTool),
            Box::new(ask_followup_question::AskFollowupQuestionTool),
            Box::new(attempt_completion::AttemptCompletionTool),
            Box::new(plan_mode_respond::PlanModeRespondTool),
        ];
        if let Some(cfg) = self.browser {
            tools.push(Box::new(browser_action::BrowserActionTool::new(
                cfg.runtime,
                cfg.viewport,
                cfg.multimodal,
            )));
        }
        tools.extend(self.extra);
        ToolRegistry {
            tools: Arc::new(tools),
        }
    }
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("tool_count", &self.tools.len())
            .finish()
    }
}
