//! Inline AI completion — ghost text display, debouncing, and async requests.
//!
//! Shows FIM (Fill-in-the-Middle) completions as dimmed "ghost text" in the
//! editor buffer. Completions are triggered automatically after a debounce
//! delay, or manually via Ctrl+Space.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use glib::prelude::*;
use gtk4::prelude::*;
use sourceview5::prelude::*;
use tokio_util::sync::CancellationToken;

use rline_ai::CompletionClient;
use rline_config::EditorSettings;

use crate::editor::SyntaxHighlighter;

// ── Public API ────────────────────────────────────────────────────────

/// Manages inline AI completions for a single editor buffer.
///
/// Handles ghost text insertion/removal, debounced request triggering,
/// async HTTP calls to the completion API, and keyboard interaction
/// (Tab to accept, Escape to dismiss).
#[derive(Clone)]
pub struct InlineCompletion {
    view: sourceview5::View,
    buffer: sourceview5::Buffer,
    ghost_tag: gtk4::TextTag,
    start_mark: gtk4::TextMark,
    end_mark: gtk4::TextMark,
    has_ghost_text: Rc<Cell<bool>>,
    debounce_source: Rc<Cell<Option<glib::SourceId>>>,
    cancel_token: Rc<RefCell<Option<CancellationToken>>>,
    client: Rc<RefCell<CompletionClient>>,
    suppressing: Rc<Cell<bool>>,

    // Cached settings
    trigger_mode: Rc<RefCell<String>>,
    max_tokens: Rc<Cell<u32>>,
    context_before: Rc<Cell<u32>>,
    context_after: Rc<Cell<u32>>,
    temperature: Rc<RefCell<f64>>,
    debounce_ms: Rc<Cell<u32>>,
    max_lines: Rc<Cell<u32>>,

    // Signal handler IDs for cleanup
    key_handler_id: Rc<RefCell<Option<glib::SignalHandlerId>>>,
    changed_handler_id: Rc<RefCell<Option<glib::SignalHandlerId>>>,

    /// Shared reference to the tab's syntax highlighter, used to block
    /// incremental rehighlighting during ghost text insert/delete and to
    /// trigger a full rehighlight after acceptance.
    highlighter: Rc<RefCell<Option<SyntaxHighlighter>>>,
}

impl std::fmt::Debug for InlineCompletion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InlineCompletion")
            .field("has_ghost_text", &self.has_ghost_text.get())
            .finish_non_exhaustive()
    }
}

impl InlineCompletion {
    /// Create a new inline completion handler for the given view and buffer.
    pub fn new(
        view: &sourceview5::View,
        buffer: &sourceview5::Buffer,
        settings: &EditorSettings,
        highlighter: Rc<RefCell<Option<SyntaxHighlighter>>>,
    ) -> Self {
        let client = CompletionClient::new(
            &settings.ai_endpoint_url,
            Some(&settings.ai_api_key),
            &settings.ai_model,
        );

        // Create the ghost text tag with theme-derived color.
        let ghost_tag = gtk4::TextTag::builder()
            .name("ghost-completion")
            .editable(false)
            .build();
        Self::apply_ghost_color(&ghost_tag, buffer);
        buffer.tag_table().add(&ghost_tag);

        // Create stable marks for tracking ghost text range.
        let start_iter = buffer.start_iter();
        let start_mark = buffer.create_mark(Some("ghost-start"), &start_iter, true);
        let end_mark = buffer.create_mark(Some("ghost-end"), &start_iter, false);

        let ic = Self {
            view: view.clone(),
            buffer: buffer.clone(),
            ghost_tag,
            start_mark,
            end_mark,
            has_ghost_text: Rc::new(Cell::new(false)),
            debounce_source: Rc::new(Cell::new(None)),
            cancel_token: Rc::new(RefCell::new(None)),
            client: Rc::new(RefCell::new(client)),
            suppressing: Rc::new(Cell::new(false)),
            trigger_mode: Rc::new(RefCell::new(settings.ai_trigger_mode.clone())),
            max_tokens: Rc::new(Cell::new(settings.ai_max_tokens)),
            context_before: Rc::new(Cell::new(settings.ai_context_lines_before)),
            context_after: Rc::new(Cell::new(settings.ai_context_lines_after)),
            temperature: Rc::new(RefCell::new(settings.ai_temperature)),
            debounce_ms: Rc::new(Cell::new(settings.ai_debounce_ms)),
            max_lines: Rc::new(Cell::new(settings.ai_max_lines)),
            key_handler_id: Rc::new(RefCell::new(None)),
            changed_handler_id: Rc::new(RefCell::new(None)),
            highlighter,
        };

        ic.setup_key_controller();
        ic.setup_buffer_changed();

        tracing::info!(
            "InlineCompletion created: endpoint={}, model={}, trigger={}, debounce={}ms",
            settings.ai_endpoint_url,
            settings.ai_model,
            settings.ai_trigger_mode,
            settings.ai_debounce_ms,
        );

        ic
    }

