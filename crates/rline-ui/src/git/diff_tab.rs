//! DiffTab — side-by-side diff view for comparing file versions.
//!
//! Displays the old version (from HEAD or index) on the left and the new
//! version (from the working tree or index) on the right, with hunk
//! highlighting using colored backgrounds.

use std::path::{Path, PathBuf};

use gtk4::prelude::*;
use sourceview5::prelude::*;

use rline_config::EditorSettings;

use super::git_worker::FileDiff;

/// What kind of line this is in the aligned output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    /// Unchanged context line.
    Context,
    /// A deleted line (left side).
    Deletion,
    /// An added line (right side).
    Addition,
    /// A blank padding line inserted for alignment.
    Padding,
}

/// Aligned content for both sides of the diff with per-line markers.
struct AlignedContent {
    left_text: String,
    right_text: String,
    left_markers: Vec<LineKind>,
    right_markers: Vec<LineKind>,
}

/// A side-by-side diff view shown as a tab in the editor notebook.
#[derive(Debug, Clone)]
pub struct DiffTab {
    /// Horizontal paned container holding both views.
    container: gtk4::Paned,
    /// Left view — old content (read-only).
    left_view: sourceview5::View,
    /// Right view — new content (read-only for diff display).
    right_view: sourceview5::View,
    /// Left buffer.
    left_buffer: sourceview5::Buffer,
    /// Right buffer.
    right_buffer: sourceview5::Buffer,
    /// Tab label widget.
    tab_label: gtk4::Box,
    /// Close button in the tab label.
    close_btn: gtk4::Button,
    /// The file path this diff is for.
    file_path: PathBuf,
}

impl DiffTab {
    /// Create a diff tab showing the given diff for a file.
    pub fn load_diff(file_path: &Path, diff: &FileDiff, settings: &EditorSettings) -> Self {
        let left_buffer = sourceview5::Buffer::new(None);
        let right_buffer = sourceview5::Buffer::new(None);

        let left_view = Self::create_view(&left_buffer, settings);
        left_view.set_editable(false);

        let right_view = Self::create_view(&right_buffer, settings);
        right_view.set_editable(false);

        // Apply theme to both buffers.
        Self::apply_theme(&left_buffer, &settings.theme);
        Self::apply_theme(&right_buffer, &settings.theme);

        // Build aligned content with padding lines so both sides match vertically.
        let aligned = Self::build_aligned_content(diff);

        left_buffer.set_text(&aligned.left_text);
        right_buffer.set_text(&aligned.right_text);

        // Detect language and apply syntax highlighting.
        let lang_manager = sourceview5::LanguageManager::default();
        if let Some(lang) =
            lang_manager.guess_language(Some(&file_path.display().to_string()), None)
        {
            left_buffer.set_language(Some(&lang));
            right_buffer.set_language(Some(&lang));
        }

        // Create diff highlight tags and apply them using the aligned line ranges.
        Self::create_diff_tags(&left_buffer, &right_buffer);
        Self::apply_aligned_highlights(&left_buffer, &right_buffer, &aligned);

        // Mark both as not modified (they are display-only).
        left_buffer.set_modified(false);
        right_buffer.set_modified(false);

        // Layout: two scrolled views in a horizontal pane.
        let left_scrolled = gtk4::ScrolledWindow::builder()
            .child(&left_view)
            .vexpand(true)
            .hexpand(true)
            .build();

        let right_scrolled = gtk4::ScrolledWindow::builder()
            .child(&right_view)
            .vexpand(true)
            .hexpand(true)
            .build();

        let container = gtk4::Paned::new(gtk4::Orientation::Horizontal);
        container.set_start_child(Some(&left_scrolled));
        container.set_end_child(Some(&right_scrolled));
        container.set_resize_start_child(true);
        container.set_resize_end_child(true);
        container.set_vexpand(true);
        container.set_hexpand(true);

        // Set the divider to the exact middle once the widget is realized and sized.
        container.connect_realize(|paned| {
            let p = paned.clone();
            gtk4::glib::idle_add_local_once(move || {
                let width = p.width();
                if width > 0 {
                    p.set_position(width / 2);
                }
            });
        });

        // Synchronize scrolling between the two views.
        Self::sync_scrolling(&left_scrolled, &right_scrolled);

        // Build tab label with close button.
        let filename = file_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_owned());
        let label = gtk4::Label::new(Some(&format!("{filename} [diff]")));
        let close_btn = gtk4::Button::from_icon_name("window-close-symbolic");
        close_btn.add_css_class("flat");
        close_btn.add_css_class("circular");
        close_btn.set_valign(gtk4::Align::Center);
        close_btn.set_has_frame(false);
        close_btn.set_margin_start(2);
        let tab_label = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        tab_label.append(&label);
        tab_label.append(&close_btn);

