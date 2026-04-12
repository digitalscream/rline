//! AgentPanel — right-pane chat UI for the AI coding agent.
//!
//! Provides a chat interface where the user sends tasks, the AI streams
//! responses, executes tools (with approval when needed), and reports
//! completion.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

use gtk4::prelude::*;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use rline_ai::agent::context::ConversationContext;
use rline_ai::agent::event::AgentEvent;
use rline_ai::agent::r#loop::{AgentLoop, AgentMode};
use rline_ai::chat::client::ChatClient;

use super::message_widget;
use super::permission;

/// State for consolidating consecutive identical tool calls under one card.
#[derive(Clone)]
struct LastToolBlock {
    name: String,
    arguments: String,
    count_label: gtk4::Label,
    result_box: gtk4::Box,
    button_box: gtk4::Box,
    count: Rc<RefCell<usize>>,
}

/// The AI agent panel displayed in the right pane.
#[derive(Clone)]
pub struct AgentPanel {
    container: gtk4::Box,
    messages_box: gtk4::Box,
    scrolled: gtk4::ScrolledWindow,
    input_view: gtk4::TextView,
    send_button: gtk4::Button,
    stop_button: gtk4::Button,
    continue_button: gtk4::Button,
    mode_dropdown: gtk4::DropDown,
    new_task_button: gtk4::Button,
    project_root: Rc<RefCell<Option<PathBuf>>>,
    cancel_token: Rc<RefCell<Option<CancellationToken>>>,
    is_running: Rc<RefCell<bool>>,
    /// The label being streamed to for the current AI response.
    current_ai_label: Rc<RefCell<Option<gtk4::Label>>>,
    /// Map of tool call ID to (result_box, button_box) for pending tool calls.
    #[allow(clippy::type_complexity)]
    pending_tool_widgets: Rc<RefCell<Vec<(String, gtk4::Box, gtk4::Box)>>>,
    /// The most recently appended tool call block, used to consolidate
    /// consecutive duplicate tool calls under a single header with a `×N`
    /// repeat badge. Cleared on "New Task".
    last_tool_block: Rc<RefCell<Option<LastToolBlock>>>,
    /// Shared conversation context persisted across sends and mode switches.
    conversation_context: Arc<Mutex<Option<ConversationContext>>>,
    /// Path to the history file for the current conversation (one file per task).
    history_file: Arc<Mutex<Option<PathBuf>>>,
    /// Callback invoked when the agent edits a file and a diff should be shown.
    #[allow(clippy::type_complexity)]
    on_open_diff: Rc<RefCell<Option<Box<dyn Fn(&std::path::Path)>>>>,
    /// Callback invoked when the agent needs to execute a command in the terminal.
    #[allow(clippy::type_complexity)]
    on_terminal_command: Rc<
        RefCell<
            Option<
                Box<
                    dyn Fn(
                        &str,
                        &std::path::Path,
                        u64,
                        tokio::sync::oneshot::Sender<(bool, String)>,
                    ),
                >,
            >,
        >,
    >,
    /// Label showing context usage (e.g. "12k / 128k").
    context_label: gtk4::Label,
    /// "Working..." indicator widget (removed when first response event arrives).
    working_indicator: Rc<RefCell<Option<gtk4::Box>>>,
    /// Source ID for the "Working..." dot animation timer.
    working_timer: Rc<RefCell<Option<glib::SourceId>>>,
}

impl std::fmt::Debug for AgentPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentPanel").finish_non_exhaustive()
    }
}

impl Default for AgentPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentPanel {
    /// Create a new agent panel.
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.set_vexpand(true);

        // ── Header: mode selector + new task + stop ──
        let header_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        header_box.set_margin_top(4);
        header_box.set_margin_start(4);
        header_box.set_margin_end(4);
        header_box.set_margin_bottom(4);

        let mode_model = gtk4::StringList::new(&["Act", "Plan", "YOLO"]);
        let mode_dropdown = gtk4::DropDown::new(Some(mode_model), gtk4::Expression::NONE);
        mode_dropdown.set_selected(1); // Plan by default.
        mode_dropdown.set_tooltip_text(Some("Agent mode"));
        header_box.append(&mode_dropdown);

        let new_task_button = gtk4::Button::from_icon_name("document-new-symbolic");
        new_task_button.set_tooltip_text(Some("New task"));
        new_task_button.add_css_class("flat");
        header_box.append(&new_task_button);

        let stop_button = gtk4::Button::from_icon_name("process-stop-symbolic");
        stop_button.set_tooltip_text(Some("Stop"));
        stop_button.add_css_class("flat");
        stop_button.set_sensitive(false);
        header_box.append(&stop_button);

