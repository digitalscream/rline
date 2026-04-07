//! EditorTab — a single tabbed editor view backed by GtkSourceView.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;
use sourceview5::prelude::*;

use rline_config::EditorSettings;
use rline_core::LineIndex;

use crate::editor::find_bar::FindBar;
use crate::editor::syntax_highlighter::SyntaxHighlighter;
use crate::error::UiError;

/// A single editor tab containing a sourceview5 View and its associated state.
#[derive(Debug, Clone)]
pub struct EditorTab {
    /// Overlay container — scrolled editor with find bar floating on top.
    overlay: gtk4::Overlay,
    /// The sourceview5 view widget.
    view: sourceview5::View,
    /// The sourceview5 buffer.
    buffer: sourceview5::Buffer,
    /// The tab label box (icon + filename + close button).
    tab_label: gtk4::Box,
    /// The filename label within the tab.
    filename_label: gtk4::Label,
    /// Close button in the tab label.
    close_btn: gtk4::Button,
    /// The file path for this tab.
    path: Rc<RefCell<Option<PathBuf>>>,
    /// Tree-sitter syntax highlighter (None when no grammar exists for this file).
    highlighter: Rc<RefCell<Option<SyntaxHighlighter>>>,
    /// Whether tree-sitter highlighting is enabled.
    use_treesitter: bool,
    /// Per-tab find/replace bar (overlaid top-right).
    find_bar: FindBar,
}

impl EditorTab {
    /// Create a new editor tab, optionally loading a file.
    pub fn new(settings: &EditorSettings) -> Self {
        let buffer = sourceview5::Buffer::new(None);
        let view = sourceview5::View::with_buffer(&buffer);

        // Configure the view
        view.set_show_line_numbers(settings.show_line_numbers);
        view.set_tab_width(settings.tab_width);
        view.set_auto_indent(true);
        view.set_indent_width(settings.tab_width as i32);
        view.set_insert_spaces_instead_of_tabs(settings.insert_spaces);
        view.set_highlight_current_line(true);
        view.set_monospace(true);
        view.set_vexpand(true);
        view.set_hexpand(true);

        if settings.wrap_text {
            view.set_wrap_mode(gtk4::WrapMode::Word);
        } else {
            view.set_wrap_mode(gtk4::WrapMode::None);
        }

        // Apply theme
        Self::apply_theme_to_buffer(&buffer, &settings.theme);

        // Apply font
        Self::apply_font(&view, &settings.editor_font_family, settings.font_size);

        let scrolled = gtk4::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .hexpand(true)
            .build();

        // Overlay: scrolled editor as main child, find bar floats on top
        let find_bar = FindBar::new(&buffer, &view);
        let overlay = gtk4::Overlay::new();
        overlay.set_child(Some(&scrolled));
        overlay.add_overlay(find_bar.widget());
        overlay.set_vexpand(true);
        overlay.set_hexpand(true);

        // Build tab label with close button
        let filename_label = gtk4::Label::new(Some("Untitled"));
        let close_btn = gtk4::Button::from_icon_name("window-close-symbolic");
        close_btn.add_css_class("flat");
        close_btn.add_css_class("circular");
        close_btn.set_valign(gtk4::Align::Center);
        close_btn.set_has_frame(false);
        // Shrink the button so it doesn't dominate the tab label.
        close_btn.set_margin_start(2);
        close_btn.set_margin_end(0);

        let tab_label = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        tab_label.append(&filename_label);
        tab_label.append(&close_btn);

        // Connect modified signal to update tab label
        let path_store: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
        let path_for_signal = path_store.clone();
        buffer.connect_modified_changed(glib::clone!(
            #[weak]
            filename_label,
            move |buf| {
                let name = path_for_signal
                    .borrow()
                    .as_ref()
                    .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
                    .unwrap_or_else(|| "Untitled".to_owned());

                if buf.is_modified() {
                    filename_label.set_text(&format!("● {name}"));
                } else {
                    filename_label.set_text(&name);
                }
            }
        ));

        Self {
            overlay,
            view,
            buffer,
            tab_label,
            filename_label,
            close_btn,
            path: path_store,
            highlighter: Rc::new(RefCell::new(None)),
            use_treesitter: settings.use_treesitter,
            find_bar,
        }
    }

    /// Load a file into this tab.
    pub fn load_file(&self, path: &Path) -> Result<(), UiError> {
        let contents = std::fs::read_to_string(path).map_err(|e| UiError::FileOpen {
            path: path.display().to_string(),
            source: e,
        })?;

        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_owned());

        self.buffer.set_text(&contents);
        self.buffer.set_modified(false);
        self.filename_label.set_text(&filename);
        self.path.replace(Some(path.to_path_buf()));

