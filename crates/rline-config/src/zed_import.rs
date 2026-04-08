//! Zed theme discovery, conversion, and installation.
//!
//! Discovers Zed themes from both extension directories
//! (`~/.local/share/zed/extensions/installed/`) and user themes
//! (`~/.config/zed/themes/`), converts them to GtkSourceView XML
//! schemes, and installs them alongside a [`SyntaxTheme`] JSON file
//! for rich syntax highlighting.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::ConfigError;
use crate::syntax_theme::SyntaxTheme;
use crate::vscode_import::{normalize_color, xml_escape};
use crate::{paths, vscode_import};

/// A discovered Zed theme ready for import.
#[derive(Debug, Clone)]
pub struct ZedThemeEntry {
    /// Display name of the individual theme (e.g., "Catppuccin Mocha").
    pub label: String,
    /// Family name from the top-level `"name"` field.
    pub family_name: String,
    /// Appearance: `"dark"` or `"light"`.
    pub appearance: String,
    /// Absolute path to the JSON file containing this theme.
    pub path: PathBuf,
    /// Index within the `"themes"` array in the JSON file.
    pub theme_index: usize,
    /// Source description (e.g., `"extension: aquarium-theme"` or `"user"`).
    pub source: String,
}

// ── Syntax mapping ────────────────────────────────────────────────────────

/// Maps Zed syntax keys to GtkSourceView style IDs.
const ZED_SYNTAX_MAP: &[(&str, &str)] = &[
    ("attribute", "def:preprocessor"),
    ("boolean", "def:boolean"),
    ("comment.doc", "def:doc-comment"),
    ("comment", "def:comment"),
    ("constant.builtin", "def:special-constant"),
    ("constant", "def:constant"),
    ("constructor", "def:type"),
    ("embedded", "def:preprocessor"),
    ("emphasis.strong", "def:strong-emphasis"),
    ("emphasis", "def:emphasis"),
    ("enum", "def:type"),
    ("function.definition", "def:function"),
    ("function.method", "def:function"),
    ("function", "def:function"),
    ("hint", "def:note"),
    ("keyword", "def:keyword"),
    ("label", "def:preprocessor"),
    ("link_text", "def:underlined"),
    ("link_uri", "def:underlined"),
    ("number", "def:number"),
    ("operator", "def:operator"),
    ("preproc", "def:preprocessor"),
    ("primary", "def:identifier"),
    ("property", "def:identifier"),
    ("punctuation.bracket", "def:operator"),
    ("punctuation.delimiter", "def:operator"),
    ("punctuation.list_marker", "def:operator"),
    ("punctuation.special.symbol", "def:special-char"),
    ("punctuation.special", "def:special-char"),
    ("punctuation", "def:operator"),
    ("string.escape", "def:special-char"),
    ("string.regex", "def:special-char"),
    ("string.special.symbol", "def:special-char"),
    ("string.special", "def:special-char"),
    ("string", "def:string"),
    ("tag", "def:keyword"),
    ("text.literal", "def:string"),
    ("title", "def:heading"),
    ("type.builtin", "def:type"),
    ("type.interface", "def:type"),
    ("type.super", "def:type"),
    ("type", "def:type"),
    ("variable.member", "def:identifier"),
    ("variable.parameter", "def:identifier"),
    ("variable.special", "def:builtin"),
    ("variable", "def:identifier"),
    ("variant", "def:type"),
];

/// Maps Zed UI color keys to VS Code UI color keys consumed by `theming.rs`.
pub const ZED_UI_COLOR_MAP: &[(&str, &str)] = &[
    ("background", "editor.background"),
    ("border", "sideBar.border"),
    ("created", "gitDecoration.untrackedResourceForeground"),
    ("deleted", "gitDecoration.deletedResourceForeground"),
    (
        "editor.active_line.background",
        "editor.lineHighlightBackground",
    ),
    (
        "editor.active_line_number",
        "editorLineNumber.activeForeground",
    ),
    ("editor.background", "editor.background"),
    ("editor.foreground", "editor.foreground"),
    ("editor.gutter.background", "editorGutter.background"),
    ("editor.line_number", "editorLineNumber.foreground"),
    ("element.hover", "button.background"),
    ("modified", "gitDecoration.modifiedResourceForeground"),
    ("panel.background", "sideBar.background"),
    (
        "search.match_background",
        "editor.findMatchHighlightBackground",
    ),
    ("status_bar.background", "statusBar.background"),
    ("tab.active_background", "tab.activeBackground"),
    ("tab.inactive_background", "tab.inactiveBackground"),
    ("tab_bar.background", "editorGroupHeader.tabsBackground"),
    ("terminal.background", "terminal.background"),
    ("terminal.foreground", "terminal.foreground"),
    ("text", "sideBar.foreground"),
    ("text.muted", "descriptionForeground"),
    ("title_bar.background", "titleBar.activeBackground"),
];

