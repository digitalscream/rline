//! Bridges `rline-syntax` tree-sitter highlighting to GtkSourceView TextTags.
//!
//! Creates GTK `TextTag` objects colored from either a rich [`SyntaxTheme`]
//! (for imported VS Code themes) or the active GtkSourceView style scheme
//! (fallback for native themes), then applies them to the buffer based on
//! [`HighlightSpan`] output from the [`HighlightEngine`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::prelude::*;
use sourceview5::prelude::*;

use rline_config::SyntaxTheme;
use rline_syntax::engine::HighlightEngine;
use rline_syntax::scope_map::{self, HIGHLIGHT_NAMES};
use rline_syntax::SupportedLanguage;

/// Bridges tree-sitter highlighting to a GtkSourceView buffer.
///
/// Owns a [`HighlightEngine`] for a specific language and a set of `TextTag`
/// objects derived from either a rich syntax theme or the GtkSourceView scheme.
/// Connects to buffer change signals for incremental re-highlighting.
pub struct SyntaxHighlighter {
    engine: Rc<RefCell<HighlightEngine>>,
    buffer: sourceview5::Buffer,
    /// Maps highlight index → TextTag (only for indices that have a style mapping).
    tags: HashMap<usize, gtk4::TextTag>,
    /// Signal handler IDs so we can block them during bulk operations.
    insert_handler: Option<glib::SignalHandlerId>,
    delete_handler: Option<glib::SignalHandlerId>,
    /// Tracks whether an idle re-highlight is already scheduled.
    rehighlight_scheduled: Rc<RefCell<bool>>,
}

impl std::fmt::Debug for SyntaxHighlighter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyntaxHighlighter")
            .field("language", &self.engine.borrow().language())
            .field("tag_count", &self.tags.len())
            .finish()
    }
}

impl SyntaxHighlighter {
    /// Create a new syntax highlighter for the given language and buffer.
    ///
    /// This disables GtkSourceView's built-in syntax highlighting on the buffer,
    /// creates TextTags from the active theme (rich or GtkSourceView fallback),
    /// and connects buffer change signals for incremental re-highlighting.
    ///
    /// # Errors
    ///
    /// Returns an error if the [`HighlightEngine`] cannot be created for the language.
    pub fn new(
        language: SupportedLanguage,
        buffer: &sourceview5::Buffer,
    ) -> Result<Self, rline_syntax::SyntaxError> {
        let engine = HighlightEngine::new(language)?;

        // Disable GtkSourceView's built-in syntax highlighting
        buffer.set_highlight_syntax(false);

        // Build TextTags — try rich syntax theme first, fall back to GtkSourceView
        let tags = Self::build_tags(buffer);

        let engine = Rc::new(RefCell::new(engine));
        let rehighlight_scheduled = Rc::new(RefCell::new(false));

        let mut highlighter = Self {
            engine,
            buffer: buffer.clone(),
            tags,
            insert_handler: None,
            delete_handler: None,
            rehighlight_scheduled,
        };

        highlighter.connect_buffer_signals();

        Ok(highlighter)
    }

    /// Perform a full highlight of the entire buffer contents.
    ///
    /// Call this after loading a file or when the theme changes.
    pub fn highlight_full(&self) {
        let (start, end) = self.buffer.bounds();
        let text = self.buffer.text(&start, &end, true);
        let source = text.as_bytes();

        // Block change signals during tag application
        self.block_signals();

        // Remove existing tree-sitter tags
        self.remove_all_highlight_tags(&start, &end);

        match self.engine.borrow_mut().parse_and_highlight(source) {
            Ok(spans) => {
                self.apply_spans(&spans, source);
            }
            Err(e) => {
                tracing::warn!("tree-sitter highlighting failed: {e}");
            }
        }

        self.unblock_signals();
    }

    /// Rebuild TextTags from the current theme and re-highlight.
    ///
    /// Call this when the user changes the theme.
    pub fn rebuild_tags_and_rehighlight(&mut self) {
        // Remove old tags from the tag table
        let tag_table = self.buffer.tag_table();
        for tag in self.tags.values() {
            tag_table.remove(tag);
        }

        self.tags = Self::build_tags(&self.buffer);
        self.highlight_full();
    }