    /// Accept all remaining ghost text (keep it in the buffer).
    pub fn accept_completion(&self) {
        if !self.has_ghost_text.get() {
            return;
        }
        self.suppressing.set(true);

        let start = self.buffer.iter_at_mark(&self.start_mark);
        let end = self.buffer.iter_at_mark(&self.end_mark);
        self.buffer.remove_tag(&self.ghost_tag, &start, &end);
        self.buffer.place_cursor(&end);

        self.has_ghost_text.set(false);
        self.suppressing.set(false);

        // Rehighlight the buffer so syntax colors apply to the accepted text.
        if let Some(ref hl) = *self.highlighter.borrow() {
            hl.highlight_full();
        }
    }

    /// Accept only the first line of ghost text, leaving remaining lines
    /// as ghost text. If there is only one line, accepts all.
    pub fn accept_one_line(&self) {
        if !self.has_ghost_text.get() {
            return;
        }

        let start = self.buffer.iter_at_mark(&self.start_mark);
        let end = self.buffer.iter_at_mark(&self.end_mark);
        let ghost_text = self.buffer.text(&start, &end, true).to_string();

        // If there's no newline, accept everything.
        let Some(newline_pos) = ghost_text.find('\n') else {
            self.accept_completion();
            return;
        };

        self.suppressing.set(true);

        // Calculate the iter at the end of the first line (including the newline).
        let accept_len = (newline_pos + 1) as i32;
        let mut accept_end = start;
        accept_end.forward_chars(accept_len);

        // Remove the ghost tag from the accepted portion only.
        let accept_start = self.buffer.iter_at_mark(&self.start_mark);
        self.buffer
            .remove_tag(&self.ghost_tag, &accept_start, &accept_end);

        // Place cursor after the accepted line.
        self.buffer.place_cursor(&accept_end);

        // Move start_mark to the beginning of the remaining ghost text.
        self.buffer.move_mark(&self.start_mark, &accept_end);

        // Re-apply the ghost tag to remaining text to ensure it stays styled
        // as ghost text after the cursor move.
        let remaining_start = self.buffer.iter_at_mark(&self.start_mark);
        let remaining_end = self.buffer.iter_at_mark(&self.end_mark);
        self.buffer
            .apply_tag(&self.ghost_tag, &remaining_start, &remaining_end);

        // Keep ghost tag priority highest so syntax tags don't override it.
        let max_priority = self.buffer.tag_table().size() - 1;
        self.ghost_tag.set_priority(max_priority);

        self.suppressing.set(false);

        // Re-highlight the full buffer so accepted text gets syntax colors.
        // The ghost tag on remaining lines survives because highlight_full()
        // only touches ts-* tags, and we re-assert ghost priority afterwards.
        if let Some(ref hl) = *self.highlighter.borrow() {
            hl.highlight_full();
            // Re-assert ghost tag priority — highlight_full() may have added
            // tags that shifted priorities.
            let max_priority = self.buffer.tag_table().size() - 1;
            self.ghost_tag.set_priority(max_priority);
        }
    }

    /// Dismiss the current ghost text (remove it from the buffer).
    pub fn dismiss_completion(&self) {
        if !self.has_ghost_text.get() {
            return;
        }
        self.suppressing.set(true);
        self.block_highlighter();

        let was_modified = self.buffer.is_modified();

        let mut start = self.buffer.iter_at_mark(&self.start_mark);
        let mut end = self.buffer.iter_at_mark(&self.end_mark);
        self.buffer.delete(&mut start, &mut end);

        if !was_modified {
            self.buffer.set_modified(false);
        }

        self.has_ghost_text.set(false);
        self.unblock_highlighter();
        self.suppressing.set(false);
    }

