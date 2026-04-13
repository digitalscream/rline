//! EditorTab — a single tabbed editor view backed by GtkSourceView.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;
use sourceview5::prelude::*;

use rline_config::EditorSettings;
use rline_core::LineIndex;

use crate::editor::block_completion::BlockCompletion;
use crate::editor::bracket_completion::BracketCompletion;
use crate::editor::find_bar::FindBar;
use crate::editor::inline_completion::InlineCompletion;
use crate::editor::minimap::Minimap;
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
    /// Per-tab minimap (overlaid right edge, semi-transparent).
    _minimap: Minimap,
    /// AI inline completion handler (None when AI is disabled).
    inline_completion: Rc<RefCell<Option<InlineCompletion>>>,
    /// Automatic bracket/quote pair completion (kept alive for the key controller).
    _bracket_completion: BracketCompletion,
    /// Block completion, auto-dedent, comment continuation, and HTML tag closing.
    _block_completion: BlockCompletion,
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
        view.set_smart_home_end(sourceview5::SmartHomeEndType::Before);
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
        Self::apply_editor_font(
            &view,
            &settings.editor_font_family,
            settings.font_size,
            settings.letter_spacing,
            settings.line_height,
        );

        let scrolled = gtk4::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .hexpand(true)
            .build();

        // Overlay: scrolled editor as main child, find bar + minimap float on top
        let find_bar = FindBar::new(&buffer, &view);
        let minimap = Minimap::new(&buffer, &view);
        let overlay = gtk4::Overlay::new();
        overlay.set_child(Some(&scrolled));
        overlay.add_overlay(minimap.widget());
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

        let highlighter: Rc<RefCell<Option<SyntaxHighlighter>>> = Rc::new(RefCell::new(None));

        // Set up inline completion if AI is enabled and configured.
        let inline_completion = if settings.ai_enabled && !settings.ai_model.is_empty() {
            Some(InlineCompletion::new(
                &view,
                &buffer,
                settings,
                highlighter.clone(),
            ))
        } else {
            None
        };

        // Bracket completion must be added after InlineCompletion so that
        // ghost-text dismissal takes priority in the Capture phase.
        let bracket_completion = BracketCompletion::new(&view, &buffer);

        // Block completion must be added after BracketCompletion so it has
        // higher Capture-phase priority for Enter handling (no conflict with
        // bracket character keys).
        let block_completion = BlockCompletion::new(&view, &buffer);

        Self {
            overlay,
            view,
            buffer,
            tab_label,
            filename_label,
            close_btn,
            path: path_store,
            highlighter,
            use_treesitter: settings.use_treesitter,
            find_bar,
            _minimap: minimap,
            inline_completion: Rc::new(RefCell::new(inline_completion)),
            _bracket_completion: bracket_completion,
            _block_completion: block_completion,
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
        // Dismiss ghost text before saving to avoid writing it to disk.
        self.dismiss_ghost_text();

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

    /// The underlying sourceview5 Buffer.
    pub fn buffer(&self) -> &sourceview5::Buffer {
        &self.buffer
    }

    /// Toggle line comments on the current line, or on every line spanned by
    /// the current selection.
    ///
    /// The comment syntax is chosen from the buffer's language (if known) and
    /// otherwise falls back to the file extension. Languages without a line
    /// comment syntax (Markdown, JSON, HTML, XML, CSS) are a no-op.
    pub fn toggle_line_comment(&self) {
        let Some(prefix) = self.line_comment_prefix() else {
            tracing::debug!("toggle_line_comment: no line-comment prefix for this buffer");
            return;
        };
        self.apply_comment_toggle(prefix);
    }

    /// Resolve the line-comment prefix for this buffer.
    fn line_comment_prefix(&self) -> Option<&'static str> {
        if let Some(lang) = self.buffer.language() {
            if let Some(p) = comment_prefix_for_lang_id(lang.id().as_str()) {
                return Some(p);
            }
        }
        let path_ref = self.path.borrow();
        if let Some(ref path) = *path_ref {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let lower = ext.to_ascii_lowercase();
                if let Some(p) = comment_prefix_for_extension(&lower) {
                    return Some(p);
                }
            }
        }
        None
    }

    /// Drive the toggle edit against the buffer.
    ///
    /// The entire block of affected lines is replaced in a single
    /// `delete` + `insert` pair wrapped in `begin_user_action` /
    /// `end_user_action`, so Ctrl+Z undoes the whole toggle in one step and
    /// syntax-highlighting signals fire once at the end of the action.
    fn apply_comment_toggle(&self, prefix: &str) {
        let had_selection = self.buffer.has_selection();

        let (first_line, last_line) = if let Some((s, e)) = self.buffer.selection_bounds() {
            let first = s.line();
            let last_raw = e.line();
            // Match VS Code: when the selection ends exactly at column 0 on a
            // later line, that trailing line is not considered "selected".
            let last = if last_raw > first && e.line_offset() == 0 {
                last_raw - 1
            } else {
                last_raw
            };
            (first, last)
        } else {
            let cursor = self.buffer.iter_at_mark(&self.buffer.get_insert());
            (cursor.line(), cursor.line())
        };

        let Some(start_iter) = self.buffer.iter_at_line(first_line) else {
            return;
        };
        let end_iter = self.line_end_iter(last_line);
        let old_text = self.buffer.text(&start_iter, &end_iter, true).to_string();

        let Some(new_text) = transform_block(&old_text, prefix) else {
            return;
        };
        if new_text == old_text {
            return;
        }

        // Dismiss any visible ghost text and suppress the AI auto-trigger while
        // we mutate the buffer — this edit came from a shortcut, not a
        // keystroke, and should not kick off a completion request.
        self.dismiss_ghost_text();
        if let Some(ref ic) = *self.inline_completion.borrow() {
            ic.set_suppressing(true);
        }

        self.buffer.begin_user_action();
        let mut del_start = start_iter;
        let mut del_end = end_iter;
        self.buffer.delete(&mut del_start, &mut del_end);
        // `del_start` now points at the deletion site; insert the new block there.
        self.buffer.insert(&mut del_start, &new_text);
        self.buffer.end_user_action();

        if let Some(ref ic) = *self.inline_completion.borrow() {
            ic.set_suppressing(false);
        }

        if had_selection {
            if let Some(sel_start) = self.buffer.iter_at_line(first_line) {
                let sel_end = self.line_end_iter(last_line);
                self.buffer.select_range(&sel_start, &sel_end);
            }
        }
    }

    /// End-of-line iter for `line` (or the buffer end if `line` is past it).
    fn line_end_iter(&self, line: i32) -> gtk4::TextIter {
        match self.buffer.iter_at_line(line) {
            Some(mut e) => {
                if !e.ends_line() {
                    e.forward_to_line_end();
                }
                e
            }
            None => self.buffer.end_iter(),
        }
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
        Self::apply_editor_font(
            &self.view,
            &settings.editor_font_family,
            settings.font_size,
            settings.letter_spacing,
            settings.line_height,
        );

        // Rebuild tree-sitter tags from the new theme and re-highlight
        if let Some(ref mut hl) = *self.highlighter.borrow_mut() {
            hl.rebuild_tags_and_rehighlight();
        }

        // Handle AI completion settings changes.
        let mut ic_ref = self.inline_completion.borrow_mut();
        let ai_should_be_on = settings.ai_enabled && !settings.ai_model.is_empty();

        tracing::info!(
            "apply_settings AI: currently={}, should_be_on={ai_should_be_on}, enabled={}, model='{}'",
            ic_ref.is_some(),
            settings.ai_enabled,
            settings.ai_model,
        );

        match (ic_ref.is_some(), ai_should_be_on) {
            (false, true) => {
                // AI was disabled, now enabled — create handler.
                tracing::info!("creating InlineCompletion for existing tab");
                *ic_ref = Some(InlineCompletion::new(
                    &self.view,
                    &self.buffer,
                    settings,
                    self.highlighter.clone(),
                ));
            }
            (true, false) => {
                // AI was enabled, now disabled — clean up.
                tracing::info!("disabling InlineCompletion for tab");
                if let Some(ic) = ic_ref.take() {
                    ic.cleanup();
                }
            }
            (true, true) => {
                // AI stays enabled — update settings and ghost tag color.
                if let Some(ref ic) = *ic_ref {
                    ic.update_settings(settings);
                    ic.update_ghost_tag_color();
                }
            }
            (false, false) => {} // Nothing to do.
        }
    }

    /// Manually trigger an AI inline completion request.
    pub fn trigger_completion(&self) {
        let ic_ref = self.inline_completion.borrow();
        if let Some(ref ic) = *ic_ref {
            tracing::info!("tab.trigger_completion: forwarding to InlineCompletion");
            ic.trigger_completion();
        } else {
            tracing::info!("tab.trigger_completion: no InlineCompletion attached");
        }
    }

    /// Dismiss any visible ghost text (e.g. before saving).
    pub fn dismiss_ghost_text(&self) {
        if let Some(ref ic) = *self.inline_completion.borrow() {
            ic.dismiss_completion();
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

    /// Apply the editor font CSS to a `sourceview5::View`.
    ///
    /// Re-uses a single CSS provider per thread to avoid leaking providers on
    /// repeated calls. When the default `"Monospace"` family is specified, a
    /// fallback chain of popular coding fonts is used instead.
    pub fn apply_editor_font(
        view: &sourceview5::View,
        font_family: &str,
        font_size: u32,
        letter_spacing: f64,
        line_height: f64,
    ) {
        use std::cell::RefCell;

        thread_local! {
            static FONT_PROVIDER: RefCell<Option<gtk4::CssProvider>> = const { RefCell::new(None) };
        }

        // When the user hasn't chosen a specific font, try high-quality coding
        // fonts before falling back to the generic "Monospace" alias.
        let font_css = if font_family == "Monospace" {
            "\"JetBrains Mono\", \"Fira Code\", \"Cascadia Code\", \"Source Code Pro\", \"Monospace\"".to_owned()
        } else {
            format!("\"{font_family}\"")
        };

        let css = format!(
            "textview {{ font-family: {font_css}; font-size: {font_size}px; line-height: {line_height}; letter-spacing: {letter_spacing}px; font-feature-settings: \"liga\" 0, \"calt\" 1; }}"
        );

        let display = view.display();

        FONT_PROVIDER.with(|cell| {
            let mut slot = cell.borrow_mut();

            // Remove the old provider so we don't accumulate stale ones.
            if let Some(old) = slot.take() {
                gtk4::style_context_remove_provider_for_display(&display, &old);
            }

            let provider = gtk4::CssProvider::new();
            provider.load_from_string(&css);
            gtk4::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            *slot = Some(provider);
        });
    }
}

/// What the Ctrl+/ shortcut should do to a given set of lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToggleAction {
    /// No non-blank lines in the selection — do nothing.
    None,
    /// Add the line-comment prefix at `min_col` on each non-blank line.
    Comment { min_col: usize },
    /// Remove the line-comment prefix (and one optional trailing space) from
    /// each non-blank line.
    Uncomment,
}

