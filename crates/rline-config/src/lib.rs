//! rline-config — Configuration parsing, settings persistence, and XDG path resolution.

pub mod error;
pub mod paths;
pub mod settings;

pub use error::ConfigError;
pub use settings::EditorSettings;
