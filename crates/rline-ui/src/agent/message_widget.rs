//! Widget factories for conversation messages in the agent panel.
//!
//! Each factory function creates a styled GTK widget representing a
//! different type of message (user, AI, tool call, completion).

use gtk4::prelude::*;

/// Build a user message widget.
pub fn build_user_message(text: &str) -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    container.set_margin_start(8);
    container.set_margin_end(8);
    container.set_margin_top(4);
    container.set_margin_bottom(4);

    let header = gtk4::Label::new(Some("You"));
    header.set_halign(gtk4::Align::Start);
    header.set_markup("<b>You</b>");
    container.append(&header);

    let label = gtk4::Label::new(Some(text));
    label.set_halign(gtk4::Align::Start);
    label.set_wrap(true);
    label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.set_xalign(0.0);
    container.append(&label);

    container
}

/// Build an AI message widget.
///
/// Returns the container and the content label (for streaming updates).
pub fn build_ai_message() -> (gtk4::Box, gtk4::Label) {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    container.set_margin_start(8);
    container.set_margin_end(8);
    container.set_margin_top(4);
    container.set_margin_bottom(4);

    let header = gtk4::Label::new(None);
    header.set_halign(gtk4::Align::Start);
    header.set_markup("<b>Assistant</b>");
    container.append(&header);

    let label = gtk4::Label::new(None);
    label.set_halign(gtk4::Align::Start);
    label.set_wrap(true);
    label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.set_xalign(0.0);
    container.append(&label);

    (container, label)
}

/// A tool call widget with collapsible detail and optional approval buttons.
pub struct ToolCallWidget {
    /// The outer container.
    pub container: gtk4::Box,
    /// Box where the tool result will be added.
    pub result_box: gtk4::Box,
    /// Box for approval buttons.
    pub button_box: gtk4::Box,
    /// The detail revealer (retained for programmatic expand/collapse).
    #[allow(dead_code)]
    pub revealer: gtk4::Revealer,
}

/// Build a tool call widget.
///
/// Displays the tool name as a header with a toggle to expand/collapse
/// the arguments and result. Includes a slot for approve/deny buttons.
pub fn build_tool_call(name: &str, arguments: &str) -> ToolCallWidget {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_margin_start(8);
    container.set_margin_end(8);
    container.set_margin_top(0);
    container.set_margin_bottom(0);

    // Header with toggle.
    let header_btn = gtk4::Button::new();
    header_btn.add_css_class("flat");
    let header_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    header_box.set_margin_start(4);
    header_box.set_margin_end(4);

    let arrow = gtk4::Label::new(Some("\u{25B6}")); // ▶
    header_box.append(&arrow);

    let summary = tool_call_summary(name, arguments);
    let name_label = gtk4::Label::new(None);
    name_label.set_markup(&summary);
    name_label.set_halign(gtk4::Align::Start);
    name_label.set_hexpand(true);
    name_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    header_box.append(&name_label);

    header_btn.set_child(Some(&header_box));
    container.append(&header_btn);

    // Revealer for detail content.
    let revealer = gtk4::Revealer::new();
    revealer.set_reveal_child(false);
    revealer.set_transition_type(gtk4::RevealerTransitionType::SlideDown);

    let detail_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    detail_box.set_margin_start(8);
    detail_box.set_margin_end(8);
    detail_box.set_margin_bottom(2);

    // Arguments display.
    if !arguments.is_empty() {
        // Try to pretty-print JSON arguments.
        let display_args = match serde_json::from_str::<serde_json::Value>(arguments) {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| arguments.to_owned()),
            Err(_) => arguments.to_owned(),
        };

        let args_label = gtk4::Label::new(Some(&display_args));
        args_label.set_halign(gtk4::Align::Start);
        args_label.set_wrap(true);
        args_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        args_label.set_selectable(true);
        args_label.set_xalign(0.0);
        args_label.add_css_class("monospace");
        detail_box.append(&args_label);
    }

    // Result area (populated later).
    let result_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    detail_box.append(&result_box);

    revealer.set_child(Some(&detail_box));
    container.append(&revealer);

    // Approval button box (populated when needed).
    let button_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    button_box.set_margin_start(8);
    button_box.set_margin_end(8);
    button_box.set_margin_bottom(2);
    button_box.set_halign(gtk4::Align::Start);
    container.append(&button_box);

    // Toggle revealer on header click.
    let rev_clone = revealer.clone();
    let arrow_clone = arrow.clone();
    header_btn.connect_clicked(move |_| {
        let revealed = rev_clone.reveals_child();
        rev_clone.set_reveal_child(!revealed);
        arrow_clone.set_text(if revealed { "\u{25B6}" } else { "\u{25BC}" });
    });

    ToolCallWidget {
        container,
        result_box,
        button_box,
        revealer,
    }
}

