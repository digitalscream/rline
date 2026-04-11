//! Configurable keyboard shortcut bindings.
//!
//! Each field stores a GTK accelerator string (e.g. `"<Ctrl>S"`, `"<Ctrl><Shift>F"`).
//! The struct is serialised alongside [`EditorSettings`] in `settings.json`
//! and provides sensible defaults matching the built-in shortcut table.

use serde::{Deserialize, Serialize};

/// A human-readable description paired with the internal action name.
///
/// Used by the shortcuts dialog to render labels and to map back to
/// `set_accels_for_action`.
#[derive(Debug, Clone)]
pub struct ShortcutDescriptor {
    /// GTK action name (e.g. `"win.open-file"`).
    pub action: &'static str,
    /// Human-readable label shown in the shortcuts dialog.
    pub label: &'static str,
}

/// All configurable keyboard shortcuts, stored as GTK accelerator strings.
///
/// Missing or empty fields deserialise to their defaults via `#[serde(default)]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyBindings {
    /// Open file (Ctrl+O).
    pub open_file: String,
    /// Save file (Ctrl+S).
    pub save_file: String,
    /// Close current editor tab (Ctrl+W).
    pub close_tab: String,
    /// Quick open / fuzzy file finder (Ctrl+P).
    pub quick_open: String,
    /// Find in current file (Ctrl+F).
    pub find: String,
    /// Find and replace in current file (Ctrl+H).
    pub find_replace: String,
    /// Split editor vertically (Ctrl+\).
    pub split_editor: String,
    /// Trigger AI completion (Ctrl+Space).
    pub trigger_completion: String,
    /// Focus project search (Ctrl+Shift+F).
    pub project_search: String,
    /// Show git panel (Ctrl+Shift+G).
    pub show_git: String,
    /// Show files panel (Ctrl+Shift+E).
    pub show_files: String,
    /// Focus terminal (Ctrl+Shift+W).
    pub focus_terminal: String,
    /// Focus agent panel (Ctrl+Shift+A).
    pub focus_agent: String,
    /// Quit application (Ctrl+Q).
    pub quit_app: String,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            open_file: "<Ctrl>O".to_owned(),
            save_file: "<Ctrl>S".to_owned(),
            close_tab: "<Ctrl>W".to_owned(),
            quick_open: "<Ctrl>P".to_owned(),
            find: "<Ctrl>F".to_owned(),
            find_replace: "<Ctrl>H".to_owned(),
            split_editor: "<Ctrl>backslash".to_owned(),
            trigger_completion: "<Ctrl>space".to_owned(),
            project_search: "<Ctrl><Shift>F".to_owned(),
            show_git: "<Ctrl><Shift>G".to_owned(),
            show_files: "<Ctrl><Shift>E".to_owned(),
            focus_terminal: "<Ctrl><Shift>W".to_owned(),
            focus_agent: "<Ctrl><Shift>A".to_owned(),
            quit_app: "<Ctrl>Q".to_owned(),
        }
    }
}

/// Ordered list of all shortcut descriptors.
///
/// The order here determines the display order in the shortcuts dialog.
pub const SHORTCUT_DESCRIPTORS: &[ShortcutDescriptor] = &[
    ShortcutDescriptor {
        action: "win.open-file",
        label: "Open file",
    },
    ShortcutDescriptor {
        action: "win.save-file",
        label: "Save file",
    },
    ShortcutDescriptor {
        action: "win.close-tab",
        label: "Close current editor tab",
    },
    ShortcutDescriptor {
        action: "win.quick-open",
        label: "Quick open (fuzzy file finder)",
    },
    ShortcutDescriptor {
        action: "win.find",
        label: "Find in current file",
    },
    ShortcutDescriptor {
        action: "win.find-replace",
        label: "Find and replace",
    },
    ShortcutDescriptor {
        action: "win.split-editor",
        label: "Split editor vertically",
    },
    ShortcutDescriptor {
        action: "win.trigger-completion",
        label: "Trigger AI completion",
    },
    ShortcutDescriptor {
        action: "win.project-search",
        label: "Focus project search",
    },
    ShortcutDescriptor {
        action: "win.show-git",
        label: "Show git panel",
    },
    ShortcutDescriptor {
        action: "win.show-files",
        label: "Show files panel",
    },
    ShortcutDescriptor {
        action: "win.focus-terminal",
        label: "Focus terminal",
    },
    ShortcutDescriptor {
        action: "win.focus-agent",
        label: "Focus agent panel",
    },
    ShortcutDescriptor {
        action: "win.quit-app",
        label: "Quit application",
    },
];

impl KeyBindings {
    /// Look up the accelerator string for a given GTK action name.
    ///
    /// Returns `None` if the action name is not recognised.
    pub fn accel_for_action(&self, action: &str) -> Option<&str> {
        match action {
            "win.open-file" => Some(&self.open_file),
            "win.save-file" => Some(&self.save_file),
            "win.close-tab" => Some(&self.close_tab),
            "win.quick-open" => Some(&self.quick_open),
            "win.find" => Some(&self.find),
            "win.find-replace" => Some(&self.find_replace),
            "win.split-editor" => Some(&self.split_editor),
            "win.trigger-completion" => Some(&self.trigger_completion),
            "win.project-search" => Some(&self.project_search),
            "win.show-git" => Some(&self.show_git),
            "win.show-files" => Some(&self.show_files),
            "win.focus-terminal" => Some(&self.focus_terminal),
            "win.focus-agent" => Some(&self.focus_agent),
            "win.quit-app" => Some(&self.quit_app),
            _ => None,
        }
    }