/// Number of leading whitespace characters on a line (tabs count as one char).
fn leading_ws_char_count(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

/// A line is blank if it contains only whitespace (or nothing).
fn is_blank(line: &str) -> bool {
    line.chars().all(char::is_whitespace)
}

/// True if `line`, after its leading whitespace, begins with `prefix`.
fn is_line_commented(line: &str, prefix: &str) -> bool {
    let ws = leading_ws_char_count(line);
    line.chars()
        .skip(ws)
        .collect::<String>()
        .starts_with(prefix)
}

/// Decide whether to add or remove comments across a block of lines.
///
/// The rule matches VS Code: if every non-blank line already begins with the
/// prefix (after indentation), uncomment; otherwise comment everything at the
/// minimum indentation column so the block stays visually aligned.
fn decide_toggle_action(lines: &[&str], prefix: &str) -> ToggleAction {
    let mut any_non_blank = false;
    let mut all_commented = true;
    let mut min_col = usize::MAX;
    for line in lines {
        if is_blank(line) {
            continue;
        }
        any_non_blank = true;
        let ws = leading_ws_char_count(line);
        if ws < min_col {
            min_col = ws;
        }
        if !is_line_commented(line, prefix) {
            all_commented = false;
        }
    }
    if !any_non_blank {
        return ToggleAction::None;
    }
    if all_commented {
        ToggleAction::Uncomment
    } else {
        ToggleAction::Comment { min_col }
    }
}

/// Transform a block of text (lines separated by `\n`) by adding or removing
/// line comments per [`decide_toggle_action`]. Returns `None` when the block
/// has no non-blank lines (nothing to do).
///
/// Keeping this pure lets the buffer-side code perform a single
/// delete-then-insert pair, which groups into one undo step.
fn transform_block(text: &str, prefix: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let action = decide_toggle_action(&lines, prefix);
    match action {
        ToggleAction::None => None,
        ToggleAction::Comment { min_col } => {
            let insert_text = format!("{prefix} ");
            let out: Vec<String> = lines
                .iter()
                .map(|line| {
                    if is_blank(line) {
                        return (*line).to_string();
                    }
                    let head: String = line.chars().take(min_col).collect();
                    let tail: String = line.chars().skip(min_col).collect();
                    format!("{head}{insert_text}{tail}")
                })
                .collect();
            Some(out.join("\n"))
        }
        ToggleAction::Uncomment => {
            let out: Vec<String> = lines
                .iter()
                .map(|line| match find_uncomment_range(line, prefix) {
                    Some((cs, ce)) => {
                        let mut result: String = line.chars().take(cs).collect();
                        let tail: String = line.chars().skip(ce).collect();
                        result.push_str(&tail);
                        result
                    }
                    None => (*line).to_string(),
                })
                .collect();
            Some(out.join("\n"))
        }
    }
}

/// Character range to delete when uncommenting a single line.
///
/// Returns `(start, end)` as character offsets within `line`, covering the
/// prefix plus one trailing space if present. Returns `None` if the line
/// isn't commented (caller should skip it).
fn find_uncomment_range(line: &str, prefix: &str) -> Option<(usize, usize)> {
    let ws = leading_ws_char_count(line);
    let rest: String = line.chars().skip(ws).collect();
    if !rest.starts_with(prefix) {
        return None;
    }
    let prefix_chars = prefix.chars().count();
    let trail = if rest.chars().nth(prefix_chars) == Some(' ') {
        1
    } else {
        0
    };
    Some((ws, ws + prefix_chars + trail))
}

/// Line-comment prefix for a sourceview5 language ID.
fn comment_prefix_for_lang_id(id: &str) -> Option<&'static str> {
    match id {
        "rust" | "c" | "cpp" | "chdr" | "cpphdr" | "c-sharp" | "java" | "js" | "javascript"
        | "jsx" | "typescript" | "tsx" | "go" | "scala" | "kotlin" | "swift" | "dart" | "glsl"
        | "rust-trait" | "opencl" | "verilog" | "systemverilog" => Some("//"),
        "python" | "python3" | "ruby" | "sh" | "bash" | "shell" | "zsh" | "fish" | "yaml"
        | "toml" | "makefile" | "perl" | "r" | "elixir" | "nim" | "dockerfile" | "gitignore"
        | "gitcommit" | "conf" | "ini" | "cmake" => Some("#"),
        "sql" | "haskell" | "lua" | "ada" => Some("--"),
        "lisp" | "scheme" | "clojure" | "commonlisp" | "emacs-lisp" => Some(";"),
        "vim" | "vimrc" => Some("\""),
        "haml" => Some("-#"),
        "erlang" | "tex" | "latex" | "matlab" | "octave" => Some("%"),
        _ => None,
    }
}

