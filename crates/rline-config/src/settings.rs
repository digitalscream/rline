//! Editor settings with persistence.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::keybindings::KeyBindings;
use crate::paths;

/// Editor settings that are persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    /// The GtkSourceView style scheme ID.
    pub theme: String,
    /// Editor font family.
    pub editor_font_family: String,
    /// Editor font size in pixels.
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
    /// Terminal font size in pixels.
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
    /// Extra letter spacing in pixels (can be fractional, e.g. 0.5).
    #[serde(default)]
    pub letter_spacing: f64,
    /// Line height multiplier (e.g. 1.4 means 140% of the font height).
    #[serde(default = "default_line_height")]
    pub line_height: f64,
    /// Font hinting level: "full" for maximum crispness, "slight" for smoother shapes.
    #[serde(default = "default_hint_style")]
    pub hint_style: String,

    // ── AI Completion ──
    /// Whether AI inline completion is enabled.
    #[serde(default)]
    pub ai_enabled: bool,
    /// Full URL to the OpenAI-compatible completions endpoint.
    #[serde(default = "default_ai_endpoint_url")]
    pub ai_endpoint_url: String,
    /// Bearer token for the completion API (empty = no auth header).
    #[serde(default)]
    pub ai_api_key: String,
    /// Model identifier sent in completion requests.
    #[serde(default)]
    pub ai_model: String,
    /// Milliseconds to wait after typing before requesting a completion.
    #[serde(default = "default_ai_debounce_ms")]
    pub ai_debounce_ms: u32,
    /// Maximum number of tokens to generate per completion.
    #[serde(default = "default_ai_max_tokens")]
    pub ai_max_tokens: u32,
    /// Number of lines before the cursor to include as context.
    #[serde(default = "default_ai_context_lines_before")]
    pub ai_context_lines_before: u32,
    /// Number of lines after the cursor to include as context.
    #[serde(default = "default_ai_context_lines_after")]
    pub ai_context_lines_after: u32,
    /// Maximum number of lines to display in a completion suggestion (0 = unlimited).
    #[serde(default = "default_ai_max_lines")]
    pub ai_max_lines: u32,
    /// Sampling temperature (0.0 = deterministic).
    #[serde(default)]
    pub ai_temperature: f64,
    /// Trigger mode: `"automatic"`, `"manual"`, or `"both"`.
    #[serde(default = "default_ai_trigger_mode")]
    pub ai_trigger_mode: String,

    // ── AI Agent ──
    /// Full URL to the chat completions endpoint for the agent.
    #[serde(default = "default_agent_endpoint_url")]
    pub agent_endpoint_url: String,
    /// Bearer token for the agent API (empty = no auth; if empty, falls back to `ai_api_key`).
    #[serde(default)]
    pub agent_api_key: String,
    /// Model identifier for agent requests.
    #[serde(default)]
    pub agent_model: String,
    /// Maximum tokens to generate per agent response.
    #[serde(default = "default_agent_max_tokens")]
    pub agent_max_tokens: u32,
    /// Sampling temperature for agent responses.
    #[serde(default)]
    pub agent_temperature: f64,
    /// Auto-approve read-only tool calls (read_file, list_files, search_files, etc.).
    #[serde(default = "default_true")]
    pub agent_auto_approve_read: bool,
    /// Auto-approve file edit tool calls (write_to_file, replace_in_file).
    #[serde(default)]
    pub agent_auto_approve_edit: bool,
    /// Auto-approve command execution tool calls.
    #[serde(default)]
    pub agent_auto_approve_command: bool,
    /// YOLO mode: skip approval for commands that affect the system outside
    /// the project directory (e.g. `apt install`, `sudo`, global npm installs).
    #[serde(default)]
    pub agent_yolo_mode: bool,
    /// Timeout in seconds for agent command execution.
    #[serde(default = "default_agent_command_timeout")]
    pub agent_command_timeout_secs: u32,
    /// Maximum context length in tokens for the agent model.
    #[serde(default = "default_agent_context_length")]
    pub agent_context_length: u32,
    /// Maximum number of tool-use turns before the agent stops.
    #[serde(default = "default_agent_max_turns")]
    pub agent_max_turns: u32,

    // ── Keyboard Shortcuts ──
    /// Configurable keyboard shortcut bindings.
    #[serde(default)]
    pub keybindings: KeyBindings,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            theme: "Adwaita-dark".to_owned(),
            editor_font_family: "Monospace".to_owned(),
            font_size: 15,
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
            letter_spacing: 0.0,
            line_height: default_line_height(),
            hint_style: default_hint_style(),
            ai_enabled: false,
            ai_endpoint_url: default_ai_endpoint_url(),
            ai_api_key: String::new(),
            ai_model: String::new(),
            ai_debounce_ms: default_ai_debounce_ms(),
            ai_max_tokens: default_ai_max_tokens(),
            ai_context_lines_before: default_ai_context_lines_before(),
            ai_context_lines_after: default_ai_context_lines_after(),
            ai_max_lines: default_ai_max_lines(),
            ai_temperature: 0.0,
            ai_trigger_mode: default_ai_trigger_mode(),
            agent_endpoint_url: default_agent_endpoint_url(),
            agent_api_key: String::new(),
            agent_model: String::new(),
            agent_max_tokens: default_agent_max_tokens(),
            agent_temperature: 0.0,
            agent_auto_approve_read: true,
            agent_auto_approve_edit: false,
            agent_auto_approve_command: false,
            agent_yolo_mode: false,
            agent_command_timeout_secs: default_agent_command_timeout(),
            agent_context_length: default_agent_context_length(),
            agent_max_turns: default_agent_max_turns(),
            keybindings: KeyBindings::default(),
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

