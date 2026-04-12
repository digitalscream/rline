//! Conversation context management for the agent loop.
//!
//! Maintains the message history and system prompt for multi-turn
//! chat completions conversations, with token-aware truncation.

use crate::chat::types::{ChatMessage, ChatRequest, Role, ToolDefinition};

/// Approximate characters per token (conservative estimate for English text).
const CHARS_PER_TOKEN: usize = 4;

/// Default agent system prompt used when no custom prompt file is configured.
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are an AI coding assistant integrated into the rline text editor. You help users with software engineering tasks by reading files, writing code, searching, and executing commands.

## Guidelines
1. Always read files before modifying them to understand the existing code.
2. Use search_files to find relevant code across the project.
3. Use list_files and list_code_definition_names to understand project structure.
4. When editing files, use replace_in_file for targeted changes. Use write_to_file only for new files or complete rewrites.
5. Explain your reasoning before making changes.
6. After making changes, verify them if possible (e.g., run tests, check compilation).
7. Ask follow-up questions only when the task is genuinely ambiguous. Do not ask unnecessary clarifying questions.

## File Paths
All file paths should be relative to the workspace root unless they are absolute paths."#;

/// Manages the conversation history for an agent session.
#[derive(Debug, Clone)]
pub struct ConversationContext {
    /// The system prompt, always the first message.
    system_prompt: String,
    /// The conversation history (excluding the system prompt).
    messages: Vec<ChatMessage>,
    /// Maximum context length in tokens.
    max_tokens: usize,
}

impl ConversationContext {
    /// Create a new context with the given system prompt and token limit.
    pub fn new(system_prompt: impl Into<String>, max_tokens: usize) -> Self {
        Self {
            system_prompt: system_prompt.into(),
            messages: Vec::new(),
            max_tokens,
        }
    }

    /// Add a user message to the conversation.
    pub fn add_user_message(&mut self, content: impl Into<String>) {
        self.messages.push(ChatMessage::user(content));
        self.maybe_truncate();
    }

    /// Add an assistant message (text only, no tool calls).
    pub fn add_assistant_message(&mut self, content: impl Into<String>) {
        self.messages.push(ChatMessage::assistant(content));
        self.maybe_truncate();
    }

    /// Add an assistant message that contains tool calls.
    pub fn add_assistant_tool_calls(
        &mut self,
        text: Option<String>,
        tool_calls: Vec<crate::chat::types::ToolCall>,
    ) {
        self.messages
            .push(ChatMessage::assistant_tool_calls(text, tool_calls));
        self.maybe_truncate();
    }

    /// Add a plain-text tool result message.
    pub fn add_tool_result(&mut self, tool_call_id: impl Into<String>, content: impl Into<String>) {
        self.messages
            .push(ChatMessage::tool_result(tool_call_id, content));
        self.maybe_truncate();
    }

    /// Add a multimodal tool result message with an inline PNG image.
    ///
    /// `png_base64` must be the raw base64 payload (no `data:` prefix).
    pub fn add_tool_result_with_image(
        &mut self,
        tool_call_id: impl Into<String>,
        text: impl Into<String>,
        png_base64: String,
    ) {
        self.messages.push(ChatMessage::tool_result_with_image(
            tool_call_id,
            text,
            png_base64,
        ));
        self.maybe_truncate();
    }

    /// Build a [`ChatRequest`] from the current context.
    pub fn to_request(
        &self,
        model: &str,
        tools: Vec<ToolDefinition>,
        max_tokens: Option<u32>,
        temperature: Option<f64>,
    ) -> ChatRequest {
        let mut messages = Vec::with_capacity(self.messages.len() + 1);
        messages.push(ChatMessage::system(&self.system_prompt));
        messages.extend(self.messages.iter().cloned());

        ChatRequest {
            model: model.to_owned(),
            messages,
            tools: if tools.is_empty() { None } else { Some(tools) },
            stream: true,
            max_tokens,
            temperature,
        }
    }

    /// Clear all messages but keep the system prompt.
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Get the number of messages (excluding system prompt).
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Export the conversation as a human-readable Markdown string
    /// suitable for saving to a history file.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("# Agent Conversation\n\n");

        for msg in &self.messages {
            let role_label = match msg.role {
                Role::User => "## User",
                Role::Assistant => "## Assistant",
                Role::Tool => "## Tool Result",
                Role::System => "## System",
            };
            out.push_str(role_label);
            out.push('\n');

            if let Some(id) = &msg.tool_call_id {
                out.push_str(&format!("*tool_call_id: {id}*\n"));
            }

            if let Some(content) = &msg.content {
                out.push('\n');
                out.push_str(&content.as_text());
                out.push('\n');
            }

            if let Some(tool_calls) = &msg.tool_calls {
                for tc in tool_calls {
                    out.push_str(&format!(
                        "\n**Tool call:** `{}` ({})\n```json\n{}\n```\n",
                        tc.function.name, tc.id, tc.function.arguments
                    ));
                }
            }

            out.push('\n');
        }