    /// Manually trigger a completion request (ignores trigger mode).
    pub fn trigger_completion(&self) {
        tracing::info!("manual trigger_completion called");
        self.dismiss_completion();
        self.cancel_debounce();
        self.request_completion();
    }

    /// Update settings without recreating the handler.
    pub fn update_settings(&self, settings: &EditorSettings) {
        // Rebuild client if connection params changed.
        let old_client = self.client.borrow();
        let needs_rebuild = true; // Always rebuild — cheap operation.
        drop(old_client);

        if needs_rebuild {
            *self.client.borrow_mut() = CompletionClient::new(
                &settings.ai_endpoint_url,
                Some(&settings.ai_api_key),
                &settings.ai_model,
            );
        }

        *self.trigger_mode.borrow_mut() = settings.ai_trigger_mode.clone();
        self.max_tokens.set(settings.ai_max_tokens);
        self.context_before.set(settings.ai_context_lines_before);
        self.context_after.set(settings.ai_context_lines_after);
        *self.temperature.borrow_mut() = settings.ai_temperature;
        self.debounce_ms.set(settings.ai_debounce_ms);
        self.max_lines.set(settings.ai_max_lines);
    }

    /// Re-derive the ghost text color from the current buffer theme.
    pub fn update_ghost_tag_color(&self) {
        Self::apply_ghost_color(&self.ghost_tag, &self.buffer);
    }

    /// Cancel pending operations and clean up signal handlers.
    pub fn cleanup(&self) {
        self.dismiss_completion();
        self.cancel_debounce();
        self.cancel_inflight();

        // Remove the key controller signal.
        if let Some(id) = self.key_handler_id.borrow_mut().take() {
            self.view.disconnect(id);
        }

        // Remove the buffer changed handler.
        if let Some(id) = self.changed_handler_id.borrow_mut().take() {
            self.buffer.disconnect(id);
        }

        // Remove the ghost tag from the tag table.
        self.buffer.tag_table().remove(&self.ghost_tag);
    }

    // ── Private ───────────────────────────────────────────────────────

    /// Show ghost text at the current cursor position.
    fn show_ghost_text(&self, text: &str) {
        if text.is_empty() {
            return;
        }

        // Don't show if there's already ghost text.
        if self.has_ghost_text.get() {
            self.dismiss_completion();
        }

        self.suppressing.set(true);
        self.block_highlighter();

        let was_modified = self.buffer.is_modified();

        // Get the cursor position and record it.
        let cursor_iter = self.buffer.iter_at_mark(&self.buffer.get_insert());
        self.buffer.move_mark(&self.start_mark, &cursor_iter);

        // Insert the ghost text.
        self.buffer.insert(&mut cursor_iter.clone(), text);

        // Apply the ghost tag over the inserted text.
        let start = self.buffer.iter_at_mark(&self.start_mark);
        let mut end = start;
        end.forward_chars(text.chars().count() as i32);
        self.buffer.move_mark(&self.end_mark, &end);
        self.buffer.apply_tag(&self.ghost_tag, &start, &end);

        // Ensure the ghost tag has the highest priority so its foreground
        // color is not overridden by syntax highlighting tags.
        let max_priority = self.buffer.tag_table().size() - 1;
        self.ghost_tag.set_priority(max_priority);

        // Restore cursor to before the ghost text.
        let cursor_restore = self.buffer.iter_at_mark(&self.start_mark);
        self.buffer.place_cursor(&cursor_restore);

        // Restore modified state — ghost text is not a real edit.
        if !was_modified {
            self.buffer.set_modified(false);
        }

        self.has_ghost_text.set(true);
        self.unblock_highlighter();
        self.suppressing.set(false);
    }

    /// Set up the key event controller on the view (capture phase).
    fn setup_key_controller(&self) {
        let key_ctrl = gtk4::EventControllerKey::new();
        key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);

