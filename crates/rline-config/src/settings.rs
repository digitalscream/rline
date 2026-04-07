//! Editor settings with persistence.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::paths;

/// Editor settings that are persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    /// The GtkSourceView style scheme ID.
    pub theme: String,
    /// Editor font family.
    pub editor_font_family: String,
    /// Editor font size in points.
    pub font_size: u32,
    /// Tab width in spaces.
    pub tab_width: u32,
    /// Whether to insert spaces instead of tab characters.
    #[serde(default = "default_true")]
    pub insert_spaces: bool,
    /// Whether to show line numbers.
    pub show_line_numbers: bool,
    /// Whether to wrap text.
    pub wrap_text: bool,
    /// Terminal font family.
    pub terminal_font_family: String,
    /// Terminal font size in points.
    pub terminal_font_size: u32,
    /// Whether to reopen the last project on startup.
    pub open_last_project: bool,
    /// The path of the last opened project (set automatically).
    pub last_project_path: Option<String>,
    /// Auto-expand search result files with this many matches or fewer.
    pub search_auto_expand_threshold: u32,
    /// Whether to use tree-sitter for syntax highlighting (falls back to GtkSourceView).
    #[serde(default = "default_true")]
    pub use_treesitter: bool,
    /// Number of most-recently-used tabs to cycle through with Ctrl+Tab.
    #[serde(default = "default_tab_cycle_depth")]
    pub tab_cycle_depth: u32,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            theme: "Adwaita-dark".to_owned(),
            editor_font_family: "Monospace".to_owned(),
            font_size: 13,
            tab_width: 4,
            insert_spaces: true,
            show_line_numbers: true,
            wrap_text: false,
            terminal_font_family: "Monospace".to_owned(),
            terminal_font_size: 13,
            open_last_project: true,
            last_project_path: None,
            search_auto_expand_threshold: 5,
            use_treesitter: true,
            tab_cycle_depth: default_tab_cycle_depth(),
        }
    }
}

/// Helper for `#[serde(default)]` on bool fields that default to true.
fn default_true() -> bool {
    true
}

/// Default number of MRU tabs to cycle through with Ctrl+Tab.
fn default_tab_cycle_depth() -> u32 {
    10
}

impl EditorSettings {
    /// Returns the path to the settings file.
    pub fn settings_path() -> Result<PathBuf, ConfigError> {
        Ok(paths::config_dir()?.join("settings.json"))
    }

    /// Load settings from disk, returning defaults if the file does not exist.
    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::settings_path()?;
        if !path.exists() {
            tracing::debug!("no settings file found, using defaults");
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(&path)?;
        let settings = serde_json::from_str(&contents)?;
        Ok(settings)
    }

    /// Save current settings to disk, creating the config directory if needed.
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::settings_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, contents)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_editor_settings_default_theme() {
        let settings = EditorSettings::default();
        assert_eq!(
            settings.theme, "Adwaita-dark",
            "default theme should be Adwaita-dark"
        );
    }

    #[test]
    fn test_editor_settings_default_font_size() {
        let settings = EditorSettings::default();
        assert_eq!(settings.font_size, 13, "default font size should be 13");
    }

    #[test]
    fn test_editor_settings_default_tab_width() {
        let settings = EditorSettings::default();
        assert_eq!(settings.tab_width, 4, "default tab width should be 4");
    }

    #[test]
    fn test_editor_settings_default_show_line_numbers() {
        let settings = EditorSettings::default();
        assert!(
            settings.show_line_numbers,
            "line numbers should be shown by default"
        );
    }

    #[test]
    fn test_editor_settings_default_wrap_text() {
        let settings = EditorSettings::default();
        assert!(
            !settings.wrap_text,
            "text wrapping should be off by default"
        );
    }

    #[test]
    fn test_editor_settings_serde_round_trip() {
        let original = EditorSettings {
            theme: "monokai".to_owned(),
            editor_font_family: "Fira Code".to_owned(),
            font_size: 16,
            tab_width: 2,
            insert_spaces: false,
            show_line_numbers: false,
            wrap_text: true,
            terminal_font_family: "JetBrains Mono".to_owned(),
            terminal_font_size: 14,
            open_last_project: false,
            last_project_path: Some("/tmp/test".to_owned()),
            search_auto_expand_threshold: 10,
            use_treesitter: false,
            tab_cycle_depth: 5,
        };

        let json = serde_json::to_string(&original).expect("serialization should succeed in test");
        let restored: EditorSettings =
            serde_json::from_str(&json).expect("deserialization should succeed in test");

        assert_eq!(
            restored.theme, original.theme,
            "theme should survive round-trip"
        );
        assert_eq!(
            restored.font_size, original.font_size,
            "font_size should survive round-trip"
        );
        assert_eq!(
            restored.tab_width, original.tab_width,
            "tab_width should survive round-trip"
        );
        assert_eq!(
            restored.show_line_numbers, original.show_line_numbers,
            "show_line_numbers should survive round-trip"
        );
        assert_eq!(
            restored.wrap_text, original.wrap_text,
            "wrap_text should survive round-trip"
        );
    }

    #[test]
    fn test_editor_settings_serde_default_round_trip() {
        let original = EditorSettings::default();
        let json = serde_json::to_string(&original).expect("serialization should succeed in test");
        let restored: EditorSettings =
            serde_json::from_str(&json).expect("deserialization should succeed in test");

        assert_eq!(
            restored.theme, original.theme,
            "default theme should survive round-trip"
        );
        assert_eq!(
            restored.font_size, original.font_size,
            "default font_size should survive round-trip"
        );
    }
}
