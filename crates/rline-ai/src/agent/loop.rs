//! Core agent loop: drives multi-turn tool-use conversations.
//!
//! The agent loop sends conversation history to the chat API, streams the
//! response, dispatches tool calls (with optional user approval), feeds
//! results back, and repeats until the model stops calling tools or the
//! task is completed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// Maximum number of consecutive failures for the same tool call before
/// the agent is forced to ask the user for guidance.
const MAX_TOOL_RETRIES: u32 = 3;

use crate::agent::context::{build_system_prompt, ConversationContext};
use crate::agent::event::AgentEvent;
use crate::chat::client::{ChatClient, StreamEvent};
use crate::chat::types::ToolCall;
use crate::mcp::manager::McpManager;
use crate::tools::{BrowserConfig, Tool, ToolCategory, ToolRegistry, ToolResult};

/// The agent's operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    /// Read-only mode: only analysis and planning tools are available.
    Plan,
    /// Full mode: all tools including file writes and command execution.
    Act,
}

impl AgentMode {
    /// Whether this mode is read-only.
    pub fn is_read_only(self) -> bool {
        self == Self::Plan
    }
}

/// Callback type for checking whether a tool call should be auto-approved.
pub type AutoApproveFn = Box<dyn Fn(&str, ToolCategory, &str) -> bool + Send + Sync>;

/// The core agent loop orchestrator.
pub struct AgentLoop {
    client: ChatClient,
    context: ConversationContext,
    registry: ToolRegistry,
    mode: AgentMode,
    event_tx: mpsc::Sender<AgentEvent>,
    auto_approve: AutoApproveFn,
    cancel: CancellationToken,
    workspace_root: PathBuf,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
    /// Maximum number of tool-use turns before forcing a stop.
    max_turns: usize,
    /// Whether the configured model accepts multimodal (image) tool results.
    multimodal: bool,
    /// MCP server manager — held to keep server processes alive for the
    /// duration of the agent loop. Cleanup happens via `McpClient::Drop`.
    _mcp_manager: Option<Arc<tokio::sync::Mutex<McpManager>>>,
}