        let ic = self.clone();
        let handler_id = key_ctrl.connect_key_pressed(move |_ctrl, key, _code, mods| {
            if !ic.has_ghost_text.get() {
                return glib::Propagation::Proceed;
            }

            match key {
                gtk4::gdk::Key::Tab => {
                    // Tab accepts one line at a time.
                    ic.accept_one_line();
                    glib::Propagation::Stop
                }
                // Both Alt keys pressed simultaneously: accept all.
                // When the second Alt arrives the first is already held,
                // so the modifier state includes ALT_MASK.
                gtk4::gdk::Key::Alt_L | gtk4::gdk::Key::Alt_R
                    if mods.contains(gtk4::gdk::ModifierType::ALT_MASK) =>
                {
                    ic.accept_completion();
                    glib::Propagation::Stop
                }
                gtk4::gdk::Key::Escape => {
                    ic.dismiss_completion();
                    glib::Propagation::Stop
                }
                // A lone Alt press should not dismiss ghost text.
                gtk4::gdk::Key::Alt_L | gtk4::gdk::Key::Alt_R => glib::Propagation::Proceed,
                _ => {
                    // Any other key dismisses ghost text but lets the key through.
                    ic.dismiss_completion();
                    glib::Propagation::Proceed
                }
            }
        });

        // Store the handler ID for later cleanup.
        // Note: EventControllerKey signals return u64 handler IDs via glib,
        // but we store the controller on the view — cleanup happens by
        // disconnecting the signal.
        *self.key_handler_id.borrow_mut() = Some(handler_id);