    /// Build TextTags for each highlight group.
    ///
    /// First tries to load a rich [`SyntaxTheme`] for the active scheme (saved
    /// alongside imported VS Code themes). If found, uses TextMate scope matching
    /// for full color granularity. Otherwise falls back to GtkSourceView style IDs.
    fn build_tags(buffer: &sourceview5::Buffer) -> HashMap<usize, gtk4::TextTag> {
        let tag_table = buffer.tag_table();
        let scheme = buffer.style_scheme();

        // Try to load a rich syntax theme for the active scheme
        let scheme_id = scheme.as_ref().map(|s| s.id().to_string());
        let syntax_theme = scheme_id
            .as_deref()
            .and_then(|id| match SyntaxTheme::load(id) {
                Ok(theme) => theme,
                Err(e) => {
                    tracing::debug!("no syntax theme for {id}: {e}");
                    None
                }
            });

        let mut tags = HashMap::new();

        for (index, &_name) in HIGHLIGHT_NAMES.iter().enumerate() {
            let tag_name = format!("ts-{index}");

            // Remove any pre-existing tag with this name
            if let Some(existing) = tag_table.lookup(&tag_name) {
                tag_table.remove(&existing);
            }

            let tag = gtk4::TextTag::new(Some(&tag_name));
            let mut has_style = false;

            if let Some(ref theme) = syntax_theme {
                // Rich theme path: map capture → TextMate scope → theme color
                if let Some(textmate_scope) = scope_map::highlight_to_textmate_scope(index) {
                    if let Some(token_style) = theme.resolve(textmate_scope) {
                        if let Some(ref fg) = token_style.foreground {
                            tag.set_foreground(Some(fg));
                            has_style = true;
                        }
                        if token_style.bold {
                            tag.set_weight(700);
                            has_style = true;
                        }
                        if token_style.italic {
                            tag.set_style(pango::Style::Italic);
                            has_style = true;
                        }
                        if token_style.underline {
                            tag.set_underline(pango::Underline::Single);
                            has_style = true;
                        }
                        if token_style.strikethrough {
                            tag.set_strikethrough(true);
                            has_style = true;
                        }
                    }
                }
            } else {
                // GtkSourceView fallback path
                let style_id = scope_map::highlight_to_style_id(index);
                let style = style_id.and_then(|id| scheme.as_ref().and_then(|s| s.style(id)));

                if let Some(ref style) = style {
                    if style.is_foreground_set() {
                        if let Some(fg) = style.foreground() {
                            tag.set_foreground(Some(&fg));
                            has_style = true;
                        }
                    }
                    if style.is_bold_set() && style.is_bold() {
                        tag.set_weight(700);
                        has_style = true;
                    }
                    if style.is_italic_set() && style.is_italic() {
                        tag.set_style(pango::Style::Italic);
                        has_style = true;
                    }
                    if style.is_underline_set() {
                        tag.set_underline(style.pango_underline());
                        has_style = true;
                    }
                    if style.is_strikethrough_set() && style.is_strikethrough() {
                        tag.set_strikethrough(true);
                        has_style = true;
                    }
                }
            }

            if has_style {
                tag_table.add(&tag);
                tags.insert(index, tag);
            }
        }

        if syntax_theme.is_some() {
            tracing::debug!(
                "built {} tags from rich syntax theme ({} captures)",
                tags.len(),
                HIGHLIGHT_NAMES.len()
            );
        } else {
            tracing::debug!(
                "built {} tags from GtkSourceView scheme ({} captures)",
                tags.len(),
                HIGHLIGHT_NAMES.len()
            );
        }

        tags
    }

    /// Apply highlight spans to the buffer as TextTags.
    fn apply_spans(&self, spans: &[rline_syntax::HighlightSpan], source: &[u8]) {
        for span in spans {
            let Some(tag) = self.tags.get(&span.highlight_index) else {
                continue;
            };

            let start_iter = self.byte_offset_to_iter(source, span.byte_start);
            let end_iter = self.byte_offset_to_iter(source, span.byte_end);

            self.buffer.apply_tag(tag, &start_iter, &end_iter);
        }
    }

