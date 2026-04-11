//! Events emitted by the agent loop for the UI to consume.

use std::path::PathBuf;

use tokio::sync::oneshot;

use crate::tools::ToolCategory;

/// Events sent from the agent loop to the UI.
pub enum AgentEvent {
    /// Incremental text content from the AI response.
    TextDelta(String),

    /// A tool call has been parsed and is about to be (or needs to be) executed.
    ToolCallStart {
        /// The unique tool call ID from the API.
        id: String,
        /// Tool name.
        name: String,
        /// JSON arguments string.
        arguments: String,
    },

    /// The agent needs user approval before executing a tool.
    ///
    /// Send `true` through the oneshot to approve, `false` to deny.
    ApprovalNeeded {
        /// The unique tool call ID.
        id: String,
        /// Tool name.
        name: String,
        /// Tool category for display.
        category: ToolCategory,
        /// JSON arguments string.
        arguments: String,
        /// Channel to send the approval decision through.
        respond: oneshot::Sender<bool>,
    },

    /// A tool has finished executing.
    ToolResult {
        /// The unique tool call ID.
        id: String,
        /// Tool name.
        name: String,
        /// Whether execution was successful.
        success: bool,
        /// Tool output text.
        output: String,
    },

    /// A follow-up question from the agent for the user to answer.
    FollowupQuestion {
        /// The question text.
        question: String,
        /// Channel to send the user's answer through.
        respond: oneshot::Sender<String>,
    },

    /// A file was created or modified by a tool (write_to_file, replace_in_file).
    ///
    /// The UI should open a diff view for this file.
    FileChanged {
        /// Absolute path to the modified file.
        path: PathBuf,
    },

    /// Updated context usage estimate.
    ContextUpdate {
        /// Estimated tokens currently used.
        used_tokens: usize,
        /// Maximum context length in tokens.
        max_tokens: usize,
    },

    /// The current AI turn is complete (no more tool calls in this turn).
    TurnComplete,

    /// An error occurred during the agent loop.
    Error(String),

    /// Request to execute a command in the UI terminal.
    ///
    /// The agent loop sends this instead of running `execute_command` via
    /// `std::process::Command`, so the command runs in the user's real shell
    /// environment (with rbenv, nvm, etc.).
    TerminalCommand {
        /// The tool call ID.
        id: String,
        /// The shell command to run.
        command: String,
        /// Working directory.
        working_dir: PathBuf,
        /// Timeout in seconds.
        timeout_secs: u64,
        /// Channel to send the result back to the agent loop.
        respond: oneshot::Sender<(bool, String)>,
    },

    /// The agent has completed the task (or plan).
    Finished {
        /// Summary of what was accomplished.
        summary: Option<String>,
        /// Whether the agent was running in Plan mode (user should switch to Act).
        plan_mode: bool,
    },
}

// AgentEvent contains oneshot::Sender which is not Debug.
impl std::fmt::Debug for AgentEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TextDelta(t) => f.debug_tuple("TextDelta").field(t).finish(),
            Self::ToolCallStart { id, name, .. } => f
                .debug_struct("ToolCallStart")
                .field("id", id)
                .field("name", name)
                .finish(),
            Self::ApprovalNeeded { id, name, .. } => f
                .debug_struct("ApprovalNeeded")
                .field("id", id)
                .field("name", name)
                .finish(),
            Self::ToolResult {
                id, name, success, ..
            } => f
                .debug_struct("ToolResult")
                .field("id", id)
                .field("name", name)
                .field("success", success)
                .finish(),
            Self::FollowupQuestion { question, .. } => f
                .debug_struct("FollowupQuestion")
                .field("question", question)
                .finish(),
            Self::FileChanged { path } => {
                f.debug_struct("FileChanged").field("path", path).finish()
            }
            Self::ContextUpdate {
                used_tokens,
                max_tokens,
            } => f
                .debug_struct("ContextUpdate")
                .field("used_tokens", used_tokens)
                .field("max_tokens", max_tokens)
                .finish(),
            Self::TurnComplete => write!(f, "TurnComplete"),
            Self::Error(e) => f.debug_tuple("Error").field(e).finish(),
            Self::TerminalCommand { id, command, .. } => f
                .debug_struct("TerminalCommand")
                .field("id", id)
                .field("command", command)
                .finish(),
            Self::Finished { summary, plan_mode } => f
                .debug_struct("Finished")
                .field("summary", summary)
                .field("plan_mode", plan_mode)
                .finish(),
        }
    }
}
