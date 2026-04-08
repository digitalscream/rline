//! Rich syntax theme with hierarchical TextMate scope resolution.
//!
//! Unlike GtkSourceView's fixed ~20 style IDs, a [`SyntaxTheme`] stores
//! arbitrary TextMate scope → color/style mappings and resolves them using
//! hierarchical prefix matching (e.g., `comment.block.documentation` falls
//! back to `comment.block`, then `comment`).

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

/// Style attributes for a single token scope.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenStyle {
    /// Foreground color in `#RRGGBB` format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    /// Whether text should be bold.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    /// Whether text should be italic.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    /// Whether text should be underlined.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    /// Whether text should be struck through.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strikethrough: bool,
}

/// A rich syntax theme that maps TextMate scopes to colors and styles.
///
/// Stored alongside GtkSourceView XML schemes to provide full-granularity
/// token coloring for tree-sitter highlights.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntaxTheme {
    /// The GtkSourceView scheme ID this theme is associated with.
    pub scheme_id: String,
    /// Display name.
    pub name: String,
    /// Scope-to-style rules, ordered from most specific to least specific.
    /// Each key is a TextMate scope selector (e.g., `"comment.block.documentation"`).
    pub rules: Vec<ScopeRule>,
    /// VS Code UI colors (`editor.background`, `sideBar.background`, etc.).
    /// Absent for themes that weren't imported from VS Code.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub ui_colors: HashMap<String, String>,
}

/// A single scope-to-style rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeRule {
    /// TextMate scope selector(s). Can be a single scope or comma-separated.
    pub scope: String,
    /// Style to apply for matching tokens.
    pub style: TokenStyle,
}

impl SyntaxTheme {
    /// Resolve a TextMate scope to a [`TokenStyle`] using hierarchical matching.
    ///
    /// Finds the most specific matching rule: if both `comment` and
    /// `comment.block.documentation` match, the longer (more specific) scope wins.
    ///
    /// Returns `None` if no rule matches.
    pub fn resolve(&self, scope: &str) -> Option<&TokenStyle> {
        let mut best_match: Option<(usize, &TokenStyle)> = None;

        for rule in &self.rules {
            for rule_scope in rule.scope.split(',') {
                let rule_scope = rule_scope.trim();
                let matches = scope == rule_scope || scope.starts_with(&format!("{rule_scope}."));
                if matches {
                    let specificity = rule_scope.len();
                    if best_match.is_none_or(|(best_len, _)| specificity > best_len) {
                        best_match = Some((specificity, &rule.style));
                    }
                }
            }
        }

        best_match.map(|(_, style)| style)
    }

    /// Look up a VS Code UI color by key (e.g., `"sideBar.background"`).
    ///
    /// Returns the normalized `#RRGGBB` color, or `None` if not present.
    pub fn ui_color(&self, key: &str) -> Option<&str> {
        self.ui_colors.get(key).map(|s| s.as_str())
    }

    /// Returns the directory where syntax theme files are stored.
    ///
    /// Typically `~/.config/rline/themes/`.
    pub fn themes_dir() -> Result<PathBuf, ConfigError> {
        Ok(crate::paths::config_dir()?.join("themes"))
    }

    /// Load a syntax theme from disk for the given scheme ID.
    ///
    /// Returns `None` if no theme file exists for this scheme.
    pub fn load(scheme_id: &str) -> Result<Option<Self>, ConfigError> {
        let path = Self::theme_path(scheme_id)?;
        if !path.exists() {
            return Ok(None);
        }
        let contents = std::fs::read_to_string(&path)?;
        let theme = serde_json::from_str(&contents)?;
        Ok(Some(theme))
    }