// ── Discovery ─────────────────────────────────────────────────────────────

/// Discover all available Zed themes on this system.
///
/// Scans both installed Zed extensions and user theme files.
/// Returns an alphabetically sorted list of individual themes.
pub fn discover_zed_themes() -> Vec<ZedThemeEntry> {
    let mut entries = Vec::new();

    // Extension themes
    for ext_dir in paths::zed_extension_dirs() {
        if let Err(e) = discover_extension_themes(&ext_dir, &mut entries) {
            tracing::debug!("skipping Zed extension {}: {e}", ext_dir.display());
        }
    }

    // User themes
    if let Some(user_dir) = paths::zed_user_themes_dir() {
        if let Err(e) = discover_user_themes(&user_dir, &mut entries) {
            tracing::debug!("error reading Zed user themes: {e}");
        }
    }

    entries.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    entries
}

/// Discover themes from a single Zed extension directory.
fn discover_extension_themes(
    ext_dir: &Path,
    entries: &mut Vec<ZedThemeEntry>,
) -> Result<(), ConfigError> {
    let toml_path = ext_dir.join("extension.toml");
    let toml_str = std::fs::read_to_string(&toml_path)?;
    let manifest: toml::Value =
        toml::from_str(&toml_str).map_err(|e| ConfigError::InvalidZedTheme(e.to_string()))?;

    let ext_name = manifest
        .get("id")
        .or_else(|| manifest.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let theme_paths = manifest
        .get("themes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    for rel_path in &theme_paths {
        let Some(rel_str) = rel_path.as_str() else {
            continue;
        };
        let json_path = ext_dir.join(rel_str);
        if json_path.exists() {
            let source = format!("extension: {ext_name}");
            if let Err(e) = enumerate_themes_in_file(&json_path, &source, entries) {
                tracing::debug!("skipping theme file {}: {e}", json_path.display());
            }
        }
    }

    Ok(())
}

/// Discover themes from the user themes directory.
fn discover_user_themes(
    user_dir: &Path,
    entries: &mut Vec<ZedThemeEntry>,
) -> Result<(), ConfigError> {
    for dir_entry in std::fs::read_dir(user_dir)? {
        let path = dir_entry?.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            if let Err(e) = enumerate_themes_in_file(&path, "user", entries) {
                tracing::debug!("skipping user theme {}: {e}", path.display());
            }
        }
    }
    Ok(())
}

/// Parse a single Zed theme JSON file and add each theme variant to `entries`.
fn enumerate_themes_in_file(
    path: &Path,
    source: &str,
    entries: &mut Vec<ZedThemeEntry>,
) -> Result<(), ConfigError> {
    let content = std::fs::read_to_string(path)?;
    let json: serde_json::Value = serde_json::from_str(&content)?;

    let family_name = json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let themes = json
        .get("themes")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ConfigError::InvalidZedTheme("missing \"themes\" array".into()))?;

    for (idx, theme) in themes.iter().enumerate() {
        let label = theme
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&family_name)
            .to_string();

        let appearance = theme
            .get("appearance")
            .and_then(|v| v.as_str())
            .unwrap_or("dark")
            .to_string();

        entries.push(ZedThemeEntry {
            label,
            family_name: family_name.clone(),
            appearance,
            path: path.to_path_buf(),
            theme_index: idx,
            source: source.to_string(),
        });
    }

    Ok(())
}

// ── Conversion ────────────────────────────────────────────────────────────

/// Generate a GtkSourceView scheme ID from a Zed theme name.
///
/// Lowercases, strips non-alphanumeric characters, and prefixes with `zed-`.
fn make_zed_scheme_id(name: &str) -> String {
    let mut id = String::with_capacity(name.len() + 4);
    id.push_str("zed-");

    let mut last_was_dash = true;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            id.push('-');
            last_was_dash = true;
        }
    }

    if id.ends_with('-') {
        id.pop();
    }

    id
}

