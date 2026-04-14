//! Widget factories for conversation messages in the agent panel.
//!
//! Each factory function creates a styled GTK widget representing a
//! different type of message (user, AI, tool call, completion).

use gtk4::prelude::*;

/// Build a header row with a small role icon and a name label.
fn build_role_header(icon: &str, name: &str) -> gtk4::Box {
    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    header.add_css_class("agent-message-header");
    header.set_halign(gtk4::Align::Start);

    let image = gtk4::Image::from_icon_name(icon);
    header.append(&image);

    let label = gtk4::Label::new(Some(name));
    label.set_halign(gtk4::Align::Start);
    header.append(&label);

    header
}

/// Build a user message widget.
pub fn build_user_message(text: &str) -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    container.add_css_class("agent-message");
    container.add_css_class("agent-message-user");

    container.append(&build_role_header("avatar-default-symbolic", "You"));

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
    container.add_css_class("agent-message");
    container.add_css_class("agent-message-ai");

    container.append(&build_role_header(
        "applications-science-symbolic",
        "Assistant",
    ));

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
    /// Label in the header that displays a `×N` repeat count when the same
    /// tool call is issued several times in a row. Hidden by default.
    pub count_label: gtk4::Label,
}

/// Build a tool call widget.
///
/// Displays the tool name as a header with a toggle to expand/collapse
/// the arguments and result. Includes a slot for approve/deny buttons.
pub fn build_tool_call(name: &str, arguments: &str) -> ToolCallWidget {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.add_css_class("agent-tool-card");

    // Header with toggle.
    let header_btn = gtk4::Button::new();
    header_btn.add_css_class("flat");
    header_btn.add_css_class("agent-tool-card-header");
    let header_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);

    let arrow = gtk4::Image::from_icon_name("pan-end-symbolic");
    arrow.add_css_class("agent-tool-arrow");
    header_box.append(&arrow);

    let summary = tool_call_summary(name, arguments);
    let name_label = gtk4::Label::new(None);
    name_label.set_markup(&summary);
    name_label.set_halign(gtk4::Align::Start);
    name_label.set_hexpand(true);
    name_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    header_box.append(&name_label);

    let count_label = gtk4::Label::new(None);
    count_label.set_halign(gtk4::Align::End);
    count_label.set_visible(false);
    count_label.add_css_class("dim-label");
    header_box.append(&count_label);

    header_btn.set_child(Some(&header_box));
    container.append(&header_btn);

    // Revealer for detail content.
    let revealer = gtk4::Revealer::new();
    revealer.set_reveal_child(false);
    revealer.set_transition_type(gtk4::RevealerTransitionType::SlideDown);

    let detail_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    detail_box.add_css_class("agent-tool-card-detail");

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
    let result_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    detail_box.append(&result_box);

    revealer.set_child(Some(&detail_box));
    container.append(&revealer);

    // Approval button box (populated when needed).
    let button_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    button_box.set_margin_start(10);
    button_box.set_margin_end(10);
    button_box.set_margin_bottom(6);
    button_box.set_halign(gtk4::Align::Start);
    container.append(&button_box);

    // Toggle revealer on header click.
    let rev_clone = revealer.clone();
    let arrow_clone = arrow.clone();
    header_btn.connect_clicked(move |_| {
        let revealed = rev_clone.reveals_child();
        rev_clone.set_reveal_child(!revealed);
        arrow_clone.set_icon_name(Some(if revealed {
            "pan-end-symbolic"
        } else {
            "pan-down-symbolic"
        }));
    });

    ToolCallWidget {
        container,
        result_box,
        button_box,
        revealer,
        count_label,
    }
}

/// Update a tool call widget's header badge to show the repeat count.
pub fn set_tool_call_repeat_count(count_label: &gtk4::Label, count: usize) {
    if count <= 1 {
        count_label.set_visible(false);
        count_label.set_label("");
    } else {
        count_label.set_markup(&format!("<small>×{count}</small>"));
        count_label.set_visible(true);
    }
}