/// Default line height multiplier.
fn default_line_height() -> f64 {
    1.4
}

/// Default font hinting level.
fn default_hint_style() -> String {
    "full".to_owned()
}

/// Default AI completion endpoint URL.
fn default_ai_endpoint_url() -> String {
    "http://localhost:8080/v1/completions".to_owned()
}

/// Default debounce delay for AI completion requests.
fn default_ai_debounce_ms() -> u32 {
    500
}

/// Default maximum tokens for AI completion.
fn default_ai_max_tokens() -> u32 {
    128
}

/// Default number of context lines before the cursor.
fn default_ai_context_lines_before() -> u32 {
    256
}

/// Default number of context lines after the cursor.
fn default_ai_context_lines_after() -> u32 {
    64
}

/// Default maximum lines for AI completion (0 = unlimited).
fn default_ai_max_lines() -> u32 {
    10
}

/// Default trigger mode for AI completion.
fn default_ai_trigger_mode() -> String {
    "automatic".to_owned()
}

/// Default agent endpoint URL.
fn default_agent_endpoint_url() -> String {
    "http://localhost:8080/v1/chat/completions".to_owned()
}

/// Default max tokens for agent responses.
fn default_agent_max_tokens() -> u32 {
    4096
}

/// Default command execution timeout for the agent.
fn default_agent_command_timeout() -> u32 {
    30
}

/// Default context length in tokens for the agent model.
fn default_agent_context_length() -> u32 {
    128_000
}

/// Default maximum tool-use turns for the agent loop.
fn default_agent_max_turns() -> u32 {
    50
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

/// Describes the open tabs in a single editor pane.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PaneState {
    /// Absolute paths of open files, in tab order.
    pub files: Vec<String>,
    /// Index of the active (focused) tab, if any.
    pub active_tab: Option<u32>,
}

/// Session state persisted across application restarts.
///
/// Stored separately from [`EditorSettings`] because it changes on every
/// tab open/close and should not clutter the user-editable settings file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionState {
    /// Files in the left (or only) editor pane.
    pub left: PaneState,
    /// Files in the right editor pane (populated only when split).
    pub right: Option<PaneState>,
}

impl SessionState {
    /// Returns the path to the session state file.
    fn session_path() -> Result<std::path::PathBuf, ConfigError> {
        Ok(paths::config_dir()?.join("session.json"))
    }