/// Look up a Zed syntax key in the syntax map.
fn zed_syntax_to_gtksourceview(key: &str) -> Option<&'static str> {
    // Try exact match first, then progressively shorter prefixes.
    let mut search = key;
    loop {
        for &(zed_key, gsv_id) in ZED_SYNTAX_MAP {
            if zed_key == search {
                return Some(gsv_id);
            }
        }
        // Try parent scope (e.g., "punctuation.special.symbol" → "punctuation.special")
        match search.rfind('.') {
            Some(pos) => search = &search[..pos],
            None => return None,
        }
    }
}

/// Convert a Zed theme to GtkSourceView XML.
///
/// Returns `(scheme_id, xml_content)`.
pub fn convert_zed_to_gtksourceview(
    entry: &ZedThemeEntry,
) -> Result<(String, String), ConfigError> {
    let content = std::fs::read_to_string(&entry.path)?;
    let json: serde_json::Value = serde_json::from_str(&content)?;

    let themes = json
        .get("themes")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ConfigError::InvalidZedTheme("missing \"themes\" array".into()))?;

    let theme = themes.get(entry.theme_index).ok_or_else(|| {
        ConfigError::InvalidZedTheme(format!("theme index {} out of bounds", entry.theme_index))
    })?;

    let name = theme
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&entry.label);

    let scheme_id = make_zed_scheme_id(name);

    let style = theme
        .get("style")
        .ok_or_else(|| ConfigError::InvalidZedTheme("missing \"style\" object".into()))?;

    let variant = if entry.appearance == "light" {
        "light"
    } else {
        "dark"
    };

    // Extract editor colors
    let bg = style
        .get("editor.background")
        .or_else(|| style.get("background"))
        .and_then(|v| v.as_str())
        .map(normalize_color);
    let fg = style
        .get("editor.foreground")
        .or_else(|| style.get("text"))
        .and_then(|v| v.as_str())
        .map(normalize_color);

    // Extract cursor and selection from players[0]
    let player0 = style
        .get("players")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first());
    let cursor_color = player0
        .and_then(|p| p.get("cursor"))
        .and_then(|v| v.as_str())
        .map(normalize_color);
    let selection_color = player0
        .and_then(|p| p.get("selection"))
        .and_then(|v| v.as_str())
        .map(normalize_color);

    // Build XML
    let mut xml = String::with_capacity(4096);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str(&format!(
        "<style-scheme id=\"{}\" name=\"{}\" version=\"1.0\">\n",
        xml_escape(&scheme_id),
        xml_escape(name)
    ));
    xml.push_str(&format!(
        "  <metadata><property name=\"variant\">{variant}</property></metadata>\n"
    ));

    // Color definitions
    if let Some(ref bg) = bg {
        xml.push_str(&format!("  <color name=\"bg\" value=\"{bg}\"/>\n"));
    }
    if let Some(ref fg) = fg {
        xml.push_str(&format!("  <color name=\"fg\" value=\"{fg}\"/>\n"));
    }

    // Text style (editor background/foreground)
    let mut text_attrs = String::new();
    if let Some(ref fg) = fg {
        text_attrs.push_str(&format!(" foreground=\"{fg}\""));
    }
    if let Some(ref bg) = bg {
        text_attrs.push_str(&format!(" background=\"{bg}\""));
    }
    if !text_attrs.is_empty() {
        xml.push_str(&format!("  <style name=\"text\"{text_attrs}/>\n"));
    }

    // Selection
    if let Some(ref sel) = selection_color {
        xml.push_str(&format!(
            "  <style name=\"selection\" background=\"{sel}\"/>\n"
        ));
    }

    // Cursor
    if let Some(ref cur) = cursor_color {
        xml.push_str(&format!(
            "  <style name=\"cursor\" foreground=\"{cur}\"/>\n"
        ));
    }

    // Current line highlight
    if let Some(line_bg) = style
        .get("editor.active_line.background")
        .and_then(|v| v.as_str())
    {
        xml.push_str(&format!(
            "  <style name=\"current-line\" background=\"{}\"/>\n",
            normalize_color(line_bg)
        ));
    }

    // Line numbers
    if let Some(ln_fg) = style.get("editor.line_number").and_then(|v| v.as_str()) {
        xml.push_str(&format!(
            "  <style name=\"line-numbers\" foreground=\"{}\"/>\n",
            normalize_color(ln_fg)
        ));
    }

    // Search match
    if let Some(match_bg) = style
        .get("search.match_background")
        .and_then(|v| v.as_str())
    {
        xml.push_str(&format!(
            "  <style name=\"search-match\" background=\"{}\"/>\n",
            normalize_color(match_bg)
        ));
    }

    // Syntax styles
    let mut emitted_styles: HashSet<&str> = HashSet::new();

    if let Some(syntax) = style.get("syntax").and_then(|v| v.as_object()) {
        for (key, value) in syntax {
            let Some(gsv_id) = zed_syntax_to_gtksourceview(key) else {
                continue;
            };

            if !emitted_styles.insert(gsv_id) {
                continue; // Already emitted this GtkSourceView style
            }

            let color = value
                .get("color")
                .and_then(|v| v.as_str())
                .map(normalize_color);
            let font_style = value.get("font_style").and_then(|v| v.as_str());
            let font_weight = value.get("font_weight").and_then(|v| v.as_f64());

            if color.is_none() && font_style.is_none() && font_weight.is_none() {
                continue;
            }

            let mut attrs = String::new();
            if let Some(ref c) = color {
                attrs.push_str(&format!(" foreground=\"{}\"", xml_escape(c)));
            }
            if font_style == Some("italic") {
                attrs.push_str(" italic=\"true\"");
            }
            if font_weight.is_some_and(|w| w >= 700.0) {
                attrs.push_str(" bold=\"true\"");
            }

            xml.push_str(&format!("  <style name=\"{gsv_id}\"{attrs}/>\n"));
        }
    }

    xml.push_str("</style-scheme>\n");

    Ok((scheme_id, xml))
}

