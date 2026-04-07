//! Editor pane — tabbed GtkSourceView buffers.

mod editor_pane;
mod find_bar;
mod settings_dialog;
mod syntax_highlighter;
mod tab;

pub use editor_pane::EditorPane;
pub use settings_dialog::SettingsDialog;
pub use syntax_highlighter::SyntaxHighlighter;
pub use tab::EditorTab;