        self.view.add_controller(key_ctrl);
    }

    /// Set up the buffer-changed handler for automatic completion triggering.
    ///
    /// Uses `end-user-action` instead of `changed` so that only genuine user
    /// edits (keystrokes, IME input) trigger completions — programmatic
    /// changes like `set_text` during file load are ignored.
    fn setup_buffer_changed(&self) {
        let ic = self.clone();
        let handler_id = self.buffer.connect_end_user_action(move |_buf| {
            if ic.suppressing.get() {
                return;
            }

            tracing::debug!("buffer changed, scheduling completion");

            // Dismiss existing ghost text on any buffer change.
            ic.dismiss_completion();
            ic.cancel_debounce();
            ic.cancel_inflight();

            // In "manual" mode, don't auto-trigger.
            let mode = ic.trigger_mode.borrow().clone();
            if mode == "manual" {
                tracing::debug!("trigger mode is manual, skipping auto-trigger");
                return;
            }

            // Start debounce timer.
            let ic2 = ic.clone();
            let delay = ic.debounce_ms.get();
            tracing::debug!("starting debounce timer: {delay}ms");
            let source_id =
                glib::timeout_add_local_once(Duration::from_millis(u64::from(delay)), move || {
                    ic2.debounce_source.set(None);
                    tracing::debug!("debounce timer fired, requesting completion");
                    ic2.request_completion();
                });
            ic.debounce_source.set(Some(source_id));
        });

        *self.changed_handler_id.borrow_mut() = Some(handler_id);
    }

    /// Request a completion from the AI backend.
    fn request_completion(&self) {
        tracing::info!("request_completion called");
        self.cancel_inflight();

        // Extract prefix and suffix context.
        let insert_mark = self.buffer.get_insert();
        let cursor = self.buffer.iter_at_mark(&insert_mark);
        let cursor_line = cursor.line();
        let cursor_offset = cursor.offset();

        let line_count = self.buffer.line_count();

        // Prefix: from (cursor_line - context_before) to cursor.
        let prefix_start_line = cursor_line
            .saturating_sub(self.context_before.get() as i32)
            .max(0);
        let prefix_start = self
            .buffer
            .iter_at_line(prefix_start_line)
            .unwrap_or_else(|| self.buffer.start_iter());
        let prefix = self.buffer.text(&prefix_start, &cursor, true).to_string();

        // Suffix: from cursor to (cursor_line + context_after).
        let suffix_end_line =
            (cursor_line + self.context_after.get() as i32).min(line_count.saturating_sub(1));
        let suffix_end = self
            .buffer
            .iter_at_line(suffix_end_line + 1)
            .unwrap_or_else(|| self.buffer.end_iter());
        let suffix = self.buffer.text(&cursor, &suffix_end, true).to_string();

        // Don't request if prefix is empty (no context).
        if prefix.is_empty() {
            return;
        }

        let cancel = CancellationToken::new();
        *self.cancel_token.borrow_mut() = Some(cancel.clone());

        let client = self.client.borrow().clone();
        let max_tokens = self.max_tokens.get();
        let temperature = *self.temperature.borrow();

        // Use std::sync::mpsc + glib::idle_add_local to bridge async → GTK.
        let (sender, receiver) = std::sync::mpsc::channel();

        // Keep a copy of the suffix for deduplication after the response arrives.
        let suffix_for_dedup = suffix.clone();
        let max_lines = self.max_lines.get();

        // Spawn the async request on the AI runtime.
        tracing::info!(
            "spawning completion request: prefix_len={}, suffix_len={}, max_tokens={max_tokens}",
            prefix.len(),
            suffix.len(),
        );
        rline_ai::ai_runtime().spawn(async move {
            tracing::debug!("async completion task started");
            let result = client
                .complete(&prefix, &suffix, max_tokens, temperature, cancel)
                .await;
            match &result {
                Ok(text) => tracing::info!("completion received: {} chars", text.len()),
                Err(e) => tracing::warn!("completion error: {e}"),
            }
            // Ignore send errors — the receiver may have been dropped.
            let _ = sender.send(result);
        });

        // Poll the receiver on the GTK main loop.
        let ic = self.clone();
        let expected_offset = cursor_offset;
        glib::idle_add_local(move || {
            match receiver.try_recv() {
                Ok(result) => {
                    // Check that the cursor hasn't moved since we sent the request.
                    let current_cursor = ic.buffer.iter_at_mark(&ic.buffer.get_insert());
                    if current_cursor.offset() != expected_offset {
                        tracing::debug!("cursor moved since completion request, discarding");
                        return glib::ControlFlow::Break;
                    }

                    match result {
                        Ok(text) => {
                            let trimmed = text.trim_end();
                            let truncated = Self::truncate_lines(trimmed, max_lines);
                            let cleaned =
                                Self::strip_overlapping_suffix(&truncated, &suffix_for_dedup);
                            if !cleaned.is_empty() {
                                ic.show_ghost_text(&cleaned);
                            }
                        }
                        Err(rline_ai::AiError::Cancelled) => {
                            tracing::debug!("completion request cancelled");
                        }
                        Err(e) => {
                            tracing::warn!("completion request failed: {e}");
                        }
                    }
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            }
        });
    }

    /// Cancel the pending debounce timer if any.
    fn cancel_debounce(&self) {
        if let Some(source_id) = self.debounce_source.take() {
            source_id.remove();
        }
    }

    /// Cancel any in-flight completion request.
    fn cancel_inflight(&self) {
        if let Some(token) = self.cancel_token.borrow_mut().take() {
            token.cancel();
        }
    }

    /// Block the syntax highlighter's buffer signals so ghost text
    /// insertions/deletions don't trigger incremental rehighlighting.
    fn block_highlighter(&self) {
        if let Some(ref hl) = *self.highlighter.borrow() {
            hl.block_signals();
        }
    }

    /// Unblock the syntax highlighter's buffer signals.
    fn unblock_highlighter(&self) {
        if let Some(ref hl) = *self.highlighter.borrow() {
            hl.unblock_signals();
        }
    }

    /// Derive ghost text color from the buffer's current style scheme.
    ///
    /// Uses the scheme's text foreground at ~40% opacity. Falls back to
    /// `#888888` if the scheme has no text style.
    fn apply_ghost_color(tag: &gtk4::TextTag, buffer: &sourceview5::Buffer) {
        let color = buffer
            .style_scheme()
            .and_then(|scheme| scheme.style("text"))
            .and_then(|style| {
                if style.is_foreground_set() {
                    style.foreground().map(|s| s.to_string())
                } else {
                    None
                }
            });

        let ghost_color = match color {
            Some(ref fg) => {
                // Parse the hex color and apply ~40% alpha.
                Self::dim_color(fg)
            }
            None => "rgba(136, 136, 136, 0.6)".to_owned(),
        };

        tag.set_foreground(Some(&ghost_color));
    }

    /// Truncate the completion to at most `max_lines` lines.
    /// A value of 0 means unlimited.
    fn truncate_lines(text: &str, max_lines: u32) -> String {
        if max_lines == 0 {
            return text.to_owned();
        }
        let mut lines = text.lines().take(max_lines as usize);
        let mut result = String::new();
        if let Some(first) = lines.next() {
            result.push_str(first);
            for line in lines {
                result.push('\n');
                result.push_str(line);
            }
        }
        result
    }

    /// Remove trailing lines from `completion` that duplicate the leading
    /// lines of `suffix` (the text already present after the cursor).
    ///
    /// Comparison is done on trimmed lines so that whitespace differences
    /// don't prevent deduplication.
    fn strip_overlapping_suffix(completion: &str, suffix: &str) -> String {
        let comp_lines: Vec<&str> = completion.lines().collect();
        let suffix_lines: Vec<&str> = suffix.lines().collect();

        if comp_lines.is_empty() || suffix_lines.is_empty() {
            return completion.to_owned();
        }

        // Find the longest overlap: the last N lines of the completion match
        // the first N lines of the suffix.
        let max_overlap = comp_lines.len().min(suffix_lines.len());
        let mut overlap = 0;

        for n in (1..=max_overlap).rev() {
            let comp_tail = &comp_lines[comp_lines.len() - n..];
            let suffix_head = &suffix_lines[..n];

            if comp_tail
                .iter()
                .zip(suffix_head.iter())
                .all(|(a, b)| a.trim() == b.trim())
            {
                overlap = n;
                break;
            }
        }

        if overlap == 0 {
            return completion.to_owned();
        }

        let keep = comp_lines.len() - overlap;
        if keep == 0 {
            return String::new();
        }

        // Rejoin the kept lines, preserving the original line endings.
        let mut result = comp_lines[..keep].join("\n");
        // If the original completion ended its kept portion with a newline, preserve it.
        if completion.contains('\n') && keep < comp_lines.len() {
            result.push('\n');
        }
        result
    }

    /// Take a hex color string (e.g. "#d4d4d4") and return an rgba() string
    /// with reduced opacity for ghost text appearance.
    fn dim_color(hex: &str) -> String {
        let hex = hex.trim_start_matches('#');
        if hex.len() >= 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(136);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(136);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(136);
            format!("rgba({r}, {g}, {b}, 0.4)")
        } else {
            "rgba(136, 136, 136, 0.6)".to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::InlineCompletion;

    #[test]
    fn test_strip_no_overlap() {
        let result =
            InlineCompletion::strip_overlapping_suffix("let x = 5;\nlet y = 10;", "fn main() {\n}");
        assert_eq!(result, "let x = 5;\nlet y = 10;");
    }

    #[test]
    fn test_strip_single_line_overlap() {
        let result = InlineCompletion::strip_overlapping_suffix(
            "    println!(\"hello\");\n}",
            "}\n\nfn other() {",
        );
        assert_eq!(result, "    println!(\"hello\");\n");
    }

    #[test]
    fn test_strip_multi_line_overlap() {
        let result =
            InlineCompletion::strip_overlapping_suffix("    let x = 5;\n    }\n}", "    }\n}\n");
        assert_eq!(result, "    let x = 5;\n");
    }

    #[test]
    fn test_strip_full_overlap() {
        let result = InlineCompletion::strip_overlapping_suffix("}\n", "}\nfn foo() {");
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_whitespace_insensitive() {
        let result =
            InlineCompletion::strip_overlapping_suffix("    x += 1;\n  }", "}\nfn bar() {");
        assert_eq!(result, "    x += 1;\n");
    }

    #[test]
    fn test_strip_empty_completion() {
        let result = InlineCompletion::strip_overlapping_suffix("", "}\n");
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_empty_suffix() {
        let result = InlineCompletion::strip_overlapping_suffix("let x = 5;", "");
        assert_eq!(result, "let x = 5;");
    }

    #[test]
    fn test_truncate_within_limit() {
        let result = InlineCompletion::truncate_lines("line1\nline2\nline3", 5);
        assert_eq!(result, "line1\nline2\nline3");
    }

    #[test]
    fn test_truncate_at_limit() {
        let result = InlineCompletion::truncate_lines("line1\nline2\nline3", 3);
        assert_eq!(result, "line1\nline2\nline3");
    }

    #[test]
    fn test_truncate_over_limit() {
        let result = InlineCompletion::truncate_lines("line1\nline2\nline3\nline4", 2);
        assert_eq!(result, "line1\nline2");
    }

    #[test]
    fn test_truncate_unlimited() {
        let text = "a\nb\nc\nd\ne";
        let result = InlineCompletion::truncate_lines(text, 0);
        assert_eq!(result, text);
    }

    #[test]
    fn test_truncate_single_line() {
        let result = InlineCompletion::truncate_lines("only one line", 5);
        assert_eq!(result, "only one line");
    }
}