    /// Convert a byte offset in the source to a `gtk4::TextIter` in the buffer.
    fn byte_offset_to_iter(&self, source: &[u8], byte_offset: usize) -> gtk4::TextIter {
        let offset = byte_offset.min(source.len());
        let mut line: i32 = 0;
        let mut line_start_byte = 0;

        for (i, &byte) in source[..offset].iter().enumerate() {
            if byte == b'\n' {
                line += 1;
                line_start_byte = i + 1;
            }
        }

        let byte_in_line = (offset - line_start_byte) as i32;

        self.buffer
            .iter_at_line_index(line, byte_in_line)
            .unwrap_or_else(|| self.buffer.end_iter())
    }

    /// Remove all tree-sitter highlight tags from the given range.
    fn remove_all_highlight_tags(&self, start: &gtk4::TextIter, end: &gtk4::TextIter) {
        for tag in self.tags.values() {
            self.buffer.remove_tag(tag, start, end);
        }
    }

    /// Connect to buffer insert/delete signals for incremental re-highlighting.
    fn connect_buffer_signals(&mut self) {
        let engine = self.engine.clone();
        let buffer_weak = self.buffer.downgrade();
        let tags = self.tags.clone();
        let scheduled = self.rehighlight_scheduled.clone();

        let schedule_rehighlight = move || {
            let mut is_scheduled = scheduled.borrow_mut();
            if *is_scheduled {
                return;
            }
            *is_scheduled = true;

            let engine = engine.clone();
            let buffer_weak = buffer_weak.clone();
            let tags = tags.clone();
            let scheduled = scheduled.clone();

            glib::idle_add_local_once(move || {
                *scheduled.borrow_mut() = false;

                let Some(buffer) = buffer_weak.upgrade() else {
                    return;
                };

                let (start, end) = buffer.bounds();
                let text = buffer.text(&start, &end, true);
                let source = text.as_bytes();

                match engine.borrow_mut().parse_and_highlight(source) {
                    Ok(spans) => {
                        for tag in tags.values() {
                            buffer.remove_tag(tag, &start, &end);
                        }
                        for span in &spans {
                            let Some(tag) = tags.get(&span.highlight_index) else {
                                continue;
                            };
                            let s_iter =
                                byte_offset_to_iter_static(&buffer, source, span.byte_start);
                            let e_iter = byte_offset_to_iter_static(&buffer, source, span.byte_end);
                            buffer.apply_tag(tag, &s_iter, &e_iter);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("incremental tree-sitter highlighting failed: {e}");
                    }
                }
            });
        };

        let schedule_for_insert = schedule_rehighlight.clone();
        let insert_handler = self.buffer.connect_insert_text(move |_buf, _loc, _text| {
            schedule_for_insert();
        });

        let schedule_for_delete = schedule_rehighlight;
        let delete_handler = self.buffer.connect_delete_range(move |_buf, _start, _end| {
            schedule_for_delete();
        });

        self.insert_handler = Some(insert_handler);
        self.delete_handler = Some(delete_handler);
    }

    /// Temporarily block change signals (e.g. during bulk tag application).
    fn block_signals(&self) {
        if let Some(ref handler) = self.insert_handler {
            self.buffer.block_signal(handler);
        }
        if let Some(ref handler) = self.delete_handler {
            self.buffer.block_signal(handler);
        }
    }

    /// Unblock change signals.
    fn unblock_signals(&self) {
        if let Some(ref handler) = self.insert_handler {
            self.buffer.unblock_signal(handler);
        }
        if let Some(ref handler) = self.delete_handler {
            self.buffer.unblock_signal(handler);
        }
    }
}

/// Static version of byte_offset_to_iter for use in closures that don't have &self.
fn byte_offset_to_iter_static(
    buffer: &sourceview5::Buffer,
    source: &[u8],
    byte_offset: usize,
) -> gtk4::TextIter {
    let offset = byte_offset.min(source.len());
    let mut line: i32 = 0;
    let mut line_start_byte = 0;

    for (i, &byte) in source[..offset].iter().enumerate() {
        if byte == b'\n' {
            line += 1;
            line_start_byte = i + 1;
        }
    }

    let byte_in_line = (offset - line_start_byte) as i32;

    buffer
        .iter_at_line_index(line, byte_in_line)
        .unwrap_or_else(|| buffer.end_iter())
}