        let continue_button = gtk4::Button::from_icon_name("media-playback-start-symbolic");
        continue_button.set_tooltip_text(Some("Continue"));
        continue_button.add_css_class("flat");
        continue_button.set_sensitive(false);
        header_box.append(&continue_button);

        // Spacer to push context label to the right.
        let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        header_box.append(&spacer);

        let context_label = gtk4::Label::new(None);
        context_label.add_css_class("dim-label");
        context_label.set_halign(gtk4::Align::End);
        header_box.append(&context_label);

        container.append(&header_box);

        // ── Scrolled message area ──
        let scrolled = gtk4::ScrolledWindow::new();
        scrolled.set_vexpand(true);
        scrolled.set_hscrollbar_policy(gtk4::PolicyType::Never);

        let messages_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        messages_box.set_valign(gtk4::Align::Start);
        scrolled.set_child(Some(&messages_box));
        container.append(&scrolled);

        // ── Input area ──
        let input_frame = gtk4::Frame::new(None);
        input_frame.set_margin_start(4);
        input_frame.set_margin_end(4);
        input_frame.set_margin_bottom(4);
        input_frame.set_margin_top(4);

        let input_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

        let input_view = gtk4::TextView::new();
        input_view.set_wrap_mode(gtk4::WrapMode::Word);
        input_view.set_left_margin(4);
        input_view.set_right_margin(4);
        input_view.set_top_margin(4);
        input_view.set_bottom_margin(4);
        // Set a height request for ~3 lines.
        input_view.set_size_request(-1, 60);

        let input_scrolled = gtk4::ScrolledWindow::new();
        input_scrolled.set_child(Some(&input_view));
        input_scrolled.set_max_content_height(120);
        input_scrolled.set_propagate_natural_height(true);
        input_box.append(&input_scrolled);

        let send_button = gtk4::Button::with_label("Send");
        send_button.add_css_class("suggested-action");
        send_button.set_margin_start(4);
        send_button.set_margin_end(4);
        send_button.set_margin_bottom(4);
        send_button.set_margin_top(2);
        input_box.append(&send_button);

        input_frame.set_child(Some(&input_box));
        container.append(&input_frame);

        let panel = Self {
            container,
            messages_box,
            scrolled,
            input_view,
            send_button,
            stop_button,
            continue_button,
            mode_dropdown,
            new_task_button,
            project_root: Rc::new(RefCell::new(None)),
            cancel_token: Rc::new(RefCell::new(None)),
            is_running: Rc::new(RefCell::new(false)),
            current_ai_label: Rc::new(RefCell::new(None)),
            pending_tool_widgets: Rc::new(RefCell::new(Vec::new())),
            last_tool_block: Rc::new(RefCell::new(None)),
            conversation_context: Arc::new(Mutex::new(None)),
            history_file: Arc::new(Mutex::new(None)),
            on_open_diff: Rc::new(RefCell::new(None)),
            on_terminal_command: Rc::new(RefCell::new(None)),
            context_label,
            working_indicator: Rc::new(RefCell::new(None)),
            working_timer: Rc::new(RefCell::new(None)),
        };