        // Set language based on file extension (kept for auto-indent and context classes)
        let lang_manager = sourceview5::LanguageManager::default();
        if let Some(lang) = lang_manager.guess_language(Some(&path.display().to_string()), None) {
            self.buffer.set_language(Some(&lang));
        }

        // Set up tree-sitter highlighting if available
        if self.use_treesitter {
            self.setup_treesitter_highlighting(path);
        }

        Ok(())
    }

    /// Save the buffer contents to the associated file path.
    pub fn save(&self) -> Result<(), UiError> {
        let path_ref = self.path.borrow();
        if let Some(ref path) = *path_ref {
            let (start, end) = self.buffer.bounds();
            let text = self.buffer.text(&start, &end, true);
            std::fs::write(path, text.as_str()).map_err(|e| UiError::FileOpen {
                path: path.display().to_string(),
                source: e,
            })?;
            self.buffer.set_modified(false);
            Ok(())
        } else {
            Ok(()) // No path — nothing to save
        }
    }

    /// Navigate to a specific line.
    pub fn goto_line(&self, line: LineIndex) {
        let iter = self.buffer.iter_at_line(line.0 as i32).unwrap_or_else(|| {
            let mut it = self.buffer.end_iter();
            it.set_line(line.0 as i32);
            it
        });
        self.buffer.place_cursor(&iter);
        // Scroll to cursor after placement
        let mut cursor_iter = self.buffer.iter_at_mark(&self.buffer.get_insert());
        self.view
            .scroll_to_iter(&mut cursor_iter, 0.2, false, 0.0, 0.5);
    }

    /// Returns true if the buffer has unsaved modifications.
    pub fn is_modified(&self) -> bool {
        self.buffer.is_modified()
    }

    /// The file path for this tab, if any.
    pub fn file_path(&self) -> Option<PathBuf> {
        self.path.borrow().clone()
    }

    /// Show this tab's find bar overlay.
    pub fn show_find_bar(&self, with_replace: bool) {
        self.find_bar.show(with_replace);
    }

    /// The widget to embed in the notebook.
    pub fn widget(&self) -> &gtk4::Overlay {
        &self.overlay
    }

    /// The tab label widget.
    pub fn tab_label(&self) -> &gtk4::Box {
        &self.tab_label
    }

    /// The close button in the tab label.
    pub fn close_btn(&self) -> &gtk4::Button {
        &self.close_btn
    }

    /// The underlying sourceview5 View.
    pub fn view(&self) -> &sourceview5::View {
        &self.view
    }

    /// Apply settings to this tab.
    pub fn apply_settings(&self, settings: &EditorSettings) {
        self.view.set_show_line_numbers(settings.show_line_numbers);
        self.view.set_tab_width(settings.tab_width);
        self.view.set_indent_width(settings.tab_width as i32);
        self.view
            .set_insert_spaces_instead_of_tabs(settings.insert_spaces);
        if settings.wrap_text {
            self.view.set_wrap_mode(gtk4::WrapMode::Word);
        } else {
            self.view.set_wrap_mode(gtk4::WrapMode::None);
        }
        Self::apply_theme_to_buffer(&self.buffer, &settings.theme);
        Self::apply_font(&self.view, &settings.editor_font_family, settings.font_size);

        // Rebuild tree-sitter tags from the new theme and re-highlight
        if let Some(ref mut hl) = *self.highlighter.borrow_mut() {
            hl.rebuild_tags_and_rehighlight();
        }
    }

    /// Set up tree-sitter highlighting for the file at the given path.
    fn setup_treesitter_highlighting(&self, path: &Path) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let Some(language) = rline_syntax::language_for_extension(ext) else {
            tracing::debug!("no tree-sitter grammar for extension: {ext:?}");
            return;
        };

        match SyntaxHighlighter::new(language, &self.buffer) {
            Ok(hl) => {
                hl.highlight_full();
                self.highlighter.replace(Some(hl));
            }
            Err(e) => {
                tracing::warn!("failed to create tree-sitter highlighter: {e}");
                // Fall back to GtkSourceView highlighting
                self.buffer.set_highlight_syntax(true);
            }
        }
    }

    fn apply_theme_to_buffer(buffer: &sourceview5::Buffer, theme_id: &str) {
        let scheme_manager = sourceview5::StyleSchemeManager::default();
        if let Some(scheme) = scheme_manager.scheme(theme_id) {
            buffer.set_style_scheme(Some(&scheme));
        }
        // Also update the application-wide chrome to match the theme
        crate::theming::apply_app_theme(theme_id);
    }

    fn apply_font(view: &sourceview5::View, font_family: &str, font_size: u32) {
        let css =
            format!("textview {{ font-family: \"{font_family}\"; font-size: {font_size}pt; }}");
        let provider = gtk4::CssProvider::new();
        provider.load_from_data(&css);
        gtk4::style_context_add_provider_for_display(
            &view.display(),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