/// Line-comment prefix for a (lower-cased) file extension.
fn comment_prefix_for_extension(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" | "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "cs" | "java" | "js"
        | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "go" | "scala" | "kt" | "kts" | "swift"
        | "dart" | "glsl" | "vert" | "frag" | "sv" => Some("//"),
        "py" | "pyw" | "rb" | "sh" | "bash" | "zsh" | "fish" | "yaml" | "yml" | "toml" | "mk"
        | "makefile" | "pl" | "pm" | "r" | "ex" | "exs" | "nim" | "conf" | "ini" | "cmake" => {
            Some("#")
        }
        "sql" | "hs" | "lhs" | "lua" | "ada" | "adb" | "ads" => Some("--"),
        "lisp" | "lsp" | "cl" | "scm" | "ss" | "clj" | "cljs" | "el" => Some(";"),
        "vim" | "vimrc" => Some("\""),
        "haml" => Some("-#"),
        "erl" | "hrl" | "tex" | "latex" | "sty" => Some("%"),
        _ => None,
    }
}

#[cfg(test)]
mod toggle_tests {
    use super::*;

    #[test]
    fn test_leading_ws_counts_tabs_and_spaces() {
        assert_eq!(leading_ws_char_count(""), 0);
        assert_eq!(leading_ws_char_count("no indent"), 0);
        assert_eq!(leading_ws_char_count("    four spaces"), 4);
        assert_eq!(leading_ws_char_count("\t\tmixed"), 2);
        assert_eq!(leading_ws_char_count("  \t mixed"), 4);
    }