/// Add a tool result display to an existing tool call widget.
/// Populate the result area of a tool card with the tool's textual output
/// and, optionally, an inline screenshot.
pub fn add_tool_result(
    result_box: &gtk4::Box,
    success: bool,
    output: &str,
    image_png: Option<&[u8]>,
) {
    // Clear any previous content.
    while let Some(child) = result_box.first_child() {
        result_box.remove(&child);
    }

    let status_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    status_row.set_halign(gtk4::Align::Start);
    let status_label = gtk4::Label::new(Some(if success { "Success" } else { "Failed" }));
    status_label.add_css_class("agent-status-pill");
    status_label.add_css_class(if success {
        "agent-status-success"
    } else {
        "agent-status-failed"
    });
    status_row.append(&status_label);
    result_box.append(&status_row);

    // Truncate very long output for display.
    let display_output = if output.len() > 2000 {
        format!("{}...\n(truncated)", &output[..2000])
    } else {
        output.to_owned()
    };

    if !display_output.is_empty() {
        let output_label = gtk4::Label::new(Some(&display_output));
        output_label.set_halign(gtk4::Align::Start);
        output_label.set_wrap(true);
        output_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        output_label.set_selectable(true);
        output_label.set_xalign(0.0);
        output_label.add_css_class("monospace");
        result_box.append(&output_label);
    }

    if let Some(png) = image_png {
        if let Some(picture) = build_screenshot_picture(png) {
            result_box.append(&picture);
        }
    }
}

/// Decode a PNG byte buffer into a `gtk4::Picture`, scaled to fit the card.
fn build_screenshot_picture(png: &[u8]) -> Option<gtk4::Picture> {
    use gtk4::gdk;
    let bytes = glib::Bytes::from(png);
    let texture = gdk::Texture::from_bytes(&bytes).ok()?;
    let picture = gtk4::Picture::for_paintable(&texture);
    picture.set_can_shrink(true);
    picture.set_content_fit(gtk4::ContentFit::Contain);
    picture.set_margin_top(4);
    picture.set_margin_bottom(4);
    picture.set_size_request(-1, 240);
    Some(picture)
}

/// Build a task completion widget.
pub fn build_completion(summary: &str) -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    container.add_css_class("agent-card");
    container.add_css_class("agent-card-completion");

    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    header.add_css_class("agent-message-header");
    header.set_halign(gtk4::Align::Start);
    header.append(&gtk4::Image::from_icon_name("emblem-ok-symbolic"));
    let header_label = gtk4::Label::new(Some("Task Complete"));
    header.append(&header_label);
    container.append(&header);

    let label = gtk4::Label::new(None);
    let pango = super::markdown::markdown_to_pango(summary);
    label.set_markup(&pango);
    label.set_halign(gtk4::Align::Start);
    label.set_wrap(true);
    label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.set_xalign(0.0);
    container.append(&label);

    container
}

/// Build a follow-up question widget with a multi-line text area and submit button.
///
/// Returns the container, the text view (for reading the answer), and the submit button.
pub fn build_followup_question(question: &str) -> (gtk4::Box, gtk4::TextView, gtk4::Button) {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    container.add_css_class("agent-card");
    container.add_css_class("agent-card-question");

    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    header.add_css_class("agent-message-header");
    header.set_halign(gtk4::Align::Start);
    header.append(&gtk4::Image::from_icon_name("dialog-question-symbolic"));
    let header_label = gtk4::Label::new(Some("Question from Agent"));
    header.append(&header_label);
    container.append(&header);

    let question_label = gtk4::Label::new(None);
    let pango = super::markdown::markdown_to_pango(question);
    question_label.set_markup(&pango);
    question_label.set_halign(gtk4::Align::Start);
    question_label.set_wrap(true);
    question_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    question_label.set_xalign(0.0);
    container.append(&question_label);

    let input_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);

    let text_view = gtk4::TextView::new();
    text_view.add_css_class("agent-input");
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
    submit.set_halign(gtk4::Align::End);
    input_box.append(&submit);

    container.append(&input_box);

    // Focus the text view so the user can start typing immediately.
    text_view.grab_focus();

    (container, text_view, submit)
}