    /// Save this syntax theme to disk.
    pub fn save(&self) -> Result<PathBuf, ConfigError> {
        let dir = Self::themes_dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", self.scheme_id));
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, contents)?;
        tracing::info!("saved syntax theme: {}", path.display());
        Ok(path)
    }

    /// Returns the path for a theme file given a scheme ID.
    fn theme_path(scheme_id: &str) -> Result<PathBuf, ConfigError> {
        Ok(Self::themes_dir()?.join(format!("{scheme_id}.json")))
    }

    /// Build a [`SyntaxTheme`] from a VS Code theme JSON value.
    ///
    /// Extracts `tokenColors` rules with their scopes and settings.
    pub fn from_vscode_json(scheme_id: &str, name: &str, theme: &serde_json::Value) -> Self {
        let token_colors = theme
            .get("tokenColors")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut rules = Vec::new();

        for rule in &token_colors {
            let settings = match rule.get("settings").and_then(|v| v.as_object()) {
                Some(s) => s,
                None => continue,
            };

            let foreground = settings
                .get("foreground")
                .and_then(|v| v.as_str())
                .map(normalize_color);
            let font_style = settings
                .get("fontStyle")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Skip rules with no foreground and no font style
            if foreground.is_none() && font_style.is_empty() {
                continue;
            }

            let style = TokenStyle {
                foreground,
                bold: font_style.contains("bold"),
                italic: font_style.contains("italic"),
                underline: font_style.contains("underline"),
                strikethrough: font_style.contains("strikethrough"),
            };

            let scopes = extract_scopes(rule);
            for scope in scopes {
                rules.push(ScopeRule {
                    scope,
                    style: style.clone(),
                });
            }
        }

        // Extract UI colors
        let mut ui_colors = HashMap::new();
        if let Some(colors) = theme.get("colors").and_then(|v| v.as_object()) {
            for (key, value) in colors {
                if let Some(color_str) = value.as_str() {
                    ui_colors.insert(key.clone(), normalize_color(color_str));
                }
            }
        }

        Self {
            scheme_id: scheme_id.to_string(),
            name: name.to_string(),
            rules,
            ui_colors,
        }
    }

    /// Build a [`SyntaxTheme`] from a Zed theme's `style` object.
    ///
    /// Extracts `syntax` entries as scope rules and maps Zed UI color keys
    /// to VS Code UI color keys so that `theming.rs` can consume them unchanged.
    pub fn from_zed_json(scheme_id: &str, name: &str, style: &serde_json::Value) -> Self {
        use crate::zed_import::ZED_UI_COLOR_MAP;

        let mut rules = Vec::new();

        // Extract syntax rules
        if let Some(syntax) = style.get("syntax").and_then(|v| v.as_object()) {
            for (key, value) in syntax {
                let color = value
                    .get("color")
                    .and_then(|v| v.as_str())
                    .map(normalize_color);
                let font_style = value.get("font_style").and_then(|v| v.as_str());
                let font_weight = value.get("font_weight").and_then(|v| v.as_f64());

                if color.is_none() && font_style.is_none() && font_weight.is_none() {
                    continue;
                }

                let style = TokenStyle {
                    foreground: color,
                    bold: font_weight.is_some_and(|w| w >= 700.0),
                    italic: font_style == Some("italic"),
                    underline: false,
                    strikethrough: false,
                };

                rules.push(ScopeRule {
                    scope: key.clone(),
                    style,
                });
            }
        }

        // Map Zed UI colors to VS Code UI color keys
        let mut ui_colors = HashMap::new();

        for &(zed_key, vscode_key) in ZED_UI_COLOR_MAP {
            if let Some(color) = style.get(zed_key).and_then(|v| v.as_str()) {
                ui_colors.insert(vscode_key.to_string(), normalize_color(color));
            }
        }

        // Special handling for players[0] cursor and selection
        if let Some(player0) = style
            .get("players")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
        {
            if let Some(cursor) = player0.get("cursor").and_then(|v| v.as_str()) {
                ui_colors.insert(
                    "editorCursor.foreground".to_string(),
                    normalize_color(cursor),
                );
            }
            if let Some(selection) = player0.get("selection").and_then(|v| v.as_str()) {
                ui_colors.insert(
                    "editor.selectionBackground".to_string(),
                    normalize_color(selection),
                );
            }
        }

        Self {
            scheme_id: scheme_id.to_string(),
            name: name.to_string(),
            rules,
            ui_colors,
        }
    }
}