/// Add a tool result display to an existing tool call widget.
pub fn add_tool_result(result_box: &gtk4::Box, success: bool, output: &str) {
    // Clear any previous content.
    while let Some(child) = result_box.first_child() {
        result_box.remove(&child);
    }

    let sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    sep.set_margin_top(4);
    sep.set_margin_bottom(4);
    result_box.append(&sep);

    let status_text = if success { "Success" } else { "Failed" };
    let status_label = gtk4::Label::new(None);
    status_label.set_markup(&format!("<small><i>{status_text}</i></small>"));
    status_label.set_halign(gtk4::Align::Start);
    result_box.append(&status_label);

    // Truncate very long output for display.
    let display_output = if output.len() > 2000 {
        format!("{}...\n(truncated)", &output[..2000])
    } else {
        output.to_owned()
    };

    let output_label = gtk4::Label::new(Some(&display_output));
    output_label.set_halign(gtk4::Align::Start);
    output_label.set_wrap(true);
    output_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    output_label.set_selectable(true);
    output_label.set_xalign(0.0);
    output_label.add_css_class("monospace");
    result_box.append(&output_label);
}

/// Build a task completion widget.
pub fn build_completion(summary: &str) -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    container.set_margin_start(8);
    container.set_margin_end(8);
    container.set_margin_top(4);
    container.set_margin_bottom(4);
    container.add_css_class("card");

    let header = gtk4::Label::new(None);
    header.set_markup("<b>Task Complete</b>");
    header.set_halign(gtk4::Align::Start);
    header.set_margin_start(4);
    header.set_margin_top(4);
    container.append(&header);

    let label = gtk4::Label::new(Some(summary));
    let pango = super::markdown::markdown_to_pango(summary);
    label.set_markup(&pango);
    label.set_halign(gtk4::Align::Start);
    label.set_wrap(true);
    label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.set_xalign(0.0);
    label.set_margin_start(4);
    label.set_margin_end(4);
    label.set_margin_bottom(4);
    container.append(&label);

    container
}

/// Build a follow-up question widget with a multi-line text area and submit button.
///
/// Returns the container, the text view (for reading the answer), and the submit button.
pub fn build_followup_question(question: &str) -> (gtk4::Box, gtk4::TextView, gtk4::Button) {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    container.set_margin_start(8);
    container.set_margin_end(8);
    container.set_margin_top(4);
    container.set_margin_bottom(4);
    container.add_css_class("card");

    let header = gtk4::Label::new(None);
    header.set_markup("<b>Question from Agent</b>");
    header.set_halign(gtk4::Align::Start);
    header.set_margin_start(4);
    header.set_margin_top(4);
    container.append(&header);

    let question_label = gtk4::Label::new(Some(question));
    let pango = super::markdown::markdown_to_pango(question);
    question_label.set_markup(&pango);
    question_label.set_halign(gtk4::Align::Start);
    question_label.set_wrap(true);
    question_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    question_label.set_xalign(0.0);
    question_label.set_margin_start(4);
    question_label.set_margin_end(4);
    container.append(&question_label);

    let input_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    input_box.set_margin_start(4);
    input_box.set_margin_end(4);
    input_box.set_margin_bottom(4);

    let text_view = gtk4::TextView::new();
    text_view.set_wrap_mode(gtk4::WrapMode::Word);
    text_view.set_left_margin(4);
    text_view.set_right_margin(4);
    text_view.set_top_margin(4);
    text_view.set_bottom_margin(4);
    text_view.set_size_request(-1, 60);

    let tv_frame = gtk4::Frame::new(None);
    tv_frame.set_child(Some(&text_view));
    input_box.append(&tv_frame);

    let submit = gtk4::Button::with_label("Submit");
    submit.add_css_class("suggested-action");
    input_box.append(&submit);

    container.append(&input_box);

    // Focus the text view so the user can start typing immediately.
    text_view.grab_focus();

    (container, text_view, submit)
}

/// Build a prompt shown after Plan mode completes, telling the user to switch to Act mode.
pub fn build_plan_mode_prompt() -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    container.set_margin_start(8);
    container.set_margin_end(8);
    container.set_margin_top(8);
    container.set_margin_bottom(4);
    container.add_css_class("card");

    let label = gtk4::Label::new(None);
    label.set_markup(
        "<b>Plan complete.</b> Switch to <b>Act</b> mode and instruct the agent to proceed \
         with the plan.",
    );
    label.set_halign(gtk4::Align::Start);
    label.set_wrap(true);
    label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    label.set_xalign(0.0);
    label.set_margin_start(8);
    label.set_margin_end(8);
    label.set_margin_top(8);
    label.set_margin_bottom(8);
    container.append(&label);

    container
}

/// Build a descriptive header for a tool call, extracting key info from arguments.
///
/// Instead of just showing "read_file", shows "read_file — src/main.rs".
fn tool_call_summary(name: &str, arguments: &str) -> String {
    let detail = extract_tool_detail(name, arguments);
    let escaped_name = glib::markup_escape_text(name);
    match detail {
        Some(d) => format!(
            "<b>{escaped_name}</b>  <small>{}</small>",
            glib::markup_escape_text(&d)
        ),
        None => format!("<b>{escaped_name}</b>"),
    }
}