    #[test]
    fn test_is_blank() {
        assert!(is_blank(""));
        assert!(is_blank("   "));
        assert!(is_blank("\t\t"));
        assert!(!is_blank(" x "));
    }

    #[test]
    fn test_is_line_commented_slash() {
        assert!(is_line_commented("// hi", "//"));
        assert!(is_line_commented("    // hi", "//"));
        assert!(is_line_commented("//", "//"));
        assert!(!is_line_commented("hi // trailing", "//"));
        assert!(!is_line_commented("", "//"));
    }

    #[test]
    fn test_decide_mixed_commentses() {
        // All non-blank lines commented → uncomment.
        let lines = vec!["// a", "    // b", "", "// c"];
        assert_eq!(
            decide_toggle_action(&lines, "//"),
            ToggleAction::Uncomment,
            "all commented → uncomment"
        );

        // One non-blank line not commented → comment all, at min indent.
        let lines = vec!["    x", "  // y"];
        assert_eq!(
            decide_toggle_action(&lines, "//"),
            ToggleAction::Comment { min_col: 2 },
            "mixed → comment at min indent"
        );

        // No non-blank lines → no-op.
        let lines = vec!["", "   ", "\t"];
        assert_eq!(decide_toggle_action(&lines, "//"), ToggleAction::None);
    }