        Self {
            container,
            left_view,
            right_view,
            left_buffer,
            right_buffer,
            tab_label,
            close_btn,
            file_path: file_path.to_path_buf(),
        }
    }

    /// Create a configured sourceview5::View for the diff pane.
    fn create_view(buffer: &sourceview5::Buffer, settings: &EditorSettings) -> sourceview5::View {
        let view = sourceview5::View::with_buffer(buffer);
        view.set_show_line_numbers(true);
        view.set_monospace(true);
        view.set_vexpand(true);
        view.set_hexpand(true);
        view.set_tab_width(settings.tab_width);
        view.set_highlight_current_line(false);
        view.set_wrap_mode(gtk4::WrapMode::None);

        // Apply font.
        let css = format!(
            "textview {{ font-family: \"{}\"; font-size: {}pt; }}",
            settings.editor_font_family, settings.font_size
        );
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(&css);
        gtk4::style_context_add_provider_for_display(
            &view.display(),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        view
    }

    /// Apply the sourceview theme to a buffer.
    fn apply_theme(buffer: &sourceview5::Buffer, theme_id: &str) {
        let scheme_manager = sourceview5::StyleSchemeManager::default();
        if let Some(scheme) = scheme_manager.scheme(theme_id) {
            buffer.set_style_scheme(Some(&scheme));
        }
    }

    /// Create text tags for diff highlighting.
    fn create_diff_tags(left_buffer: &sourceview5::Buffer, right_buffer: &sourceview5::Buffer) {
        // Deletion: red-ish background on old (left) side.
        let tag_table = left_buffer.tag_table();
        let del_tag = gtk4::TextTag::builder()
            .name("diff-deletion")
            .paragraph_background("rgba(255, 80, 80, 0.15)")
            .build();
        tag_table.add(&del_tag);
        let pad_tag = gtk4::TextTag::builder()
            .name("diff-padding")
            .paragraph_background("rgba(128, 128, 128, 0.08)")
            .build();
        tag_table.add(&pad_tag);

        // Addition: green-ish background on new (right) side.
        let tag_table = right_buffer.tag_table();
        let add_tag = gtk4::TextTag::builder()
            .name("diff-addition")
            .paragraph_background("rgba(80, 200, 80, 0.15)")
            .build();
        tag_table.add(&add_tag);
        let pad_tag = gtk4::TextTag::builder()
            .name("diff-padding")
            .paragraph_background("rgba(128, 128, 128, 0.08)")
            .build();
        tag_table.add(&pad_tag);
    }

    /// Build aligned content by inserting blank padding lines so both sides
    /// stay vertically aligned at hunk boundaries.
    fn build_aligned_content(diff: &FileDiff) -> AlignedContent {
        let old_lines: Vec<&str> = diff.old_content.lines().collect();
        let new_lines: Vec<&str> = diff.new_content.lines().collect();

        let mut left_out: Vec<String> = Vec::new();
        let mut right_out: Vec<String> = Vec::new();
        // Track which output lines are deletions, additions, or padding.
        let mut left_markers: Vec<LineKind> = Vec::new();
        let mut right_markers: Vec<LineKind> = Vec::new();

        let mut old_pos: usize = 0; // Current position in old_lines (0-based).
        let mut new_pos: usize = 0; // Current position in new_lines (0-based).

        for hunk in &diff.hunks {
            let hunk_old_start = hunk.old_start.saturating_sub(1) as usize;
            let hunk_new_start = hunk.new_start.saturating_sub(1) as usize;
            let hunk_old_lines = hunk.old_lines as usize;
            let hunk_new_lines = hunk.new_lines as usize;

            // Emit context lines before this hunk (lines that are the same).
            while old_pos < hunk_old_start && new_pos < hunk_new_start {
                left_out.push(old_lines.get(old_pos).unwrap_or(&"").to_string());
                right_out.push(new_lines.get(new_pos).unwrap_or(&"").to_string());
                left_markers.push(LineKind::Context);
                right_markers.push(LineKind::Context);
                old_pos += 1;
                new_pos += 1;
            }

            // Emit the hunk's deleted lines (left) and added lines (right).
            let del_count = hunk_old_lines;
            let add_count = hunk_new_lines;

            // Push deleted lines on the left.
            for _ in 0..del_count {
                left_out.push(old_lines.get(old_pos).unwrap_or(&"").to_string());
                left_markers.push(LineKind::Deletion);
                old_pos += 1;
            }

            // Push added lines on the right.
            for _ in 0..add_count {
                right_out.push(new_lines.get(new_pos).unwrap_or(&"").to_string());
                right_markers.push(LineKind::Addition);
                new_pos += 1;
            }

            // Pad the shorter side so both sides have the same number of output
            // lines for this hunk.
            match del_count.cmp(&add_count) {
                std::cmp::Ordering::Greater => {
                    for _ in 0..(del_count - add_count) {
                        right_out.push(String::new());
                        right_markers.push(LineKind::Padding);
                    }
                }
                std::cmp::Ordering::Less => {
                    for _ in 0..(add_count - del_count) {
                        left_out.push(String::new());
                        left_markers.push(LineKind::Padding);
                    }
                }
                std::cmp::Ordering::Equal => {}
            }
        }

        // Emit remaining context lines after the last hunk.
        while old_pos < old_lines.len() || new_pos < new_lines.len() {
            left_out.push(old_lines.get(old_pos).unwrap_or(&"").to_string());
            right_out.push(new_lines.get(new_pos).unwrap_or(&"").to_string());
            left_markers.push(LineKind::Context);
            right_markers.push(LineKind::Context);
            old_pos += 1;
            new_pos += 1;
        }

        AlignedContent {
            left_text: left_out.join("\n"),
            right_text: right_out.join("\n"),
            left_markers,
            right_markers,
        }
    }

    /// Apply highlighting based on aligned line markers.
    fn apply_aligned_highlights(
        left_buffer: &sourceview5::Buffer,
        right_buffer: &sourceview5::Buffer,
        aligned: &AlignedContent,
    ) {
        for (line_idx, kind) in aligned.left_markers.iter().enumerate() {
            let tag_name = match kind {
                LineKind::Deletion => "diff-deletion",
                LineKind::Padding => "diff-padding",
                _ => continue,
            };
            let start = left_buffer
                .iter_at_line(line_idx as i32)
                .unwrap_or_else(|| left_buffer.end_iter());
            let end = left_buffer
                .iter_at_line(line_idx as i32 + 1)
                .unwrap_or_else(|| left_buffer.end_iter());
            left_buffer.apply_tag_by_name(tag_name, &start, &end);
        }

        for (line_idx, kind) in aligned.right_markers.iter().enumerate() {
            let tag_name = match kind {
                LineKind::Addition => "diff-addition",
                LineKind::Padding => "diff-padding",
                _ => continue,
            };
            let start = right_buffer
                .iter_at_line(line_idx as i32)
                .unwrap_or_else(|| right_buffer.end_iter());
            let end = right_buffer
                .iter_at_line(line_idx as i32 + 1)
                .unwrap_or_else(|| right_buffer.end_iter());
            right_buffer.apply_tag_by_name(tag_name, &start, &end);
        }
    }

    /// Synchronize vertical scrolling between two scrolled windows.
    fn sync_scrolling(left: &gtk4::ScrolledWindow, right: &gtk4::ScrolledWindow) {
        let right_adj = right.vadjustment();
        let left_adj = left.vadjustment();

        let r = right_adj.clone();
        left_adj.connect_value_changed(move |adj| {
            if (r.value() - adj.value()).abs() > 0.5 {
                r.set_value(adj.value());
            }
        });

        let l = left_adj.clone();
        right_adj.connect_value_changed(move |adj| {
            if (l.value() - adj.value()).abs() > 0.5 {
                l.set_value(adj.value());
            }
        });
    }

    /// The file path this diff is for.
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// The widget to embed in the notebook.
    pub fn widget(&self) -> &gtk4::Paned {
        &self.container
    }

    /// The tab label widget.
    pub fn tab_label(&self) -> &gtk4::Box {
        &self.tab_label
    }

    /// The close button in the tab label.
    pub fn close_btn(&self) -> &gtk4::Button {
        &self.close_btn
    }

    /// Apply updated settings to both views.
    pub fn apply_settings(&self, settings: &EditorSettings) {
        for view in [&self.left_view, &self.right_view] {
            view.set_tab_width(settings.tab_width);
            let css = format!(
                "textview {{ font-family: \"{}\"; font-size: {}pt; }}",
                settings.editor_font_family, settings.font_size
            );
            let provider = gtk4::CssProvider::new();
            provider.load_from_data(&css);
            gtk4::style_context_add_provider_for_display(
                &view.display(),
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        Self::apply_theme(&self.left_buffer, &settings.theme);
        Self::apply_theme(&self.right_buffer, &settings.theme);
    }
}