// ── Installation ──────────────────────────────────────────────────────────

/// Import a Zed theme: convert, install the GtkSourceView scheme, and save
/// a [`SyntaxTheme`] for rich highlighting.
///
/// Returns the GtkSourceView scheme ID.
pub fn import_zed_theme(entry: &ZedThemeEntry) -> Result<String, ConfigError> {
    let (scheme_id, xml) = convert_zed_to_gtksourceview(entry)?;

    // Install the GtkSourceView XML scheme
    vscode_import::install_gtksourceview_scheme(&scheme_id, &xml)?;

    // Build and save the rich SyntaxTheme
    let content = std::fs::read_to_string(&entry.path)?;
    let json: serde_json::Value = serde_json::from_str(&content)?;

    if let Some(theme) = json
        .get("themes")
        .and_then(|v| v.as_array())
        .and_then(|a| a.get(entry.theme_index))
    {
        let style = theme
            .get("style")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let syntax_theme = SyntaxTheme::from_zed_json(&scheme_id, &entry.label, &style);
        syntax_theme.save()?;
        tracing::info!("saved Zed SyntaxTheme for: {scheme_id}");
    }

    Ok(scheme_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zed_scheme_id_basic() {
        assert_eq!(
            make_zed_scheme_id("Catppuccin Mocha"),
            "zed-catppuccin-mocha"
        );
    }

    #[test]
    fn test_zed_scheme_id_special_chars() {
        assert_eq!(make_zed_scheme_id("One Dark Pro++"), "zed-one-dark-pro");
    }

    #[test]
    fn test_zed_scheme_id_prefix() {
        let id = make_zed_scheme_id("Aquarium");
        assert!(id.starts_with("zed-"), "scheme ID should start with 'zed-'");
    }

    #[test]
    fn test_syntax_lookup_exact() {
        assert_eq!(zed_syntax_to_gtksourceview("comment"), Some("def:comment"));
        assert_eq!(zed_syntax_to_gtksourceview("keyword"), Some("def:keyword"));
        assert_eq!(
            zed_syntax_to_gtksourceview("string.escape"),
            Some("def:special-char")
        );
    }

    #[test]
    fn test_syntax_lookup_fallback() {
        // Unknown sub-scope should fall back to parent
        assert_eq!(
            zed_syntax_to_gtksourceview("comment.line"),
            Some("def:comment")
        );
        assert_eq!(
            zed_syntax_to_gtksourceview("string.quoted.double"),
            Some("def:string")
        );
    }

    #[test]
    fn test_syntax_lookup_unknown() {
        assert_eq!(zed_syntax_to_gtksourceview("totally_unknown"), None);
    }

    #[test]
    fn test_convert_minimal_zed_theme() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let theme_json = serde_json::json!({
            "name": "Test Theme",
            "themes": [{
                "name": "Test Dark",
                "appearance": "dark",
                "style": {
                    "editor.background": "#1e1e2e",
                    "editor.foreground": "#cdd6f4",
                    "players": [
                        {"cursor": "#f5e0dc", "selection": "#45475a"}
                    ],
                    "syntax": {
                        "comment": {"color": "#6c7086", "font_style": "italic"},
                        "keyword": {"color": "#cba6f7"},
                        "string": {"color": "#a6e3a1"},
                        "function": {"color": "#89b4fa"},
                        "number": {"color": "#fab387"}
                    }
                }
            }]
        });

        let path = dir.path().join("test.json");
        std::fs::write(&path, theme_json.to_string()).expect("write test file");

        let entry = ZedThemeEntry {
            label: "Test Dark".into(),
            family_name: "Test Theme".into(),
            appearance: "dark".into(),
            path,
            theme_index: 0,
            source: "test".into(),
        };

        let (scheme_id, xml) = convert_zed_to_gtksourceview(&entry).expect("conversion");
        assert_eq!(scheme_id, "zed-test-dark");
        assert!(xml.contains("variant\">dark</"), "should be dark variant");
        assert!(xml.contains("#1e1e2e"), "should contain background color");
        assert!(xml.contains("#cdd6f4"), "should contain foreground color");
        assert!(xml.contains("def:comment"), "should map comment syntax");
        assert!(xml.contains("def:keyword"), "should map keyword syntax");
        assert!(xml.contains("def:string"), "should map string syntax");
        assert!(xml.contains("italic=\"true\""), "comment should be italic");
        assert!(xml.contains("name=\"cursor\""), "should have cursor style");
        assert!(
            xml.contains("name=\"selection\""),
            "should have selection style"
        );
    }

    #[test]
    fn test_convert_null_colors_skipped() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let theme_json = serde_json::json!({
            "name": "Sparse",
            "themes": [{
                "name": "Sparse Dark",
                "appearance": "dark",
                "style": {
                    "editor.background": "#000000",
                    "syntax": {
                        "comment": {"color": null},
                        "keyword": {"color": "#ff0000"}
                    }
                }
            }]
        });

        let path = dir.path().join("sparse.json");
        std::fs::write(&path, theme_json.to_string()).expect("write");

        let entry = ZedThemeEntry {
            label: "Sparse Dark".into(),
            family_name: "Sparse".into(),
            appearance: "dark".into(),
            path,
            theme_index: 0,
            source: "test".into(),
        };

        let (_, xml) = convert_zed_to_gtksourceview(&entry).expect("conversion");
        // comment with null color should be skipped
        assert!(
            !xml.contains("def:comment"),
            "null color comment should not produce a style"
        );
        assert!(xml.contains("def:keyword"), "keyword should be present");
    }

    #[test]
    fn test_convert_multi_theme_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let theme_json = serde_json::json!({
            "name": "Multi",
            "themes": [
                {
                    "name": "Multi Dark",
                    "appearance": "dark",
                    "style": {"editor.background": "#111111"}
                },
                {
                    "name": "Multi Light",
                    "appearance": "light",
                    "style": {"editor.background": "#ffffff"}
                }
            ]
        });

        let path = dir.path().join("multi.json");
        std::fs::write(&path, theme_json.to_string()).expect("write");

        // Import the second theme (index 1)
        let entry = ZedThemeEntry {
            label: "Multi Light".into(),
            family_name: "Multi".into(),
            appearance: "light".into(),
            path,
            theme_index: 1,
            source: "test".into(),
        };

        let (scheme_id, xml) = convert_zed_to_gtksourceview(&entry).expect("conversion");
        assert_eq!(scheme_id, "zed-multi-light");
        assert!(xml.contains("variant\">light</"), "should be light variant");
        assert!(xml.contains("#ffffff"), "should have light background");
    }
}