    #[test]
    fn test_decide_single_uncommented_line() {
        let lines = vec!["  let x = 1;"];
        assert_eq!(
            decide_toggle_action(&lines, "//"),
            ToggleAction::Comment { min_col: 2 }
        );
    }

    #[test]
    fn test_decide_respects_blank_lines_for_min_col() {
        let lines = vec!["", "    x", "  y", ""];
        assert_eq!(
            decide_toggle_action(&lines, "//"),
            ToggleAction::Comment { min_col: 2 },
            "blank lines must not lower min_col to 0"
        );
    }

    #[test]
    fn test_find_uncomment_range_with_space() {
        let line = "    // hello";
        let (s, e) = find_uncomment_range(line, "//").expect("commented");
        assert_eq!((s, e), (4, 7), "removes // and one trailing space");
    }

    #[test]
    fn test_find_uncomment_range_without_space() {
        let line = "    //hello";
        let (s, e) = find_uncomment_range(line, "//").expect("commented");
        assert_eq!((s, e), (4, 6), "removes only //, no trailing space to eat");
    }

    #[test]
    fn test_find_uncomment_range_multi_char_prefix() {
        let line = "  -- SELECT 1";
        let (s, e) = find_uncomment_range(line, "--").expect("commented");
        assert_eq!((s, e), (2, 5));
    }

