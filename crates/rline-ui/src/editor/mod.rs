//! Editor pane — tabbed GtkSourceView buffers with optional vertical split.

mod editor_pane;
mod find_bar;
mod settings_dialog;
mod split_container;
mod syntax_highlighter;
mod tab;

pub use editor_pane::EditorPane;
pub use settings_dialog::SettingsDialog;
pub use split_container::SplitContainer;
pub use syntax_highlighter::SyntaxHighlighter;
pub use tab::EditorTab;