/// Extract scopes from a tokenColors rule entry.
fn extract_scopes(rule: &serde_json::Value) -> Vec<String> {
    match rule.get("scope") {
        Some(serde_json::Value::String(s)) => s.split(',').map(|s| s.trim().to_string()).collect(),
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

/// Normalize a hex color, stripping alpha channel if present.
fn normalize_color(color: &str) -> String {
    let color = color.trim();
    if color.len() == 9 && color.starts_with('#') {
        color[..7].to_string()
    } else {
        color.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_theme() -> SyntaxTheme {
        SyntaxTheme {
            scheme_id: "test".into(),
            name: "Test".into(),
            rules: vec![
                ScopeRule {
                    scope: "comment".into(),
                    style: TokenStyle {
                        foreground: Some("#666666".into()),
                        italic: true,
                        ..Default::default()
                    },
                },
                ScopeRule {
                    scope: "comment.block.documentation".into(),
                    style: TokenStyle {
                        foreground: Some("#888888".into()),
                        italic: true,
                        ..Default::default()
                    },
                },
                ScopeRule {
                    scope: "keyword".into(),
                    style: TokenStyle {
                        foreground: Some("#c678dd".into()),
                        bold: true,
                        ..Default::default()
                    },
                },
                ScopeRule {
                    scope: "variable".into(),
                    style: TokenStyle {
                        foreground: Some("#e06c75".into()),
                        ..Default::default()
                    },
                },
                ScopeRule {
                    scope: "entity.name.function".into(),
                    style: TokenStyle {
                        foreground: Some("#61afef".into()),
                        ..Default::default()
                    },
                },
                ScopeRule {
                    scope: "constant.other.symbol".into(),
                    style: TokenStyle {
                        foreground: Some("#56b6c2".into()),
                        ..Default::default()
                    },
                },
            ],
            ui_colors: HashMap::new(),
        }
    }

    #[test]
    fn test_exact_match() {
        let theme = make_theme();
        let style = theme.resolve("keyword").expect("should match keyword");
        assert_eq!(style.foreground.as_deref(), Some("#c678dd"));
        assert!(style.bold);
    }

    #[test]
    fn test_hierarchical_fallback() {
        let theme = make_theme();
        // "comment.line" should fall back to "comment"
        let style = theme
            .resolve("comment.line")
            .expect("should match via fallback");
        assert_eq!(style.foreground.as_deref(), Some("#666666"));
    }

    #[test]
    fn test_specific_overrides_general() {
        let theme = make_theme();
        // "comment.block.documentation" should match the specific rule, not the general "comment"
        let style = theme
            .resolve("comment.block.documentation")
            .expect("should match specific rule");
        assert_eq!(style.foreground.as_deref(), Some("#888888"));
    }

    #[test]
    fn test_no_match_returns_none() {
        let theme = make_theme();
        assert!(theme.resolve("meta.brace.curly").is_none());
    }

    #[test]
    fn test_variable_distinct_from_function() {
        let theme = make_theme();
        let var_style = theme.resolve("variable").expect("should match variable");
        let func_style = theme
            .resolve("entity.name.function")
            .expect("should match function");
        assert_ne!(
            var_style.foreground, func_style.foreground,
            "variable and function should have distinct colors"
        );
    }

    #[test]
    fn test_symbol_has_unique_color() {
        let theme = make_theme();
        let sym_style = theme
            .resolve("constant.other.symbol")
            .expect("should match symbol");
        assert_eq!(sym_style.foreground.as_deref(), Some("#56b6c2"));
    }

    #[test]
    fn test_from_vscode_json() {
        let json = serde_json::json!({
            "tokenColors": [
                {
                    "scope": ["comment", "punctuation.definition.comment"],
                    "settings": {
                        "foreground": "#6a9955",
                        "fontStyle": "italic"
                    }
                },
                {
                    "scope": "keyword",
                    "settings": {
                        "foreground": "#569cd6",
                        "fontStyle": "bold"
                    }
                }
            ]
        });

        let theme = SyntaxTheme::from_vscode_json("test", "Test", &json);
        assert_eq!(theme.rules.len(), 3);

        let comment_style = theme.resolve("comment").expect("should resolve comment");
        assert_eq!(comment_style.foreground.as_deref(), Some("#6a9955"));
        assert!(comment_style.italic);

        let keyword_style = theme.resolve("keyword").expect("should resolve keyword");
        assert_eq!(keyword_style.foreground.as_deref(), Some("#569cd6"));
        assert!(keyword_style.bold);
    }

    #[test]
    fn test_serde_round_trip() {
        let theme = make_theme();
        let json = serde_json::to_string(&theme).expect("should serialize");
        let restored: SyntaxTheme = serde_json::from_str(&json).expect("should deserialize");
        assert_eq!(restored.scheme_id, theme.scheme_id);
        assert_eq!(restored.rules.len(), theme.rules.len());
    }
}