impl AgentLoop {
    /// Create a new agent loop with a fresh conversation context.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: ChatClient,
        mode: AgentMode,
        event_tx: mpsc::Sender<AgentEvent>,
        auto_approve: AutoApproveFn,
        cancel: CancellationToken,
        workspace_root: PathBuf,
        max_tokens: Option<u32>,
        temperature: Option<f64>,
        max_context_tokens: usize,
        custom_system_prompt: Option<String>,
        max_turns: usize,
        mcp_tools: Vec<Box<dyn Tool>>,
        mcp_manager: Option<Arc<tokio::sync::Mutex<McpManager>>>,
        mcp_tool_summary: Option<String>,
        browser_config: Option<BrowserConfig>,
    ) -> Self {
        let mode_str = match mode {
            AgentMode::Plan => "PLAN",
            AgentMode::Act => "ACT",
        };
        let system_prompt = build_system_prompt(
            &workspace_root.to_string_lossy(),
            mode_str,
            custom_system_prompt.as_deref(),
            mcp_tool_summary.as_deref(),
        );

        let multimodal = browser_config
            .as_ref()
            .map(|c| c.multimodal)
            .unwrap_or(false);

        let mut registry_builder = ToolRegistry::builder().extra_tools(mcp_tools);
        if let Some(cfg) = browser_config {
            registry_builder = registry_builder.browser(cfg);
        }
        let registry = registry_builder.build();

        Self {
            client,
            context: ConversationContext::new(system_prompt, max_context_tokens),
            registry,
            mode,
            event_tx,
            auto_approve,
            cancel,
            workspace_root,
            max_tokens,
            temperature,
            max_turns,
            multimodal,
            _mcp_manager: mcp_manager,
        }
    }

    /// Create an agent loop that continues an existing conversation.
    #[allow(clippy::too_many_arguments)]
    pub fn with_context(
        client: ChatClient,
        mode: AgentMode,
        context: ConversationContext,
        event_tx: mpsc::Sender<AgentEvent>,
        auto_approve: AutoApproveFn,
        cancel: CancellationToken,
        workspace_root: PathBuf,
        max_tokens: Option<u32>,
        temperature: Option<f64>,
        max_turns: usize,
        mcp_tools: Vec<Box<dyn Tool>>,
        mcp_manager: Option<Arc<tokio::sync::Mutex<McpManager>>>,
        browser_config: Option<BrowserConfig>,
    ) -> Self {
        let multimodal = browser_config
            .as_ref()
            .map(|c| c.multimodal)
            .unwrap_or(false);

        let mut registry_builder = ToolRegistry::builder().extra_tools(mcp_tools);
        if let Some(cfg) = browser_config {
            registry_builder = registry_builder.browser(cfg);
        }
        let registry = registry_builder.build();

        Self {
            client,
            context,
            registry,
            mode,
            event_tx,
            auto_approve,
            cancel,
            workspace_root,
            max_tokens,
            temperature,
            max_turns,
            multimodal,
            _mcp_manager: mcp_manager,
        }
    }

    /// Run the agent loop with an initial user message.
    ///
    /// Returns the conversation context so it can be reused in subsequent
    /// sends (preserving history across messages and mode switches).
    pub async fn run(mut self, user_message: String) -> ConversationContext {
        info!("agent loop starting in {:?} mode", self.mode);
        self.context.add_user_message(&user_message);
        self.send_context_update();

        let mut turn = 0;
        // Track consecutive failures: key = "tool_name\0args", value = count.
        let mut failure_counts: HashMap<String, u32> = HashMap::new();

        loop {
            if self.cancel.is_cancelled() {
                let _ = self
                    .event_tx
                    .send(AgentEvent::Error("Cancelled".to_owned()));
                return self.context;
            }

            if turn >= self.max_turns {
                warn!("agent hit max turns limit ({turn})");
                let _ = self.event_tx.send(AgentEvent::Error(
                    "Maximum tool-use turns reached. Please start a new task.".to_owned(),
                ));
                return self.context;
            }

            turn += 1;
            debug!("agent turn {turn}");

            // Build the request.
            let tool_defs = self.registry.definitions(self.mode == AgentMode::Plan);
            let request = self.context.to_request(
                "", // model is set by ChatClient
                tool_defs,
                self.max_tokens,
                self.temperature,
            );

            // Send the request and get the async stream receiver.
            let mut stream_rx = self.client.send_streaming(request, self.cancel.clone());

            // Process stream events.
            let mut accumulated_text = String::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();

            loop {
                if self.cancel.is_cancelled() {
                    let _ = self
                        .event_tx
                        .send(AgentEvent::Error("Cancelled".to_owned()));
                    return self.context;
                }

                match stream_rx.recv().await {
                    Some(StreamEvent::TextDelta(delta)) => {
                        accumulated_text.push_str(&delta);
                        let _ = self.event_tx.send(AgentEvent::TextDelta(delta));
                    }
                    Some(StreamEvent::ToolCalls(calls)) => {
                        tool_calls.extend(calls);
                    }
                    Some(StreamEvent::Done { text }) => {
                        if let Some(t) = text {
                            accumulated_text = t;
                        }
                        break;
                    }
                    None => {
                        // Channel closed — stream task ended.
                        break;
                    }
                }
            }

            if tool_calls.is_empty() {
                // No tool calls — the model just responded with text.
                if !accumulated_text.is_empty() {
                    self.context.add_assistant_message(&accumulated_text);
                }
                self.send_context_update();
                let _ = self.event_tx.send(AgentEvent::TurnComplete);
                let _ = self.event_tx.send(AgentEvent::Finished {
                    summary: None,
                    plan_mode: self.mode == AgentMode::Plan,
                });
                return self.context;
            }

            // Record the assistant message with tool calls.
            let text_for_context = if accumulated_text.is_empty() {
                None
            } else {
                Some(accumulated_text.clone())
            };
            self.context
                .add_assistant_tool_calls(text_for_context, tool_calls.clone());

            // Execute each tool call.
            let mut should_finish = false;

            for tc in &tool_calls {
                if self.cancel.is_cancelled() {
                    let _ = self
                        .event_tx
                        .send(AgentEvent::Error("Cancelled".to_owned()));
                    return self.context;
                }

                let tool_name = &tc.function.name;
                let tool_args = &tc.function.arguments;

                // Notify UI that a tool call is starting.
                let _ = self.event_tx.send(AgentEvent::ToolCallStart {
                    id: tc.id.clone(),
                    name: tool_name.clone(),
                    arguments: tool_args.clone(),
                });

                // Handle special interactive tools.
                if tool_name == "ask_followup_question" {
                    let result = self.handle_followup_question(&tc.id, tool_args).await;
                    if result.is_none() {
                        // Cancelled or channel closed.
                        return self.context;
                    }
                    continue;
                }

                if tool_name == "attempt_completion" || tool_name == "plan_mode_respond" {
                    let is_plan = tool_name == "plan_mode_respond";
                    let result = self
                        .registry
                        .execute(tool_name, tool_args, &self.workspace_root);
                    let tool_result = match result {
                        Ok(r) => r,
                        Err(e) => ToolResult::err(format!("Tool error: {e}")),
                    };

                    self.context.add_tool_result(&tc.id, &tool_result.output);

                    let _ = self.event_tx.send(AgentEvent::ToolResult {
                        id: tc.id.clone(),
                        name: tool_name.clone(),
                        success: tool_result.success,
                        output: tool_result.output.clone(),
                        image_png: None,
                    });

                    let _ = self.event_tx.send(AgentEvent::Finished {
                        summary: Some(tool_result.output),
                        plan_mode: is_plan,
                    });
                    should_finish = true;
                    break;
                }

                // Route execute_command through the UI terminal for proper
                // shell environment (rbenv, nvm, virtualenv, etc.).
                if tool_name == "execute_command" {
                    // Check permission before executing.
                    let cmd_approved = if (self.auto_approve)(
                        tool_name,
                        ToolCategory::ExecuteCommand,
                        tool_args,
                    ) {
                        true
                    } else {
                        let (resp_tx, resp_rx) = oneshot::channel();
                        let _ = self.event_tx.send(AgentEvent::ApprovalNeeded {
                            id: tc.id.clone(),
                            name: tool_name.clone(),
                            category: ToolCategory::ExecuteCommand,
                            arguments: tool_args.clone(),
                            respond: resp_tx,
                        });
                        match resp_rx.await {
                            Ok(decision) => decision,
                            Err(_) => {
                                let _ = self
                                    .event_tx
                                    .send(AgentEvent::Error("Approval cancelled".to_owned()));
                                return self.context;
                            }
                        }
                    };

                    if cmd_approved {
                        let result = self
                            .handle_terminal_command(&tc.id, tool_args, &mut failure_counts)
                            .await;
                        if result.is_none() {
                            return self.context;
                        }
                    } else {
                        let denied_msg = "Command execution denied by user.".to_owned();
                        self.context.add_tool_result(&tc.id, &denied_msg);
                        let _ = self.event_tx.send(AgentEvent::ToolResult {
                            id: tc.id.clone(),
                            name: tool_name.clone(),
                            success: false,
                            output: denied_msg,
                            image_png: None,
                        });
                    }
                    continue;
                }

                // Check permissions for non-interactive tools.
                let category = self
                    .registry
                    .get(tool_name)
                    .map(|t| t.category())
                    .unwrap_or(ToolCategory::ExecuteCommand);

                let approved = if (self.auto_approve)(tool_name, category, tool_args) {
                    true
                } else {
                    // Ask the UI for approval.
                    let (resp_tx, resp_rx) = oneshot::channel();
                    let _ = self.event_tx.send(AgentEvent::ApprovalNeeded {
                        id: tc.id.clone(),
                        name: tool_name.clone(),
                        category,
                        arguments: tool_args.clone(),
                        respond: resp_tx,
                    });

                    // Wait for the user's decision.
                    match resp_rx.await {
                        Ok(decision) => decision,
                        Err(_) => {
                            // Channel closed — user likely cancelled.
                            let _ = self
                                .event_tx
                                .send(AgentEvent::Error("Approval cancelled".to_owned()));
                            return self.context;
                        }
                    }
                };

                let tool_result = if approved {
                    // Execute the tool on a blocking thread, but race against
                    // the cancellation token so Stop takes effect immediately.
                    let name = tool_name.clone();
                    let args = tool_args.clone();
                    let root = self.workspace_root.clone();
                    let registry = self.registry.clone();
                    let handle =
                        tokio::task::spawn_blocking(move || registry.execute(&name, &args, &root));

                    tokio::select! {
                        result = handle => {
                            match result {
                                Ok(Ok(r)) => r,
                                Ok(Err(e)) => ToolResult::err(format!("Tool error: {e}")),
                                Err(e) => ToolResult::err(format!("Task join error: {e}")),
                            }
                        }
                        _ = self.cancel.cancelled() => {
                            let _ = self.event_tx.send(AgentEvent::Error("Cancelled".to_owned()));
                            return self.context;
                        }
                    }
                } else {
                    ToolResult::err("Tool execution denied by user.".to_owned())
                };

                // Track consecutive failures for the same tool+args.
                let failure_key = format!("{tool_name}\0{tool_args}");
                let mut result_output = tool_result.output.clone();

                if tool_result.success {
                    failure_counts.remove(&failure_key);
                } else {
                    let count = failure_counts.entry(failure_key).or_insert(0);
                    *count += 1;
                    if *count >= MAX_TOOL_RETRIES {
                        result_output.push_str(&format!(
                            "\n\n[SYSTEM] This tool call has failed {} consecutive times with \
                             the same arguments. You MUST try a different approach or use \
                             ask_followup_question to ask the user for guidance. Do NOT retry \
                             the same command again.",
                            *count
                        ));
                    }
                }

                let image_for_event = tool_result.image_png.clone();
                if let (true, Some(png)) = (self.multimodal, tool_result.image_png) {
                    use base64::engine::general_purpose::STANDARD;
                    use base64::Engine;
                    let b64 = STANDARD.encode(&png);
                    self.context
                        .add_tool_result_with_image(&tc.id, &result_output, b64);
                } else {
                    self.context.add_tool_result(&tc.id, &result_output);
                }

                // Emit FileChanged for file-editing tools so the UI can show a diff.
                if tool_result.success
                    && (tool_name == "write_to_file" || tool_name == "replace_in_file")
                {
                    if let Some(path) = extract_file_path(tool_args, &self.workspace_root) {
                        let _ = self.event_tx.send(AgentEvent::FileChanged { path });
                    }
                }

                let _ = self.event_tx.send(AgentEvent::ToolResult {
                    id: tc.id.clone(),
                    name: tool_name.clone(),
                    success: tool_result.success,
                    output: tool_result.output,
                    image_png: image_for_event,
                });
            }

            self.send_context_update();

            if should_finish {
                return self.context;
            }

            // Continue to next turn — the model will see the tool results.
        }
    }

    /// Send a context usage update to the UI.
    fn send_context_update(&self) {
        let _ = self.event_tx.send(AgentEvent::ContextUpdate {
            used_tokens: self.context.estimated_tokens(),
            max_tokens: self.context.max_tokens(),
        });
    }

    /// Handle the `ask_followup_question` tool by routing to the UI.
    async fn handle_followup_question(
        &mut self,
        tool_call_id: &str,
        arguments: &str,
    ) -> Option<()> {
        #[derive(serde::Deserialize)]
        struct QuestionArgs {
            question: String,
        }

        let args: QuestionArgs = match serde_json::from_str(arguments) {
            Ok(a) => a,
            Err(e) => {
                let err_msg = format!("Invalid question arguments: {e}");
                self.context.add_tool_result(tool_call_id, &err_msg);
                let _ = self.event_tx.send(AgentEvent::ToolResult {
                    id: tool_call_id.to_owned(),
                    name: "ask_followup_question".to_owned(),
                    success: false,
                    output: err_msg,
                    image_png: None,
                });
                return Some(());
            }
        };

        let (resp_tx, resp_rx) = oneshot::channel();
        let _ = self.event_tx.send(AgentEvent::FollowupQuestion {
            question: args.question,
            respond: resp_tx,
        });

        match resp_rx.await {
            Ok(answer) => {
                self.context.add_tool_result(tool_call_id, &answer);
                let _ = self.event_tx.send(AgentEvent::ToolResult {
                    id: tool_call_id.to_owned(),
                    name: "ask_followup_question".to_owned(),
                    success: true,
                    output: answer,
                    image_png: None,
                });
                Some(())
            }
            Err(_) => {
                let _ = self
                    .event_tx
                    .send(AgentEvent::Error("Followup question cancelled".to_owned()));
                None
            }
        }
    }

    /// Handle `execute_command` by routing it to the UI terminal.
    async fn handle_terminal_command(
        &mut self,
        tool_call_id: &str,
        arguments: &str,
        failure_counts: &mut HashMap<String, u32>,
    ) -> Option<()> {
        #[derive(serde::Deserialize)]
        struct CmdArgs {
            command: String,
            #[serde(default)]
            timeout_secs: Option<u64>,
        }

        let args: CmdArgs = match serde_json::from_str(arguments) {
            Ok(a) => a,
            Err(e) => {
                let err_msg = format!("Invalid command arguments: {e}");
                self.context.add_tool_result(tool_call_id, &err_msg);
                let _ = self.event_tx.send(AgentEvent::ToolResult {
                    id: tool_call_id.to_owned(),
                    name: "execute_command".to_owned(),
                    success: false,
                    output: err_msg,
                    image_png: None,
                });
                return Some(());
            }
        };

        let timeout = args.timeout_secs.unwrap_or(30);

        let (resp_tx, resp_rx) = oneshot::channel();
        let _ = self.event_tx.send(AgentEvent::TerminalCommand {
            id: tool_call_id.to_owned(),
            command: args.command.clone(),
            working_dir: self.workspace_root.clone(),
            timeout_secs: timeout,
            respond: resp_tx,
        });

        // Wait for the UI to execute the command and return the result.
        let (success, output) = tokio::select! {
            result = resp_rx => {
                match result {
                    Ok(r) => r,
                    Err(_) => (false, "Terminal command channel closed".to_owned()),
                }
            }
            _ = self.cancel.cancelled() => {
                let _ = self.event_tx.send(AgentEvent::Error("Cancelled".to_owned()));
                return None;
            }
        };

        // Track consecutive failures.
        let failure_key = format!("execute_command\0{}", args.command);
        let mut result_output = output.clone();

        if success {
            failure_counts.remove(&failure_key);
        } else {
            let count = failure_counts.entry(failure_key).or_insert(0);
            *count += 1;
            if *count >= MAX_TOOL_RETRIES {
                result_output.push_str(&format!(
                    "\n\n[SYSTEM] This command has failed {} consecutive times. \
                     You MUST try a different approach or use ask_followup_question \
                     to ask the user for guidance. Do NOT retry the same command again.",
                    *count
                ));
            }
        }

        self.context.add_tool_result(tool_call_id, &result_output);

        let _ = self.event_tx.send(AgentEvent::ToolResult {
            id: tool_call_id.to_owned(),
            name: "execute_command".to_owned(),
            success,
            output,
            image_png: None,
        });

        Some(())
    }
}

/// Extract the "path" field from tool arguments and resolve it against
/// the workspace root to produce an absolute path.
fn extract_file_path(arguments: &str, workspace_root: &Path) -> Option<PathBuf> {
    let v: serde_json::Value = serde_json::from_str(arguments).ok()?;
    let path_str = v.get("path")?.as_str()?;
    let p = std::path::Path::new(path_str);
    if p.is_absolute() {
        Some(p.to_path_buf())
    } else {
        Some(workspace_root.join(p))
    }
}