    /// Load session state from disk, returning an empty state if the file
    /// does not exist or cannot be parsed.
    pub fn load() -> Self {
        let path = match Self::session_path() {
            Ok(p) => p,
            Err(_) => return Self::default(),
        };
        if !path.exists() {
            return Self::default();
        }
        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("failed to read session state: {e}");
                return Self::default();
            }
        };
        serde_json::from_str(&contents).unwrap_or_default()
    }

    /// Save session state to disk.
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::session_path()?;
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
        assert_eq!(settings.font_size, 15, "default font size should be 15");
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
            letter_spacing: 0.5,
            line_height: 1.6,
            hint_style: "slight".to_owned(),
            ai_enabled: true,
            ai_endpoint_url: "http://example.com/v1/completions".to_owned(),
            ai_api_key: "sk-test-key".to_owned(),
            ai_model: "codellama".to_owned(),
            ai_debounce_ms: 300,
            ai_max_tokens: 256,
            ai_context_lines_before: 128,
            ai_context_lines_after: 32,
            ai_max_lines: 5,
            ai_temperature: 0.2,
            ai_trigger_mode: "both".to_owned(),
            agent_endpoint_url: "http://example.com/v1/chat/completions".to_owned(),
            agent_api_key: "agent-key".to_owned(),
            agent_model: "qwen-2.5".to_owned(),
            agent_max_tokens: 8192,
            agent_temperature: 0.1,
            agent_auto_approve_read: true,
            agent_auto_approve_edit: true,
            agent_auto_approve_command: false,
            agent_yolo_mode: false,
            agent_command_timeout_secs: 60,
            agent_context_length: 256_000,
            agent_max_turns: 75,
            keybindings: KeyBindings::default(),
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

    #[test]
    fn test_editor_settings_missing_ai_fields_use_defaults() {
        let json = r#"{"theme": "Adwaita-dark", "font_size": 15}"#;
        let settings: EditorSettings =
            serde_json::from_str(json).expect("should handle missing AI fields");

        assert!(!settings.ai_enabled, "ai_enabled should default to false");
        assert_eq!(
            settings.ai_endpoint_url, "http://localhost:8080/v1/completions",
            "ai_endpoint_url should have default value"
        );
        assert!(
            settings.ai_api_key.is_empty(),
            "ai_api_key should default to empty"
        );
        assert!(
            settings.ai_model.is_empty(),
            "ai_model should default to empty"
        );
        assert_eq!(
            settings.ai_debounce_ms, 500,
            "ai_debounce_ms should default to 500"
        );
        assert_eq!(
            settings.ai_max_tokens, 128,
            "ai_max_tokens should default to 128"
        );
        assert_eq!(
            settings.ai_trigger_mode, "automatic",
            "ai_trigger_mode should default to automatic"
        );
    }

    #[test]
    fn test_session_state_serde_round_trip() {
        let original = SessionState {
            left: PaneState {
                files: vec!["/tmp/a.rs".to_owned(), "/tmp/b.rs".to_owned()],
                active_tab: Some(1),
            },
            right: Some(PaneState {
                files: vec!["/tmp/c.rs".to_owned()],
                active_tab: Some(0),
            }),
        };

        let json = serde_json::to_string(&original).expect("serialization should succeed in test");
        let restored: SessionState =
            serde_json::from_str(&json).expect("deserialization should succeed in test");

        assert_eq!(
            restored.left.files, original.left.files,
            "left pane files should survive round-trip"
        );
        assert_eq!(
            restored.left.active_tab, original.left.active_tab,
            "left active tab should survive round-trip"
        );
        assert!(
            restored.right.is_some(),
            "right pane should be present after round-trip"
        );
        let right = restored.right.as_ref().unwrap();
        assert_eq!(
            right.files,
            original.right.as_ref().unwrap().files,
            "right pane files should survive round-trip"
        );
    }

    #[test]
    fn test_session_state_default_empty() {
        let state = SessionState::default();
        assert!(
            state.left.files.is_empty(),
            "default session should have no left pane files"
        );
        assert!(
            state.right.is_none(),
            "default session should have no right pane"
        );
    }

    #[test]
    fn test_session_state_missing_right_pane() {
        let json = r#"{"left": {"files": ["/tmp/a.rs"], "active_tab": 0}}"#;
        let restored: SessionState =
            serde_json::from_str(json).expect("deserialization should handle missing right pane");
        assert_eq!(restored.left.files.len(), 1, "should have one file");
        assert!(
            restored.right.is_none(),
            "right pane should be None when absent in JSON"
        );
    }
}