        out
    }

    /// Estimate the current total token usage (system prompt + all messages).
    pub fn estimated_tokens(&self) -> usize {
        let system_chars = self.system_prompt.len();
        let message_chars: usize = self.messages.iter().map(message_char_count).sum();
        (system_chars + message_chars) / CHARS_PER_TOKEN
    }

    /// Get the configured maximum token limit.
    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    /// Truncate old messages if total tokens exceed the budget.
    ///
    /// Removes the oldest complete conversational units (user message +
    /// assistant response + any tool call/result pairs) to preserve
    /// coherent conversation structure. Never removes the most recent
    /// exchange.
    fn maybe_truncate(&mut self) {
        while self.estimated_tokens() > self.max_tokens && self.messages.len() > 2 {
            // Find the end of the first complete exchange to remove.
            // A complete exchange is: user → assistant (with optional tool calls/results).
            let remove_end = find_exchange_end(&self.messages);
            if remove_end == 0 {
                // Can't find a safe removal point — stop.
                break;
            }
            self.messages.drain(..remove_end);
        }
    }
}

/// Count approximate characters in a message (content + tool call arguments).
fn message_char_count(m: &ChatMessage) -> usize {
    let content_len = m.content.as_ref().map_or(0, |c| c.char_len());
    let tool_calls_len = m.tool_calls.as_ref().map_or(0, |tc| {
        tc.iter()
            .map(|t| t.function.name.len() + t.function.arguments.len())
            .sum()
    });
    // Add overhead for role, JSON structure, etc.
    content_len + tool_calls_len + 20
}

/// Find the index of the end of the first complete conversational exchange.
///
/// Returns the number of messages to remove from the front. A complete
/// exchange starts with a user message and includes all subsequent
/// assistant/tool messages until the next user message.
fn find_exchange_end(messages: &[ChatMessage]) -> usize {
    if messages.len() <= 2 {
        return 0;
    }

    // Skip to after the first user message.
    let mut i = 0;
    if messages[i].role == Role::User {
        i += 1;
    }

    // Include all assistant/tool messages until the next user message.
    while i < messages.len() && messages[i].role != Role::User {
        i += 1;
    }

    // Don't remove everything — keep at least the last message.
    if i >= messages.len() {
        return 0;
    }

    i
}

/// Build the default system prompt for the agent.
///
/// Automatically discovers and includes `.clinerules` and `memory-bank`
/// content from the workspace root, matching Cline's behavior.
/// If `mcp_tool_summary` is non-empty, it is appended to inform the model
/// about available MCP tools.
pub fn build_system_prompt(
    workspace_root: &str,
    mode: &str,
    custom_prompt: Option<&str>,
    mcp_tool_summary: Option<&str>,
) -> String {
    let base = custom_prompt.unwrap_or(DEFAULT_SYSTEM_PROMPT);

    let mut prompt = format!(
        r#"{base}

## Environment
- Workspace root: {workspace_root}
- Current mode: {mode}

## Mode Behavior
- In PLAN mode: You can only read files, list directories, search, and analyze code. You CANNOT modify files or execute commands. When you have finished your analysis, you MUST call `plan_mode_respond` with your complete plan. Do NOT attempt to execute the plan yourself — the user will switch to Act mode to do that.
- In ACT mode: You can read, write, and modify files, execute commands, and perform all available actions. When the task is done, call `attempt_completion` with a summary.
- In PLAN mode: call `plan_mode_respond` when your plan is ready.
- In ACT mode: call `attempt_completion` when the task is done."#
    );

    let root = std::path::Path::new(workspace_root);

    // ── .clinerules ──
    let rules = load_clinerules(root);
    if !rules.is_empty() {
        prompt.push_str("\n\n## Project Rules (.clinerules)\n\n");
        prompt.push_str(&rules);
    }

    // ── memory-bank ──
    let memory = load_memory_bank(root);
    if !memory.is_empty() {
        prompt.push_str("\n\n## Memory Bank\n\n");
        prompt.push_str(&memory);
    }

    // ── MCP tools ──
    if let Some(summary) = mcp_tool_summary {
        if !summary.is_empty() {
            prompt.push_str("\n\n## External Tools (MCP Servers)\n\n");
            prompt.push_str(
                "The following tools are provided by external MCP servers. \
                 Use them when they can help accomplish the user's task. \
                 They are available as regular tools in your tool list.\n\n",
            );
            prompt.push_str(summary);
        }
    }

    prompt
}