/// Build a prompt shown after Plan mode completes, telling the user to switch to Act mode.
pub fn build_plan_mode_prompt() -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    container.add_css_class("agent-card");
    container.add_css_class("agent-card-plan");

    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    header.add_css_class("agent-message-header");
    header.set_halign(gtk4::Align::Start);
    header.append(&gtk4::Image::from_icon_name("document-properties-symbolic"));
    let header_label = gtk4::Label::new(Some("Plan Complete"));
    header.append(&header_label);
    container.append(&header);

    let label = gtk4::Label::new(None);
    label.set_markup("Switch to <b>Act</b> mode and instruct the agent to proceed with the plan.");
    label.set_halign(gtk4::Align::Start);
    label.set_wrap(true);
    label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    label.set_xalign(0.0);
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
        "browser_action" => {
            let action = v.get("action").and_then(|a| a.as_str())?;
            match action {
                "launch" => v
                    .get("url")
                    .and_then(|u| u.as_str())
                    .map(|u| format!("launch: {u}")),
                "click" => v
                    .get("coordinate")
                    .and_then(|c| c.as_str())
                    .map(|c| format!("click @ {c}")),
                "type" => v.get("text").and_then(|t| t.as_str()).map(|t| {
                    if t.len() > 40 {
                        format!("type: {}...", &t[..37])
                    } else {
                        format!("type: {t}")
                    }
                }),
                other => Some(other.to_owned()),
            }
        }
        _ => None,
    }
}

/// Build a collapsible thinking block widget.
///
/// Shows "Thought for N seconds" as a clickable header with the thinking
/// content hidden behind a revealer.
pub fn build_thinking_block(content: &str, duration_secs: u64) -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.add_css_class("agent-tool-card");

    let header_btn = gtk4::Button::new();
    header_btn.add_css_class("flat");
    header_btn.add_css_class("agent-tool-card-header");
    let header_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);

    let arrow = gtk4::Image::from_icon_name("pan-end-symbolic");
    arrow.add_css_class("agent-tool-arrow");
    header_box.append(&arrow);

    let label_text = if duration_secs == 1 {
        "Thought for 1 second".to_owned()
    } else {
        format!("Thought for {duration_secs} seconds")
    };
    let name_label = gtk4::Label::new(None);
    name_label.set_markup(&format!("<i>{}</i>", glib::markup_escape_text(&label_text)));
    name_label.add_css_class("dim-label");
    name_label.set_halign(gtk4::Align::Start);
    header_box.append(&name_label);

    header_btn.set_child(Some(&header_box));
    container.append(&header_btn);

    let revealer = gtk4::Revealer::new();
    revealer.set_reveal_child(false);
    revealer.set_transition_type(gtk4::RevealerTransitionType::SlideDown);

    let content_label = gtk4::Label::new(None);
    let pango = super::markdown::markdown_to_pango(content);
    content_label.set_markup(&pango);
    content_label.set_halign(gtk4::Align::Start);
    content_label.set_wrap(true);
    content_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    content_label.set_selectable(true);
    content_label.set_xalign(0.0);

    let detail_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    detail_box.add_css_class("agent-tool-card-detail");
    detail_box.append(&content_label);

    revealer.set_child(Some(&detail_box));
    container.append(&revealer);

    let rev_clone = revealer.clone();
    let arrow_clone = arrow.clone();
    header_btn.connect_clicked(move |_| {
        let revealed = rev_clone.reveals_child();
        rev_clone.set_reveal_child(!revealed);
        arrow_clone.set_icon_name(Some(if revealed {
            "pan-end-symbolic"
        } else {
            "pan-down-symbolic"
        }));
    });

    container
}

/// Build a "Working..." indicator label.
///
/// Returns the container box and the label (for updating the dot animation).
pub fn build_working_indicator() -> (gtk4::Box, gtk4::Label) {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    container.add_css_class("agent-message");
    container.add_css_class("agent-message-ai");

    container.append(&build_role_header(
        "applications-science-symbolic",
        "Assistant",
    ));

    let label = gtk4::Label::new(Some("Working."));
    label.add_css_class("agent-working");
    label.set_halign(gtk4::Align::Start);
    label.set_xalign(0.0);
    container.append(&label);

    (container, label)
}

/// Build an error message widget.
pub fn build_error(message: &str) -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    container.add_css_class("agent-card");
    container.add_css_class("agent-card-error");

    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    header.add_css_class("agent-message-header");
    header.set_halign(gtk4::Align::Start);
    header.append(&gtk4::Image::from_icon_name("dialog-error-symbolic"));
    let header_label = gtk4::Label::new(Some("Error"));
    header.append(&header_label);
    container.append(&header);

    let label = gtk4::Label::new(Some(message));
    label.set_halign(gtk4::Align::Start);
    label.set_wrap(true);
    label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.set_xalign(0.0);
    container.append(&label);

    container
}