/// Extract a human-readable detail string from tool arguments.
fn extract_tool_detail(name: &str, arguments: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(arguments).ok()?;

    match name {
        "read_file" | "write_to_file" | "list_files" | "list_code_definition_names" => {
            v.get("path").and_then(|p| p.as_str()).map(String::from)
        }
        "replace_in_file" => v.get("path").and_then(|p| p.as_str()).map(String::from),
        "search_files" => {
            let path = v.get("path").and_then(|p| p.as_str()).unwrap_or(".");
            let regex = v.get("regex").and_then(|r| r.as_str()).unwrap_or("?");
            Some(format!("{path} — /{regex}/"))
        }
        "execute_command" => v.get("command").and_then(|c| c.as_str()).map(|c| {
            if c.len() > 60 {
                format!("{}...", &c[..57])
            } else {
                c.to_owned()
            }
        }),
        "ask_followup_question" => v.get("question").and_then(|q| q.as_str()).map(|q| {
            if q.len() > 60 {
                format!("{}...", &q[..57])
            } else {
                q.to_owned()
            }
        }),
        "attempt_completion" => Some("task complete".to_owned()),
        _ => None,
    }
}

/// Build a collapsible thinking block widget.
///
/// Shows "Thought for N seconds" as a clickable header with the thinking
/// content hidden behind a revealer.
pub fn build_thinking_block(content: &str, duration_secs: u64) -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.set_margin_start(8);
    container.set_margin_end(8);
    container.set_margin_top(2);
    container.set_margin_bottom(2);

    let header_btn = gtk4::Button::new();
    header_btn.add_css_class("flat");
    let header_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    header_box.set_margin_start(4);
    header_box.set_margin_end(4);

    let arrow = gtk4::Label::new(Some("\u{25B6}")); // ▶
    header_box.append(&arrow);

    let label_text = if duration_secs == 1 {
        "Thought for 1 second".to_owned()
    } else {
        format!("Thought for {duration_secs} seconds")
    };
    let name_label = gtk4::Label::new(None);
    name_label.set_markup(&format!(
        "<small><i>{}</i></small>",
        glib::markup_escape_text(&label_text)
    ));
    name_label.set_halign(gtk4::Align::Start);
    header_box.append(&name_label);

    header_btn.set_child(Some(&header_box));
    container.append(&header_btn);

    let revealer = gtk4::Revealer::new();
    revealer.set_reveal_child(false);
    revealer.set_transition_type(gtk4::RevealerTransitionType::SlideDown);

    let content_label = gtk4::Label::new(Some(content));
    let pango = super::markdown::markdown_to_pango(content);
    content_label.set_markup(&pango);
    content_label.set_halign(gtk4::Align::Start);
    content_label.set_wrap(true);
    content_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    content_label.set_selectable(true);
    content_label.set_xalign(0.0);
    content_label.set_margin_start(8);
    content_label.set_margin_end(8);
    content_label.set_margin_bottom(4);
    revealer.set_child(Some(&content_label));
    container.append(&revealer);

    let rev_clone = revealer.clone();
    let arrow_clone = arrow.clone();
    header_btn.connect_clicked(move |_| {
        let revealed = rev_clone.reveals_child();
        rev_clone.set_reveal_child(!revealed);
        arrow_clone.set_text(if revealed { "\u{25B6}" } else { "\u{25BC}" });
    });

    container
}

/// Build a "Working..." indicator label.
///
/// Returns the container box and the label (for updating the dot animation).
pub fn build_working_indicator() -> (gtk4::Box, gtk4::Label) {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    container.set_margin_start(8);
    container.set_margin_end(8);
    container.set_margin_top(4);
    container.set_margin_bottom(4);

    let header = gtk4::Label::new(None);
    header.set_markup("<b>Assistant</b>");
    header.set_halign(gtk4::Align::Start);
    container.append(&header);

    let label = gtk4::Label::new(None);
    label.set_markup("<i>Working.</i>");
    label.set_halign(gtk4::Align::Start);
    label.set_xalign(0.0);
    container.append(&label);

    (container, label)
}

/// Build an error message widget.
pub fn build_error(message: &str) -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    container.set_margin_start(8);
    container.set_margin_end(8);
    container.set_margin_top(4);
    container.set_margin_bottom(4);

    let label = gtk4::Label::new(None);
    label.set_markup(&format!(
        "<span foreground=\"red\"><b>Error:</b> {}</span>",
        glib::markup_escape_text(message)
    ));
    label.set_halign(gtk4::Align::Start);
    label.set_wrap(true);
    label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    label.set_xalign(0.0);
    container.append(&label);

    container
}