/// Load project rules from `.clinerules` (file or directory).
fn load_clinerules(workspace_root: &std::path::Path) -> String {
    let path = workspace_root.join(".clinerules");

    if path.is_file() {
        return std::fs::read_to_string(&path).unwrap_or_default();
    }

    if path.is_dir() {
        let mut parts = Vec::new();
        let entries = match std::fs::read_dir(&path) {
            Ok(e) => e,
            Err(_) => return String::new(),
        };

        let mut files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.ends_with(".md") || name.ends_with(".txt")
            })
            .collect();
        files.sort_by_key(|e| e.file_name());

        for entry in files {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if !content.trim().is_empty() {
                    let name = entry.file_name();
                    parts.push(format!("### {}\n\n{}", name.to_string_lossy(), content));
                }
            }
        }

        return parts.join("\n\n");
    }

    String::new()
}

/// Load all markdown files from the `memory-bank` directory.
fn load_memory_bank(workspace_root: &std::path::Path) -> String {
    let path = workspace_root.join("memory-bank");

    if !path.is_dir() {
        return String::new();
    }

    let entries = match std::fs::read_dir(&path) {
        Ok(e) => e,
        Err(_) => return String::new(),
    };

    let mut files: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".md"))
        .collect();
    files.sort_by_key(|e| e.file_name());

    let mut parts = Vec::new();
    for entry in files {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            if !content.trim().is_empty() {
                let name = entry.file_name();
                parts.push(format!("### {}\n\n{}", name.to_string_lossy(), content));
            }
        }
    }

    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_add_messages() {
        let mut ctx = ConversationContext::new("system prompt", 100_000);
        ctx.add_user_message("hello");
        ctx.add_assistant_message("hi there");
        assert_eq!(ctx.message_count(), 2);
    }

    #[test]
    fn test_context_to_request() {
        let mut ctx = ConversationContext::new("system prompt", 100_000);
        ctx.add_user_message("hello");

        let req = ctx.to_request("test-model", vec![], Some(100), Some(0.0));
        assert_eq!(req.messages.len(), 2); // system + user
        assert_eq!(req.model, "test-model");
        assert!(req.tools.is_none());
    }

    #[test]
    fn test_context_clear() {
        let mut ctx = ConversationContext::new("system prompt", 100_000);
        ctx.add_user_message("hello");
        ctx.add_assistant_message("hi");
        ctx.clear();
        assert_eq!(ctx.message_count(), 0);
    }

    #[test]
    fn test_build_system_prompt_contains_workspace() {
        let prompt = build_system_prompt("/home/user/project", "ACT", None, None);
        assert!(prompt.contains("/home/user/project"));
        assert!(prompt.contains("ACT"));
    }

    #[test]
    fn test_build_system_prompt_custom_overrides_default() {
        let custom = "You are a helpful pirate assistant.";
        let prompt = build_system_prompt("/tmp/proj", "PLAN", Some(custom), None);
        assert!(
            prompt.contains(custom),
            "custom prompt text should appear in output"
        );
        assert!(
            !prompt.contains("You are an AI coding assistant"),
            "default prompt text should not appear when custom is provided"
        );
        assert!(
            prompt.contains("/tmp/proj"),
            "workspace root should still appear"
        );
        assert!(prompt.contains("PLAN"), "mode should still appear");
    }

    #[test]
    fn test_estimated_tokens() {
        let mut ctx = ConversationContext::new("system", 100_000);
        let initial = ctx.estimated_tokens();
        ctx.add_user_message("hello world"); // ~11 chars + 20 overhead
        assert!(
            ctx.estimated_tokens() > initial,
            "tokens should increase after adding a message"
        );
    }

    #[test]
    fn test_truncation_preserves_recent() {
        // Create a context with very small limit.
        let mut ctx = ConversationContext::new("sys", 50);
        ctx.add_user_message("first question");
        ctx.add_assistant_message("first answer");
        ctx.add_user_message("second question");
        ctx.add_assistant_message("second answer");
        ctx.add_user_message(
            "third question that triggers truncation with a long message to push over the limit",
        );

        // Should have truncated the oldest exchange but kept recent messages.
        assert!(
            ctx.message_count() < 5,
            "should have truncated some messages, got {}",
            ctx.message_count()
        );
        assert!(
            ctx.message_count() >= 2,
            "should keep at least the recent messages"
        );
    }

    #[test]
    fn test_find_exchange_end_basic() {
        let msgs = vec![
            ChatMessage::user("q1"),
            ChatMessage::assistant("a1"),
            ChatMessage::user("q2"),
            ChatMessage::assistant("a2"),
        ];
        assert_eq!(find_exchange_end(&msgs), 2);
    }

    #[test]
    fn test_find_exchange_end_with_tool_calls() {
        let msgs = vec![
            ChatMessage::user("q1"),
            ChatMessage::assistant("thinking"),
            ChatMessage::tool_result("tc1", "result"),
            ChatMessage::user("q2"),
        ];
        assert_eq!(find_exchange_end(&msgs), 3);
    }

    #[test]
    fn test_find_exchange_end_too_few() {
        let msgs = vec![ChatMessage::user("q1"), ChatMessage::assistant("a1")];
        assert_eq!(
            find_exchange_end(&msgs),
            0,
            "should not remove when only 2 messages"
        );
    }
}