        panel.connect_signals();
        panel
    }

    /// Get the root widget for this panel.
    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    /// Set the workspace root directory.
    pub fn set_project_root(&self, root: &std::path::Path) {
        *self.project_root.borrow_mut() = Some(root.to_path_buf());
    }

    /// Focus the input area.
    pub fn focus_input(&self) {
        self.input_view.grab_focus();
    }

    /// Set a callback to be invoked when the agent edits a file and a diff
    /// should be displayed.
    pub fn set_on_open_diff<F: Fn(&std::path::Path) + 'static>(&self, f: F) {
        *self.on_open_diff.borrow_mut() = Some(Box::new(f));
    }

    /// Set a callback to be invoked when the agent needs to execute a command
    /// in the terminal pane.
    pub fn set_on_terminal_command<
        F: Fn(&str, &std::path::Path, u64, tokio::sync::oneshot::Sender<(bool, String)>) + 'static,
    >(
        &self,
        f: F,
    ) {
        *self.on_terminal_command.borrow_mut() = Some(Box::new(f));
    }

    /// Connect button signals.
    fn connect_signals(&self) {
        // Send button.
        let panel = self.clone();
        self.send_button.connect_clicked(move |_| {
            panel.on_send();
        });

        // Enter key in input (Shift+Enter for newline).
        let panel = self.clone();
        let key_controller = gtk4::EventControllerKey::new();
        key_controller.connect_key_pressed(move |_, key, _, mods| {
            if key == gtk4::gdk::Key::Return && !mods.contains(gtk4::gdk::ModifierType::SHIFT_MASK)
            {
                panel.on_send();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.input_view.add_controller(key_controller);

        // Stop button — cancel the running agent.
        let panel = self.clone();
        self.stop_button.connect_clicked(move |_| {
            if let Some(token) = panel.cancel_token.borrow().as_ref() {
                token.cancel();
            }
            // UI state is updated by the poll loop's Disconnected/Error handler,
            // but we enable Continue immediately for responsiveness.
            panel.continue_button.set_sensitive(true);
            panel.continue_button.add_css_class("success");
        });

        // Continue button — resume the conversation after stopping.
        let panel = self.clone();
        self.continue_button.connect_clicked(move |_| {
            panel.on_continue();
        });

        // New task button — clears the conversation.
        let panel = self.clone();
        self.new_task_button.connect_clicked(move |_| {
            panel.clear_messages();
        });
    }

    /// Handle the continue action — resume the agent after stopping.
    fn on_continue(&self) {
        if *self.is_running.borrow() {
            return;
        }
        self.continue_button.set_sensitive(false);
        self.continue_button.remove_css_class("success");
        self.start_agent("Continue from where you left off.".to_owned());
    }

    /// Handle the send action.
    fn on_send(&self) {
        if *self.is_running.borrow() {
            return;
        }

        let buffer = self.input_view.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();
        let text = text.trim().to_owned();

        if text.is_empty() {
            return;
        }

        // Clear input.
        buffer.set_text("");

        // Add user message to the UI.
        let user_widget = message_widget::build_user_message(&text);
        self.messages_box.append(&user_widget);

        // Start the agent.
        self.start_agent(text);
    }

    /// Clear all messages from the panel.
    fn clear_messages(&self) {
        // Cancel any running agent.
        if let Some(token) = self.cancel_token.borrow().as_ref() {
            token.cancel();
        }

        while let Some(child) = self.messages_box.first_child() {
            self.messages_box.remove(&child);
        }

        *self.is_running.borrow_mut() = false;
        *self.current_ai_label.borrow_mut() = None;
        self.pending_tool_widgets.borrow_mut().clear();
        *self.last_tool_block.borrow_mut() = None;
        remove_working_indicator(&self.working_indicator, &self.working_timer);
        *self
            .conversation_context
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        *self.history_file.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.stop_button.set_sensitive(false);
        self.stop_button.remove_css_class("error");
        self.continue_button.set_sensitive(false);
        self.continue_button.remove_css_class("success");
        self.send_button.set_sensitive(true);
        self.input_view.set_sensitive(true);
    }

    /// Start the agent loop with the given user message.
    fn start_agent(&self, user_message: String) {
        let settings = rline_config::EditorSettings::load().unwrap_or_default();

        let workspace_root = match self.project_root.borrow().clone() {
            Some(r) => r,
            None => {
                let err = message_widget::build_error("No project is open. Open a project first.");
                self.messages_box.append(&err);
                return;
            }
        };

        // Determine the effective API key (agent-specific or fallback to inline completion key).
        let api_key = if settings.agent_api_key.is_empty() {
            settings.ai_api_key.clone()
        } else {
            settings.agent_api_key.clone()
        };

        if settings.agent_model.is_empty() {
            let err = message_widget::build_error(
                "No agent model configured. Set agent_model in Settings.",
            );
            self.messages_box.append(&err);
            return;
        }

        let client = ChatClient::new(
            &settings.agent_endpoint_url,
            &api_key,
            &settings.agent_model,
        );

        let selected_mode = self.mode_dropdown.selected();
        let is_yolo = selected_mode == 2;
        let mode = if selected_mode == 1 {
            AgentMode::Plan
        } else {
            // Both Act (0) and YOLO (2) use Act mode; YOLO just auto-approves everything.
            AgentMode::Act
        };

        let cancel = CancellationToken::new();
        *self.cancel_token.borrow_mut() = Some(cancel.clone());
        *self.is_running.borrow_mut() = true;
        self.stop_button.set_sensitive(true);
        self.stop_button.add_css_class("error");
        self.continue_button.set_sensitive(false);
        self.continue_button.remove_css_class("success");
        self.send_button.set_sensitive(false);
        self.input_view.set_sensitive(false);

        // Don't pre-create an AI message widget — TextDelta creates one lazily.
        *self.current_ai_label.borrow_mut() = None;

        // Show "Working..." indicator while waiting for the model response.
        show_working_indicator(
            &self.messages_box,
            &self.scrolled,
            &self.working_indicator,
            &self.working_timer,
        );

        // Create channels.
        let (event_tx, event_rx) = mpsc::channel::<AgentEvent>();

        // Build the auto-approve function.
        // In YOLO mode, everything is auto-approved without checking settings.
        let ws_root = workspace_root.clone();
        let auto_approve: rline_ai::agent::r#loop::AutoApproveFn = if is_yolo {
            Box::new(|_tool_name, _category, _arguments| true)
        } else {
            Box::new(move |tool_name, category, arguments| {
                let s = rline_config::EditorSettings::load().unwrap_or_default();
                permission::should_auto_approve(tool_name, category, arguments, &ws_root, &s)
            })
        };

        let max_tokens = Some(settings.agent_max_tokens);
        let temperature = Some(settings.agent_temperature);
        let max_context = settings.agent_context_length as usize;
        let max_turns = settings.agent_max_turns as usize;
        let browser_config = rline_ai::tools::BrowserConfig {
            runtime: rline_ai::ai_runtime().handle().clone(),
            viewport: (
                settings.agent_browser_viewport_width,
                settings.agent_browser_viewport_height,
            ),
            multimodal: settings.agent_multimodal,
        };
        // Spawn the agent loop on the AI runtime, reusing context if available.
        let ws_root2 = workspace_root.clone();
        let ws_root3 = workspace_root.clone();
        let ctx_arc = self.conversation_context.clone();
        let hist_arc = self.history_file.clone();
        rline_ai::ai_runtime().spawn(async move {
            // Start MCP servers and discover tools.
            let global_mcp_path = rline_config::paths::mcp_config_path().ok();
            let mcp_manager = rline_ai::mcp::manager::McpManager::from_workspace(
                global_mcp_path.as_deref(),
                &ws_root2,
            )
            .await;
            let (mcp_tools, mcp_mgr_arc, mcp_tool_summary) = match mcp_manager {
                Ok(mgr) => {
                    let tools = mgr.discover_tools().await;
                    let summary = rline_ai::mcp::manager::build_tool_summary(&tools);
                    let arc = if mgr.has_servers() {
                        Some(std::sync::Arc::new(tokio::sync::Mutex::new(mgr)))
                    } else {
                        None
                    };
                    (tools, arc, summary)
                }
                Err(e) => {
                    tracing::warn!("failed to start MCP servers: {e}");
                    (Vec::new(), None, None)
                }
            };

            let existing_ctx = ctx_arc.lock().unwrap_or_else(|e| e.into_inner()).take();

            let agent = match existing_ctx {
                Some(ctx) => AgentLoop::with_context(
                    client,
                    mode,
                    ctx,
                    event_tx,
                    auto_approve,
                    cancel,
                    ws_root2,
                    max_tokens,
                    temperature,
                    max_turns,
                    mcp_tools,
                    mcp_mgr_arc,
                    Some(browser_config.clone()),
                ),
                None => {
                    // Load custom system prompt from config dir if it exists.
                    // Replace [current_working_directory] placeholder with the
                    // actual workspace root so users can reference it in their prompt.
                    let custom_prompt = rline_config::paths::system_prompt_path()
                        .ok()
                        .and_then(|p| std::fs::read_to_string(p).ok())
                        .filter(|s| !s.trim().is_empty())
                        .map(|s| {
                            s.replace("[current_working_directory]", &ws_root2.to_string_lossy())
                        });

                    AgentLoop::new(
                        client,
                        mode,
                        event_tx,
                        auto_approve,
                        cancel,
                        ws_root2,
                        max_tokens,
                        temperature,
                        max_context,
                        custom_prompt,
                        max_turns,
                        mcp_tools,
                        mcp_mgr_arc,
                        mcp_tool_summary,
                        Some(browser_config),
                    )
                }
            };
            let ctx = agent.run(user_message).await;
            // Save conversation history to .agent-history/ (creates or updates file).
            save_conversation_history(&ws_root3, &ctx, &hist_arc);
            // Store the context back for the next send.
            *ctx_arc.lock().unwrap_or_else(|e| e.into_inner()) = Some(ctx);
        });

        // Poll for events on the GTK main loop.
        let messages_box = self.messages_box.clone();
        let scrolled = self.scrolled.clone();
        let is_running = self.is_running.clone();
        let stop_btn = self.stop_button.clone();
        let continue_btn = self.continue_button.clone();
        let send_btn = self.send_button.clone();
        let input_view = self.input_view.clone();
        let current_label = self.current_ai_label.clone();
        let pending = self.pending_tool_widgets.clone();
        let accumulated_text = Rc::new(RefCell::new(String::new()));
        let text_run_start = Rc::new(RefCell::new(Instant::now()));
        let on_diff = self.on_open_diff.clone();
        let on_terminal_cmd = self.on_terminal_command.clone();
        let ctx_label = self.context_label.clone();
        let working_indicator = self.working_indicator.clone();
        let working_timer = self.working_timer.clone();
        let last_tool_block = self.last_tool_block.clone();

        glib::idle_add_local(move || {
            match event_rx.try_recv() {
                Ok(event) => {
                    handle_event(
                        event,
                        &messages_box,
                        &scrolled,
                        &current_label,
                        &pending,
                        &accumulated_text,
                        &text_run_start,
                        &on_diff,
                        &on_terminal_cmd,
                        &ctx_label,
                        &working_indicator,
                        &working_timer,
                        &last_tool_block,
                    );
                    glib::ControlFlow::Continue
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Agent loop ended — finalize any remaining text.
                    finalize_markdown(
                        &current_label,
                        &accumulated_text,
                        &text_run_start,
                        &messages_box,
                    );
                    remove_working_indicator(&working_indicator, &working_timer);
                    *is_running.borrow_mut() = false;
                    stop_btn.set_sensitive(false);
                    stop_btn.remove_css_class("error");
                    continue_btn.set_sensitive(true);
                    continue_btn.add_css_class("success");
                    send_btn.set_sensitive(true);
                    input_view.set_sensitive(true);
                    glib::ControlFlow::Break
                }
            }
        });
    }
}

/// Handle a single agent event on the GTK main thread.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn handle_event(
    event: AgentEvent,
    messages_box: &gtk4::Box,
    scrolled: &gtk4::ScrolledWindow,
    current_label: &Rc<RefCell<Option<gtk4::Label>>>,
    pending: &Rc<RefCell<Vec<(String, gtk4::Box, gtk4::Box)>>>,
    accumulated_text: &Rc<RefCell<String>>,
    text_run_start: &Rc<RefCell<Instant>>,
    on_open_diff: &Rc<RefCell<Option<Box<dyn Fn(&std::path::Path)>>>>,
    #[allow(clippy::type_complexity)] on_terminal_command: &Rc<
        RefCell<
            Option<
                Box<
                    dyn Fn(
                        &str,
                        &std::path::Path,
                        u64,
                        tokio::sync::oneshot::Sender<(bool, String)>,
                    ),
                >,
            >,
        >,
    >,
    context_label: &gtk4::Label,
    working_indicator: &Rc<RefCell<Option<gtk4::Box>>>,
    working_timer: &Rc<RefCell<Option<glib::SourceId>>>,
    last_tool_block: &Rc<RefCell<Option<LastToolBlock>>>,
) {
    match event {
        AgentEvent::TextDelta(delta) => {
            // Remove "Working..." on first text delta.
            remove_working_indicator(working_indicator, working_timer);

            let mut acc = accumulated_text.borrow_mut();
            if acc.is_empty() {
                // Start timing this text run.
                *text_run_start.borrow_mut() = Instant::now();
            }
            acc.push_str(&delta);

            // Don't show streaming text while inside a thinking block —
            // it will be collapsed into "Thought for X seconds" on finalize.
            let acc_ref = &*acc;
            if acc_ref.contains("<thinking>") && !acc_ref.contains("</thinking>") {
                // Still inside a thinking block — skip updating the label.
                return;
            }

            // Lazily create the AI message widget on first text delta.
            if current_label.borrow().is_none() {
                let (ai_container, ai_label) = message_widget::build_ai_message();
                messages_box.append(&ai_container);
                *current_label.borrow_mut() = Some(ai_label);
            }

            // Show the non-thinking portion as plain text while streaming.
            let display = strip_thinking_blocks(acc_ref);
            if let Some(label) = current_label.borrow().as_ref() {
                label.set_text(&display);
            }
            scroll_to_bottom(scrolled);
        }
        AgentEvent::ToolCallStart {
            id,
            name,
            arguments,
        } => {
            // Remove "Working..." on tool call start.
            remove_working_indicator(working_indicator, working_timer);

            // Finalize the current AI text with markdown formatting.
            finalize_markdown(
                current_label,
                accumulated_text,
                text_run_start,
                messages_box,
            );

            // If this tool call is identical to the most recently created
            // card, reuse that card instead of spawning a new one — bump the
            // repeat badge and map the new id onto the existing boxes. This
            // collapses runaway loops (e.g. repeated scroll_down) into a
            // single visual block.
            let matched = {
                let existing = last_tool_block.borrow();
                existing
                    .as_ref()
                    .filter(|b| b.name == name && b.arguments == arguments)
                    .cloned()
            };

            if let Some(block) = matched {
                let next = {
                    let mut n = block.count.borrow_mut();
                    *n += 1;
                    *n
                };
                message_widget::set_tool_call_repeat_count(&block.count_label, next);
                pending
                    .borrow_mut()
                    .push((id, block.result_box.clone(), block.button_box.clone()));
            } else {
                let widget = message_widget::build_tool_call(&name, &arguments);
                messages_box.append(&widget.container);
                pending.borrow_mut().push((
                    id.clone(),
                    widget.result_box.clone(),
                    widget.button_box.clone(),
                ));
                *last_tool_block.borrow_mut() = Some(LastToolBlock {
                    name,
                    arguments,
                    count_label: widget.count_label,
                    result_box: widget.result_box,
                    button_box: widget.button_box,
                    count: Rc::new(RefCell::new(1)),
                });
                // id is consumed in the push above; discard the variable so
                // the borrow-checker is happy with the separate branches.
                let _ = id;
            }

            scroll_to_bottom(scrolled);
        }
        AgentEvent::ApprovalNeeded {
            id,
            name,
            arguments: _,
            category: _,
            respond,
        } => {
            // Find the pending widget for this tool call.
            let pending_ref = pending.borrow();
            let entry = pending_ref.iter().find(|(tid, _, _)| *tid == id);

            if let Some((_, _, button_box)) = entry {
                let approve_btn = gtk4::Button::with_label("Approve");
                approve_btn.add_css_class("suggested-action");
                let deny_btn = gtk4::Button::with_label("Deny");
                deny_btn.add_css_class("destructive-action");

                button_box.append(&approve_btn);
                button_box.append(&deny_btn);

                // Wrap the oneshot sender in Rc<RefCell<Option<>>> so both
                // buttons can attempt to use it (only one will succeed).
                let sender = Rc::new(RefCell::new(Some(respond)));
                let bb = button_box.clone();

                let sender_clone = sender.clone();
                let bb_clone = bb.clone();
                approve_btn.connect_clicked(move |_| {
                    if let Some(tx) = sender_clone.borrow_mut().take() {
                        let _ = tx.send(true);
                    }
                    // Remove buttons.
                    while let Some(child) = bb_clone.first_child() {
                        bb_clone.remove(&child);
                    }
                });

                let sender_clone = sender;
                deny_btn.connect_clicked(move |_| {
                    if let Some(tx) = sender_clone.borrow_mut().take() {
                        let _ = tx.send(false);
                    }
                    while let Some(child) = bb.first_child() {
                        bb.remove(&child);
                    }
                });
            } else {
                // No matching widget — just approve to avoid blocking.
                warn!("no pending widget for tool call {id} ({name}), auto-approving");
                let _ = respond.send(true);
            }

            scroll_to_bottom(scrolled);
        }
        AgentEvent::ToolResult {
            id,
            success,
            output,
            image_png,
            ..
        } => {
            let pending_ref = pending.borrow();
            let entry = pending_ref.iter().find(|(tid, _, _)| *tid == id);
            if let Some((_, result_box, _)) = entry {
                message_widget::add_tool_result(result_box, success, &output, image_png.as_deref());
            }
            scroll_to_bottom(scrolled);
        }
        AgentEvent::FileChanged { path } => {
            if let Some(cb) = on_open_diff.borrow().as_ref() {
                cb(&path);
            }
        }
        AgentEvent::TerminalCommand {
            command,
            working_dir,
            timeout_secs,
            respond,
            ..
        } => {
            if let Some(cb) = on_terminal_command.borrow().as_ref() {
                cb(&command, &working_dir, timeout_secs, respond);
            } else {
                // No terminal available — fall back to error.
                warn!("no terminal command handler, failing command");
                let _ = respond.send((
                    false,
                    "No terminal available for command execution.".to_owned(),
                ));
            }
        }
        AgentEvent::FollowupQuestion { question, respond } => {
            remove_working_indicator(working_indicator, working_timer);
            let (container, text_view, submit) = message_widget::build_followup_question(&question);
            messages_box.append(&container);

            let sender = Rc::new(RefCell::new(Some(respond)));

            // Submit on button click.
            let tv_clone = text_view.clone();
            let sender_clone = sender.clone();
            submit.connect_clicked(move |btn| {
                let buf = tv_clone.buffer();
                let answer = buf
                    .text(&buf.start_iter(), &buf.end_iter(), false)
                    .to_string();
                if let Some(tx) = sender_clone.borrow_mut().take() {
                    let _ = tx.send(answer);
                }
                btn.set_sensitive(false);
                tv_clone.set_sensitive(false);
            });

            // Submit on Enter (Shift+Enter for newline).
            let tv_clone = text_view.clone();
            let sender_clone = sender;
            let submit_clone = submit.clone();
            let key_ctrl = gtk4::EventControllerKey::new();
            key_ctrl.connect_key_pressed(move |_, key, _, mods| {
                if key == gtk4::gdk::Key::Return
                    && !mods.contains(gtk4::gdk::ModifierType::SHIFT_MASK)
                {
                    let buf = tv_clone.buffer();
                    let answer = buf
                        .text(&buf.start_iter(), &buf.end_iter(), false)
                        .to_string();
                    if let Some(tx) = sender_clone.borrow_mut().take() {
                        let _ = tx.send(answer);
                    }
                    submit_clone.set_sensitive(false);
                    tv_clone.set_sensitive(false);
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            text_view.add_controller(key_ctrl);

            scroll_to_bottom(scrolled);
        }
        AgentEvent::ContextUpdate {
            used_tokens,
            max_tokens,
        } => {
            let used_k = used_tokens / 1000;
            let max_k = max_tokens / 1000;
            context_label.set_text(&format!("{used_k}k / {max_k}k"));
        }
        AgentEvent::TurnComplete => {
            // Finalize markdown on the completed AI text.
            finalize_markdown(
                current_label,
                accumulated_text,
                text_run_start,
                messages_box,
            );
            // Show "Working..." while waiting for the next model turn.
            show_working_indicator(messages_box, scrolled, working_indicator, working_timer);
            scroll_to_bottom(scrolled);
        }
        AgentEvent::Error(msg) => {
            remove_working_indicator(working_indicator, working_timer);
            let widget = message_widget::build_error(&msg);
            messages_box.append(&widget);
            scroll_to_bottom(scrolled);
        }
        AgentEvent::Finished { summary, plan_mode } => {
            remove_working_indicator(working_indicator, working_timer);
            // Finalize markdown on any remaining AI text.
            finalize_markdown(
                current_label,
                accumulated_text,
                text_run_start,
                messages_box,
            );

            if let Some(summary) = summary {
                let widget = message_widget::build_completion(&summary);
                messages_box.append(&widget);
            }

            if plan_mode {
                let prompt = message_widget::build_plan_mode_prompt();
                messages_box.append(&prompt);
            }

            pending.borrow_mut().clear();
            scroll_to_bottom(scrolled);
        }
    }
}

/// Apply markdown formatting to the current AI label and reset the accumulator.
///
/// Called when a text run ends (tool call starts, turn completes, or the agent
/// finishes). Falls back to plain text if Pango markup parsing fails.
///
/// If the accumulated text contains `<thinking>...</thinking>` blocks, they are
/// extracted into collapsible "Thought for X seconds" widgets. The AI label's
/// parent container is repurposed: thinking widgets are inserted before the label,
/// and only the non-thinking text is rendered in the label itself.
fn finalize_markdown(
    current_label: &Rc<RefCell<Option<gtk4::Label>>>,
    accumulated_text: &Rc<RefCell<String>>,
    text_run_start: &Rc<RefCell<Instant>>,
    messages_box: &gtk4::Box,
) {
    let text = accumulated_text.borrow().clone();
    if text.is_empty() {
        *current_label.borrow_mut() = None;
        return;
    }

    let elapsed_secs = text_run_start.borrow().elapsed().as_secs();
    let (thinking_blocks, visible_text) = extract_thinking_blocks(&text);

    if thinking_blocks.is_empty() {
        // No thinking blocks — simple path: apply markdown to the label.
        if let Some(label) = current_label.borrow().as_ref() {
            let pango = super::markdown::markdown_to_pango(&text);
            label.set_markup(&pango);
        }
    } else {
        // Has thinking blocks. Get the AI message container (label's parent Box).
        let ai_container = current_label
            .borrow()
            .as_ref()
            .and_then(|l| l.parent())
            .and_then(|p| p.downcast::<gtk4::Box>().ok());

        if let Some(container) = ai_container {
            // The container is a vertical Box with [header_label, content_label].
            // Insert thinking widgets between header and content label.
            let header = container.first_child();

            for content in &thinking_blocks {
                let widget = message_widget::build_thinking_block(content, elapsed_secs);
                container.insert_child_after(&widget, header.as_ref());
            }
        } else {
            // Fallback: append thinking blocks directly to messages_box.
            for content in &thinking_blocks {
                let widget = message_widget::build_thinking_block(content, elapsed_secs);
                messages_box.append(&widget);
            }
        }

        // Update the label with just the visible (non-thinking) text.
        if let Some(label) = current_label.borrow().as_ref() {
            let trimmed = visible_text.trim();
            if trimmed.is_empty() {
                label.set_visible(false);
            } else {
                let pango = super::markdown::markdown_to_pango(trimmed);
                label.set_markup(&pango);
            }
        }
    }

    accumulated_text.borrow_mut().clear();
    *current_label.borrow_mut() = None;
}

/// Extract `<thinking>...</thinking>` blocks from text.
///
/// Returns a vector of thinking block contents and the remaining visible text
/// with thinking blocks removed.
fn extract_thinking_blocks(text: &str) -> (Vec<String>, String) {
    let mut blocks = Vec::new();
    let mut visible = String::with_capacity(text.len());
    let mut remaining = text;

    loop {
        match remaining.find("<thinking>") {
            Some(start) => {
                // Text before the thinking block is visible.
                visible.push_str(&remaining[..start]);

                let after_open = &remaining[start + "<thinking>".len()..];
                match after_open.find("</thinking>") {
                    Some(end) => {
                        blocks.push(after_open[..end].trim().to_owned());
                        remaining = &after_open[end + "</thinking>".len()..];
                    }
                    None => {
                        // Unclosed thinking block — treat the rest as thinking.
                        blocks.push(after_open.trim().to_owned());
                        break;
                    }
                }
            }
            None => {
                visible.push_str(remaining);
                break;
            }
        }
    }

    (blocks, visible)
}

/// Strip thinking blocks from text for display during streaming.
fn strip_thinking_blocks(text: &str) -> String {
    let (_, visible) = extract_thinking_blocks(text);
    visible
}

/// Show the "Working..." indicator in the messages box with animated dots.
fn show_working_indicator(
    messages_box: &gtk4::Box,
    scrolled: &gtk4::ScrolledWindow,
    working_indicator: &Rc<RefCell<Option<gtk4::Box>>>,
    working_timer: &Rc<RefCell<Option<glib::SourceId>>>,
) {
    // Remove any existing indicator first.
    remove_working_indicator(working_indicator, working_timer);

    let (container, label) = message_widget::build_working_indicator();
    messages_box.append(&container);
    *working_indicator.borrow_mut() = Some(container);
    scroll_to_bottom(scrolled);

    // Cycle dots every 500ms: "Working." → "Working.." → "Working..." → repeat.
    let dot_count = Rc::new(RefCell::new(1u8));
    let source_id = glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        let mut count = dot_count.borrow_mut();
        *count = (*count % 3) + 1;
        let dots = ".".repeat(*count as usize);
        label.set_markup(&format!("<i>Working{dots}</i>"));
        glib::ControlFlow::Continue
    });
    *working_timer.borrow_mut() = Some(source_id);
}

/// Remove the "Working..." indicator and stop its animation timer.
fn remove_working_indicator(
    working_indicator: &Rc<RefCell<Option<gtk4::Box>>>,
    working_timer: &Rc<RefCell<Option<glib::SourceId>>>,
) {
    if let Some(timer_id) = working_timer.borrow_mut().take() {
        timer_id.remove();
    }
    if let Some(widget) = working_indicator.borrow_mut().take() {
        if let Some(parent) = widget.parent() {
            if let Some(parent_box) = parent.downcast_ref::<gtk4::Box>() {
                parent_box.remove(&widget);
            }
        }
    }
}

/// Scroll a `ScrolledWindow` to the bottom.
fn scroll_to_bottom(scrolled: &gtk4::ScrolledWindow) {
    let adj = scrolled.vadjustment();
    // Schedule the scroll for the next idle cycle so the layout has updated.
    glib::idle_add_local_once(move || {
        adj.set_value(adj.upper() - adj.page_size());
    });
}

/// Save (or update) the conversation history file for the current task.
///
/// On the first call for a conversation, creates a new timestamped file in
/// `.agent-history/`. On subsequent calls, overwrites the same file with the
/// full updated conversation.
fn save_conversation_history(
    workspace_root: &std::path::Path,
    context: &rline_ai::agent::context::ConversationContext,
    history_file: &Arc<Mutex<Option<PathBuf>>>,
) {
    if context.message_count() == 0 {
        return;
    }

    let history_dir = workspace_root.join(".agent-history");
    if let Err(e) = std::fs::create_dir_all(&history_dir) {
        tracing::warn!("failed to create .agent-history directory: {e}");
        return;
    }

    // Reuse existing file path, or create a new one for this conversation.
    let mut guard = history_file.lock().unwrap_or_else(|e| e.into_inner());
    let path = match guard.as_ref() {
        Some(p) => p.clone(),
        None => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            let secs = now.as_secs();
            let (year, month, day, hour, min, sec) = epoch_to_datetime(secs);
            let filename = format!("{year:04}-{month:02}-{day:02}_{hour:02}-{min:02}-{sec:02}.md");
            let p = history_dir.join(filename);
            *guard = Some(p.clone());
            p
        }
    };
    drop(guard); // Release the lock before doing I/O.

    let content = context.to_markdown();

    if let Err(e) = std::fs::write(&path, &content) {
        tracing::warn!("failed to save conversation history: {e}");
    } else {
        tracing::debug!("saved conversation history to {}", path.display());
    }
}

/// Convert Unix epoch seconds to (year, month, day, hour, minute, second) in UTC.
///
/// Simple implementation without pulling in a datetime crate.
fn epoch_to_datetime(epoch: u64) -> (u64, u64, u64, u64, u64, u64) {
    let sec = epoch % 60;
    let min = (epoch / 60) % 60;
    let hour = (epoch / 3600) % 24;

    let mut days = epoch / 86400;
    let mut year = 1970;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_days: [u64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if days < md {
            month = i as u64 + 1;
            break;
        }
        days -= md;
    }

    let day = days + 1;
    (year, month, day, hour, min, sec)
}

/// Whether a year is a leap year.
fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