    /// Set the accelerator string for a given GTK action name.
    ///
    /// Returns `true` if the action was recognised and updated.
    pub fn set_accel_for_action(&mut self, action: &str, accel: &str) -> bool {
        match action {
            "win.open-file" => self.open_file = accel.to_owned(),
            "win.save-file" => self.save_file = accel.to_owned(),
            "win.close-tab" => self.close_tab = accel.to_owned(),
            "win.quick-open" => self.quick_open = accel.to_owned(),
            "win.find" => self.find = accel.to_owned(),
            "win.find-replace" => self.find_replace = accel.to_owned(),
            "win.split-editor" => self.split_editor = accel.to_owned(),
            "win.trigger-completion" => self.trigger_completion = accel.to_owned(),
            "win.project-search" => self.project_search = accel.to_owned(),
            "win.show-git" => self.show_git = accel.to_owned(),
            "win.show-files" => self.show_files = accel.to_owned(),
            "win.focus-terminal" => self.focus_terminal = accel.to_owned(),
            "win.focus-agent" => self.focus_agent = accel.to_owned(),
            "win.quit-app" => self.quit_app = accel.to_owned(),
            _ => return false,
        }
        true
    }

    /// Convert a GTK accelerator string to a human-readable label.
    ///
    /// For example, `"<Ctrl><Shift>F"` becomes `"Ctrl+Shift+F"`.
    pub fn accel_to_label(accel: &str) -> String {
        if accel.is_empty() {
            return String::new();
        }
        let mut parts = Vec::new();
        let mut rest = accel;

        // Extract modifier tags like <Ctrl>, <Shift>, <Alt>, <Super>
        while let Some(open) = rest.find('<') {
            if let Some(close) = rest[open..].find('>') {
                let tag = &rest[open + 1..open + close];
                match tag {
                    "Ctrl" | "Control" | "Primary" => parts.push("Ctrl"),
                    "Shift" => parts.push("Shift"),
                    "Alt" | "Mod1" => parts.push("Alt"),
                    "Super" | "Meta" => parts.push("Super"),
                    _ => parts.push(tag),
                }
                rest = &rest[open + close + 1..];
            } else {
                break;
            }
        }

        // The remaining text is the key name
        if !rest.is_empty() {
            let key_label = match rest {
                "backslash" => "\\",
                "space" => "Space",
                "Return" | "KP_Enter" => "Enter",
                "Tab" | "ISO_Left_Tab" => "Tab",
                "Escape" => "Esc",
                "Delete" => "Del",
                "BackSpace" => "Backspace",
                "Home" => "Home",
                "End" => "End",
                "Page_Up" => "PgUp",
                "Page_Down" => "PgDn",
                "Up" => "Up",
                "Down" => "Down",
                "Left" => "Left",
                "Right" => "Right",
                other => other,
            };
            parts.push(key_label);
        }

        parts.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_keybindings_round_trip() {
        let original = KeyBindings::default();
        let json = serde_json::to_string(&original).expect("serialization should succeed in test");
        let restored: KeyBindings =
            serde_json::from_str(&json).expect("deserialization should succeed in test");
        assert_eq!(
            restored.open_file, original.open_file,
            "open_file should survive round-trip"
        );
        assert_eq!(
            restored.split_editor, original.split_editor,
            "split_editor should survive round-trip"
        );
    }

    #[test]
    fn test_missing_fields_use_defaults() {
        let json = r#"{"open_file": "<Ctrl><Shift>O"}"#;
        let kb: KeyBindings = serde_json::from_str(json).expect("should handle missing fields");
        assert_eq!(kb.open_file, "<Ctrl><Shift>O", "overridden field");
        assert_eq!(kb.save_file, "<Ctrl>S", "default for missing field");
    }

    #[test]
    fn test_accel_for_action_known() {
        let kb = KeyBindings::default();
        assert_eq!(
            kb.accel_for_action("win.save-file"),
            Some("<Ctrl>S"),
            "should return accel for known action"
        );
    }

    #[test]
    fn test_accel_for_action_unknown() {
        let kb = KeyBindings::default();
        assert_eq!(
            kb.accel_for_action("win.nonexistent"),
            None,
            "should return None for unknown action"
        );
    }

    #[test]
    fn test_set_accel_for_action() {
        let mut kb = KeyBindings::default();
        assert!(kb.set_accel_for_action("win.find", "<Ctrl><Shift>F"));
        assert_eq!(kb.find, "<Ctrl><Shift>F");
        assert!(!kb.set_accel_for_action("win.nonexistent", "x"));
    }

    #[test]
    fn test_accel_to_label_simple() {
        assert_eq!(KeyBindings::accel_to_label("<Ctrl>S"), "Ctrl+S");
    }

    #[test]
    fn test_accel_to_label_multi_modifier() {
        assert_eq!(
            KeyBindings::accel_to_label("<Ctrl><Shift>F"),
            "Ctrl+Shift+F"
        );
    }

    #[test]
    fn test_accel_to_label_special_key() {
        assert_eq!(KeyBindings::accel_to_label("<Ctrl>backslash"), "Ctrl+\\");
    }

    #[test]
    fn test_accel_to_label_space() {
        assert_eq!(KeyBindings::accel_to_label("<Ctrl>space"), "Ctrl+Space");
    }

    #[test]
    fn test_accel_to_label_empty() {
        assert_eq!(KeyBindings::accel_to_label(""), "");
    }

    #[test]
    fn test_descriptor_count_matches_fields() {
        let kb = KeyBindings::default();
        // Every descriptor should resolve to a known action
        for desc in SHORTCUT_DESCRIPTORS {
            assert!(
                kb.accel_for_action(desc.action).is_some(),
                "descriptor action '{}' should be recognised",
                desc.action
            );
        }
        assert_eq!(
            SHORTCUT_DESCRIPTORS.len(),
            14,
            "should have exactly 14 shortcut descriptors"
        );
    }
}
