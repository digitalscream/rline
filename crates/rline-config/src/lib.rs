//! rline-config — Configuration parsing, settings persistence, and XDG path resolution.

pub mod error;
pub mod keybindings;
pub mod paths;
pub mod settings;
pub mod syntax_theme;
pub mod vscode_import;
pub mod zed_import;

pub use error::ConfigError;
pub use keybindings::{KeyBindings, ShortcutDescriptor, SHORTCUT_DESCRIPTORS};
pub use settings::{EditorSettings, PaneState, SessionState};
pub use syntax_theme::SyntaxTheme;