    #[test]
    fn test_find_uncomment_range_not_commented() {
        assert_eq!(find_uncomment_range("hello // world", "//"), None);
        assert_eq!(find_uncomment_range("", "//"), None);
    }

    #[test]
    fn test_find_uncomment_range_only_prefix() {
        let line = "//";
        let (s, e) = find_uncomment_range(line, "//").expect("commented");
        assert_eq!((s, e), (0, 2));
    }

    #[test]
    fn test_prefix_lookup_by_lang_id() {
        assert_eq!(comment_prefix_for_lang_id("rust"), Some("//"));
        assert_eq!(comment_prefix_for_lang_id("python"), Some("#"));
        assert_eq!(comment_prefix_for_lang_id("sql"), Some("--"));
        assert_eq!(comment_prefix_for_lang_id("haml"), Some("-#"));
        assert_eq!(comment_prefix_for_lang_id("markdown"), None);
        assert_eq!(comment_prefix_for_lang_id("html"), None);
        assert_eq!(comment_prefix_for_lang_id(""), None);
    }

    #[test]
    fn test_prefix_lookup_by_extension() {
        assert_eq!(comment_prefix_for_extension("rs"), Some("//"));
        assert_eq!(comment_prefix_for_extension("py"), Some("#"));
        assert_eq!(comment_prefix_for_extension("sql"), Some("--"));
        assert_eq!(comment_prefix_for_extension("haml"), Some("-#"));
        assert_eq!(comment_prefix_for_extension("md"), None);
    }

    #[test]
    fn test_transform_block_comments_mixed_indents() {
        // '// ' is inserted at the minimum-indent column (2 spaces here) on
        // every non-blank line, so deeper-indented lines keep their extra
        // whitespace *after* the marker. This preserves relative indentation
        // and makes the toggle a clean round-trip.
        let input = "    let x = 1;\n  if y {\n\n        println!();\n  }";
        let out = transform_block(input, "//").expect("non-empty");
        assert_eq!(
            out, "  //   let x = 1;\n  // if y {\n\n  //       println!();\n  // }",
            "marker inserted at min indent; original leading whitespace preserved after it"
        );
    }

    #[test]
    fn test_transform_block_uncomments_all_commented() {
        let input = "    // a\n  // b\n\n// c";
        let out = transform_block(input, "//").expect("non-empty");
        assert_eq!(
            out, "    a\n  b\n\nc",
            "prefix and single trailing space stripped per line; blanks preserved"
        );
    }

    #[test]
    fn test_transform_block_toggle_roundtrip() {
        let original = "fn main() {\n    println!();\n}";
        let commented = transform_block(original, "//").expect("non-empty");
        let back = transform_block(&commented, "//").expect("non-empty");
        assert_eq!(
            back, original,
            "comment then uncomment should restore the original text"
        );
    }

    #[test]
    fn test_transform_block_blank_only_is_noop() {
        assert_eq!(transform_block("", "//"), None);
        assert_eq!(transform_block("   \n\t\n", "//"), None);
    }

    #[test]
    fn test_transform_block_single_line_hash() {
        let out = transform_block("print('hi')", "#").expect("non-empty");
        assert_eq!(out, "# print('hi')");
        let back = transform_block(&out, "#").expect("non-empty");
        assert_eq!(back, "print('hi')");
    }
}
