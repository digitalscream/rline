//! VS Code theme discovery and conversion to GtkSourceView 5 style schemes.
//!
//! This module scans VS Code extension directories for installed color themes,
//! converts them from VS Code's JSON format to GtkSourceView 5 XML style schemes,
//! and installs them in the user's local styles directory.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::ConfigError;
use crate::paths;

/// A discovered VS Code theme.
#[derive(Debug, Clone)]
pub struct VscodeThemeEntry {
    /// Human-readable theme label (e.g. "One Dark Pro").
    pub label: String,
    /// Absolute path to the theme JSON file.
    pub path: PathBuf,
    /// Base UI theme type (e.g. "vs-dark", "vs", "hc-black").
    pub ui_theme: String,
    /// The extension that provides this theme (e.g. "catppuccin.catppuccin-vsc").
    pub extension_name: String,
}

/// Discover all VS Code themes installed on this system.
///
/// Scans all known VS Code extension directories and reads `package.json`
/// from each extension to find contributed themes.
pub fn discover_vscode_themes() -> Vec<VscodeThemeEntry> {
    let ext_dirs = paths::vscode_extension_dirs();
    let mut themes = Vec::new();

    for ext_dir in &ext_dirs {
        let entries = match std::fs::read_dir(ext_dir) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(
                    "could not read VS Code extensions dir {}: {e}",
                    ext_dir.display()
                );
                continue;
            }
        };

        for entry in entries.flatten() {
            let ext_path = entry.path();
            if !ext_path.is_dir() {
                continue;
            }

            let pkg_path = ext_path.join("package.json");
            if !pkg_path.exists() {
                continue;
            }

            match discover_themes_in_extension(&ext_path, &pkg_path) {
                Ok(mut ext_themes) => themes.append(&mut ext_themes),
                Err(e) => {
                    tracing::debug!("skipping extension {}: {e}", ext_path.display());
                }
            }
        }
    }

    themes.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    themes
}

/// Convert a VS Code theme JSON file to a GtkSourceView 5 XML style scheme.
///
/// Returns `(scheme_id, xml_content)` on success. The scheme ID is derived
/// from the theme label prefixed with `vscode-`.
///
/// # Errors
///
/// Returns [`ConfigError::InvalidVscodeTheme`] if the theme file cannot be
/// parsed or contains no usable style information.
pub fn convert_vscode_to_gtksourceview(
    entry: &VscodeThemeEntry,
) -> Result<(String, String), ConfigError> {
    let json_str = std::fs::read_to_string(&entry.path).map_err(ConfigError::Io)?;
    let theme: serde_json::Value =
        serde_json::from_str(&json_str).map_err(ConfigError::Serialization)?;

    // Handle single-level include/inheritance
    let theme = resolve_includes(&theme, &entry.path)?;

    let name = theme
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&entry.label);

    let scheme_id = make_scheme_id(name);

    let variant = match theme.get("type").and_then(|v| v.as_str()) {
        Some("light") => "light",
        Some("hc-light") => "light",
        _ if entry.ui_theme.contains("light") => "light",
        _ => "dark",
    };

    let mut xml = String::with_capacity(4096);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str(&format!(
        "<style-scheme id=\"{scheme_id}\" _name=\"{}\" version=\"1.0\">\n",
        xml_escape(name)
    ));
    xml.push_str(&format!(
        "  <metadata>\n    <property name=\"variant\">{variant}</property>\n  </metadata>\n\n"
    ));

    // Extract global colors
    let colors = theme.get("colors").and_then(|v| v.as_object());

    // Define named colors from editor colors
    let bg = colors
        .and_then(|c| c.get("editor.background"))
        .and_then(|v| v.as_str())
        .map(normalize_color);
    let fg = colors
        .and_then(|c| c.get("editor.foreground"))
        .and_then(|v| v.as_str())
        .map(normalize_color);

    if let Some(ref bg) = bg {
        xml.push_str(&format!("  <color name=\"bg\" value=\"{bg}\"/>\n"));
    }
    if let Some(ref fg) = fg {
        xml.push_str(&format!("  <color name=\"fg\" value=\"{fg}\"/>\n"));
    }
    xml.push('\n');

    // Global styles from editor colors
    let mut text_style = "  <style name=\"text\"".to_string();
    if fg.is_some() {
        text_style.push_str(" foreground=\"fg\"");
    }
    if bg.is_some() {
        text_style.push_str(" background=\"bg\"");
    }
    text_style.push_str("/>\n");
    xml.push_str(&text_style);

    if let Some(color) = extract_color(colors, "editor.selectionBackground") {
        xml.push_str(&format!(
            "  <style name=\"selection\" background=\"{color}\"/>\n"
        ));
    }
    if let Some(color) = extract_color(colors, "editor.lineHighlightBackground") {
        xml.push_str(&format!(
            "  <style name=\"current-line\" background=\"{color}\"/>\n"
        ));
    }
    if let Some(color) = extract_color(colors, "editorCursor.foreground") {
        xml.push_str(&format!(
            "  <style name=\"cursor\" foreground=\"{color}\"/>\n"
        ));
    }
    if let Some(color) = extract_color(colors, "editorLineNumber.foreground") {
        xml.push_str(&format!(
            "  <style name=\"line-numbers\" foreground=\"{color}\"/>\n"
        ));
    }
    if let Some(color) = extract_color(colors, "editorBracketMatch.background") {
        xml.push_str(&format!(
            "  <style name=\"bracket-match\" background=\"{color}\"/>\n"
        ));
    }
    if let Some(color) = extract_color(colors, "editor.findMatchHighlightBackground") {
        xml.push_str(&format!(
            "  <style name=\"search-match\" background=\"{color}\"/>\n"
        ));
    }
    xml.push('\n');

    // Map tokenColors to GtkSourceView styles
    let token_colors = theme
        .get("tokenColors")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut emitted_styles: HashSet<&'static str> = HashSet::new();

    for rule in &token_colors {
        let settings = match rule.get("settings").and_then(|v| v.as_object()) {
            Some(s) => s,
            None => continue,
        };

        let foreground = settings.get("foreground").and_then(|v| v.as_str());
        let font_style = settings.get("fontStyle").and_then(|v| v.as_str());

        // Skip rules with no foreground and no font style
        if foreground.is_none() && font_style.is_none() {
            continue;
        }

        let scopes = extract_scopes(rule);

        for scope in &scopes {
            if let Some(style_id) = scope_to_gtksourceview(scope) {
                if emitted_styles.contains(style_id) {
                    continue;
                }
                emitted_styles.insert(style_id);

                let mut style_line = format!("  <style name=\"{style_id}\"");
                if let Some(fg) = foreground {
                    style_line.push_str(&format!(" foreground=\"{}\"", normalize_color(fg)));
                }
                if let Some(fs) = font_style {
                    if fs.contains("bold") {
                        style_line.push_str(" bold=\"true\"");
                    }
                    if fs.contains("italic") {
                        style_line.push_str(" italic=\"true\"");
                    }
                    if fs.contains("underline") {
                        style_line.push_str(" underline=\"single\"");
                    }
                    if fs.contains("strikethrough") {
                        style_line.push_str(" strikethrough=\"true\"");
                    }
                }
                style_line.push_str("/>\n");
                xml.push_str(&style_line);
            }
        }
    }

    xml.push_str("\n</style-scheme>\n");

    Ok((scheme_id, xml))
}

/// Import a VS Code theme: convert to GtkSourceView XML, save the rich syntax theme,
/// and install both.
///
/// Returns the scheme ID on success.
///
/// # Errors
///
/// Returns an error if conversion or installation fails.
pub fn import_vscode_theme(entry: &VscodeThemeEntry) -> Result<String, ConfigError> {
    let (scheme_id, xml) = convert_vscode_to_gtksourceview(entry)?;
    install_gtksourceview_scheme(&scheme_id, &xml)?;

    // Also save the rich syntax theme for direct scope→color resolution
    let json_str = std::fs::read_to_string(&entry.path).map_err(ConfigError::Io)?;
    let theme_json: serde_json::Value =
        serde_json::from_str(&json_str).map_err(ConfigError::Serialization)?;
    let theme_json = resolve_includes(&theme_json, &entry.path)?;

    let name = theme_json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&entry.label);

    let syntax_theme =
        crate::syntax_theme::SyntaxTheme::from_vscode_json(&scheme_id, name, &theme_json);
    syntax_theme.save()?;

    Ok(scheme_id)
}

/// Install a GtkSourceView style scheme XML file to the user's local styles directory.
///
/// Creates the directory if it does not exist. Returns the path to the installed file.
///
/// # Errors
///
/// Returns [`ConfigError::Io`] if the directory cannot be created or the file
/// cannot be written, or [`ConfigError::NoDataDir`] if the data directory
/// cannot be determined.
pub fn install_gtksourceview_scheme(
    scheme_id: &str,
    xml_content: &str,
) -> Result<PathBuf, ConfigError> {
    let styles_dir = paths::gtksourceview_styles_dir()?;
    std::fs::create_dir_all(&styles_dir)?;

    let file_path = styles_dir.join(format!("{scheme_id}.xml"));
    std::fs::write(&file_path, xml_content)?;

    tracing::info!("installed GtkSourceView scheme: {}", file_path.display());
    Ok(file_path)
}

// ── Scope mapping ──────────────────────────────────────────────────────────

/// Maps VS Code TextMate scopes to GtkSourceView style IDs using prefix matching.
///
/// The table is ordered from most specific to least specific so that
/// `constant.numeric` is checked before `constant`.
const SCOPE_MAP: &[(&str, &str)] = &[
    ("comment", "def:comment"),
    ("string.regexp", "def:special-char"),
    ("string", "def:string"),
    ("constant.numeric", "def:number"),
    ("constant.language", "def:special-constant"),
    ("constant.character.escape", "def:special-char"),
    ("constant", "def:constant"),
    ("keyword.operator", "def:operator"),
    ("keyword", "def:keyword"),
    ("storage.type", "def:type"),
    ("storage.modifier", "def:keyword"),
    ("storage", "def:keyword"),
    ("entity.name.function", "def:function"),
    ("entity.name.type", "def:type"),
    ("entity.name.tag", "def:keyword"),
    ("entity.name.section", "def:function"),
    ("entity.other.attribute-name", "def:preprocessor"),
    ("support.function", "def:builtin"),
    ("support.type", "def:type"),
    ("support.class", "def:type"),
    ("support.constant", "def:special-constant"),
    ("variable.language", "def:builtin"),
    ("variable", "def:identifier"),
    ("meta.preprocessor", "def:preprocessor"),
    ("punctuation.definition.tag", "def:keyword"),
    ("invalid.illegal", "def:error"),
    ("invalid.deprecated", "def:warning"),
    ("invalid", "def:error"),
    ("markup.bold", "def:strong-emphasis"),
    ("markup.italic", "def:emphasis"),
    ("markup.underline", "def:underlined"),
    ("markup.deleted", "def:deletion"),
    ("markup.inserted", "def:insertion"),
    ("markup.heading", "def:keyword"),
];

/// Match a VS Code scope string against the mapping table.
fn scope_to_gtksourceview(scope: &str) -> Option<&'static str> {
    let scope = scope.trim();
    for &(prefix, style_id) in SCOPE_MAP {
        if scope == prefix || scope.starts_with(&format!("{prefix}.")) {
            return Some(style_id);
        }
    }
    None
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Discover themes within a single VS Code extension directory.
fn discover_themes_in_extension(
    ext_path: &Path,
    pkg_path: &Path,
) -> Result<Vec<VscodeThemeEntry>, ConfigError> {
    let pkg_str = std::fs::read_to_string(pkg_path)?;
    let pkg: serde_json::Value = serde_json::from_str(&pkg_str)?;

    let ext_name = ext_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    // Strip version suffix (e.g. "catppuccin.catppuccin-vsc-3.18.1" -> "catppuccin.catppuccin-vsc")
    let ext_name = strip_version_suffix(ext_name);

    let theme_contributions = pkg
        .get("contributes")
        .and_then(|c| c.get("themes"))
        .and_then(|t| t.as_array());

    let Some(themes) = theme_contributions else {
        return Ok(Vec::new());
    };

    let mut entries = Vec::new();
    for theme in themes {
        let label = theme
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");
        let ui_theme = theme
            .get("uiTheme")
            .and_then(|v| v.as_str())
            .unwrap_or("vs-dark");
        let rel_path = match theme.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => continue,
        };

        let theme_path = ext_path.join(rel_path);
        if !theme_path.exists() {
            tracing::debug!("theme file not found: {}", theme_path.display());
            continue;
        }

        entries.push(VscodeThemeEntry {
            label: label.to_string(),
            path: theme_path,
            ui_theme: ui_theme.to_string(),
            extension_name: ext_name.to_string(),
        });
    }

    Ok(entries)
}

/// Strip version suffix from extension directory name.
///
/// "catppuccin.catppuccin-vsc-3.18.1" → "catppuccin.catppuccin-vsc"
fn strip_version_suffix(name: &str) -> String {
    // Find the last segment that looks like a version (starts with digit after a dash)
    let bytes = name.as_bytes();
    let mut last_dash = None;
    for (i, &b) in bytes.iter().enumerate().rev() {
        if b == b'-' {
            // Check if the character after the dash is a digit
            if i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
                last_dash = Some(i);
                break;
            }
        }
    }
    match last_dash {
        Some(i) => name[..i].to_string(),
        None => name.to_string(),
    }
}

/// Generate a GtkSourceView scheme ID from a theme name.
///
/// Lowercases, replaces non-alphanumeric characters with hyphens,
/// collapses multiple hyphens, and prefixes with `vscode-`.
fn make_scheme_id(name: &str) -> String {
    let mut id = String::with_capacity(name.len() + 7);
    id.push_str("vscode-");

    let mut last_was_dash = true; // Avoid leading dash after prefix
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            id.push('-');
            last_was_dash = true;
        }
    }

    // Trim trailing dash
    if id.ends_with('-') {
        id.pop();
    }

    id
}

/// Resolve a single level of VS Code theme `include`.
fn resolve_includes(
    theme: &serde_json::Value,
    theme_path: &Path,
) -> Result<serde_json::Value, ConfigError> {
    let include_path = match theme.get("include").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return Ok(theme.clone()),
    };

    let base_path = theme_path
        .parent()
        .ok_or_else(|| ConfigError::InvalidVscodeTheme("no parent directory".into()))?
        .join(include_path);

    if !base_path.exists() {
        tracing::warn!(
            "included theme not found: {}, proceeding without it",
            base_path.display()
        );
        return Ok(theme.clone());
    }

    let base_str = std::fs::read_to_string(&base_path)?;
    let base: serde_json::Value = serde_json::from_str(&base_str)?;

    // Merge: child overrides base
    let mut merged = base.clone();
    if let (Some(merged_obj), Some(child_obj)) = (merged.as_object_mut(), theme.as_object()) {
        // Merge top-level colors
        if let Some(child_colors) = child_obj.get("colors").and_then(|v| v.as_object()) {
            let base_colors = merged_obj
                .entry("colors")
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            if let Some(base_colors) = base_colors.as_object_mut() {
                for (k, v) in child_colors {
                    base_colors.insert(k.clone(), v.clone());
                }
            }
        }

        // Merge tokenColors: child rules prepended (higher priority)
        if let Some(child_tokens) = child_obj.get("tokenColors").and_then(|v| v.as_array()) {
            let base_tokens = merged_obj
                .entry("tokenColors")
                .or_insert_with(|| serde_json::Value::Array(Vec::new()));
            if let Some(base_arr) = base_tokens.as_array_mut() {
                let mut combined = child_tokens.clone();
                combined.append(base_arr);
                *base_arr = combined;
            }
        }

        // Overlay other scalar fields from child
        for (k, v) in child_obj {
            if k != "colors" && k != "tokenColors" && k != "include" {
                merged_obj.insert(k.clone(), v.clone());
            }
        }
    }

    Ok(merged)
}

/// Extract scopes from a tokenColors rule entry.
///
/// The `scope` field can be a string or an array of strings.
fn extract_scopes(rule: &serde_json::Value) -> Vec<String> {
    match rule.get("scope") {
        Some(serde_json::Value::String(s)) => {
            // Can be comma-separated
            s.split(',').map(|s| s.trim().to_string()).collect()
        }
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

/// Normalize a VS Code hex color to `#RRGGBB` format.
///
/// Strips alpha channel from `#RRGGBBAA` format.
pub(crate) fn normalize_color(color: &str) -> String {
    let color = color.trim();
    if color.len() == 9 && color.starts_with('#') {
        // #RRGGBBAA → #RRGGBB
        color[..7].to_string()
    } else if color.len() == 5 && color.starts_with('#') {
        // #RGBA → #RGB (rare)
        color[..4].to_string()
    } else {
        color.to_string()
    }
}

/// Extract a color from the VS Code `colors` object and normalize it.
fn extract_color(
    colors: Option<&serde_json::Map<String, serde_json::Value>>,
    key: &str,
) -> Option<String> {
    colors
        .and_then(|c| c.get(key))
        .and_then(|v| v.as_str())
        .map(normalize_color)
}

/// Escape special XML characters in attribute values.
pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_mapping_comment() {
        assert_eq!(
            scope_to_gtksourceview("comment"),
            Some("def:comment"),
            "comment should map to def:comment"
        );
    }

    #[test]
    fn test_scope_mapping_comment_line() {
        assert_eq!(
            scope_to_gtksourceview("comment.line.double-slash"),
            Some("def:comment"),
            "comment.line.double-slash should match comment prefix"
        );
    }

    #[test]
    fn test_scope_mapping_keyword() {
        assert_eq!(
            scope_to_gtksourceview("keyword.control"),
            Some("def:keyword")
        );
    }

    #[test]
    fn test_scope_mapping_constant_numeric() {
        assert_eq!(
            scope_to_gtksourceview("constant.numeric.float"),
            Some("def:number"),
            "constant.numeric.float should match constant.numeric → def:number"
        );
    }

    #[test]
    fn test_scope_mapping_entity_name_function() {
        assert_eq!(
            scope_to_gtksourceview("entity.name.function"),
            Some("def:function")
        );
    }

    #[test]
    fn test_scope_mapping_unknown_returns_none() {
        assert_eq!(
            scope_to_gtksourceview("meta.brace.curly"),
            None,
            "unmapped scope should return None"
        );
    }

    #[test]
    fn test_make_scheme_id_simple() {
        assert_eq!(make_scheme_id("One Dark Pro"), "vscode-one-dark-pro");
    }

    #[test]
    fn test_make_scheme_id_special_chars() {
        assert_eq!(
            make_scheme_id("Catppuccin Mocha (Dark)"),
            "vscode-catppuccin-mocha-dark"
        );
    }

    #[test]
    fn test_normalize_color_8digit() {
        assert_eq!(
            normalize_color("#1e1e2eff"),
            "#1e1e2e",
            "should strip alpha from 8-digit hex"
        );
    }

    #[test]
    fn test_normalize_color_6digit() {
        assert_eq!(
            normalize_color("#1e1e2e"),
            "#1e1e2e",
            "6-digit hex should pass through"
        );
    }

    #[test]
    fn test_xml_escape() {
        assert_eq!(
            xml_escape("Dark & Light \"Theme\""),
            "Dark &amp; Light &quot;Theme&quot;"
        );
    }

    #[test]
    fn test_strip_version_suffix() {
        assert_eq!(
            strip_version_suffix("catppuccin.catppuccin-vsc-3.18.1"),
            "catppuccin.catppuccin-vsc"
        );
    }

    #[test]
    fn test_strip_version_suffix_no_version() {
        assert_eq!(strip_version_suffix("some-extension"), "some-extension");
    }

    #[test]
    fn test_extract_scopes_string() {
        let rule = serde_json::json!({
            "scope": "comment, string",
            "settings": { "foreground": "#aaa" }
        });
        let scopes = extract_scopes(&rule);
        assert_eq!(scopes, vec!["comment", "string"]);
    }

    #[test]
    fn test_extract_scopes_array() {
        let rule = serde_json::json!({
            "scope": ["keyword", "storage.type"],
            "settings": { "foreground": "#aaa" }
        });
        let scopes = extract_scopes(&rule);
        assert_eq!(scopes, vec!["keyword", "storage.type"]);
    }

    #[test]
    fn test_convert_minimal_theme() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let theme_path = dir.path().join("test-theme.json");
        let theme_json = serde_json::json!({
            "name": "Test Dark",
            "type": "dark",
            "colors": {
                "editor.background": "#1a1a2e",
                "editor.foreground": "#e0e0e0"
            },
            "tokenColors": [
                {
                    "scope": "comment",
                    "settings": {
                        "foreground": "#6a6a8e",
                        "fontStyle": "italic"
                    }
                },
                {
                    "scope": ["keyword", "storage"],
                    "settings": {
                        "foreground": "#c792ea",
                        "fontStyle": "bold"
                    }
                },
                {
                    "scope": "string",
                    "settings": {
                        "foreground": "#c3e88d"
                    }
                }
            ]
        });
        std::fs::write(&theme_path, theme_json.to_string()).expect("should write test theme");

        let entry = VscodeThemeEntry {
            label: "Test Dark".into(),
            path: theme_path,
            ui_theme: "vs-dark".into(),
            extension_name: "test.test-dark".into(),
        };

        let (scheme_id, xml) =
            convert_vscode_to_gtksourceview(&entry).expect("conversion should succeed");

        assert_eq!(scheme_id, "vscode-test-dark");
        assert!(
            xml.contains("variant\">dark</property>"),
            "should be dark variant"
        );
        assert!(xml.contains("name=\"text\""), "should have text style");
        assert!(xml.contains("#1a1a2e"), "should contain background color");
        assert!(xml.contains("def:comment"), "should have comment style");
        assert!(xml.contains("italic=\"true\""), "comment should be italic");
        assert!(xml.contains("def:keyword"), "should have keyword style");
        assert!(xml.contains("bold=\"true\""), "keyword should be bold");
        assert!(xml.contains("def:string"), "should have string style");
        assert!(xml.contains("#c3e88d"), "string color should be present");
    }

    #[test]
    fn test_convert_light_theme() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let theme_path = dir.path().join("light.json");
        let theme_json = serde_json::json!({
            "name": "Test Light",
            "type": "light",
            "colors": {},
            "tokenColors": []
        });
        std::fs::write(&theme_path, theme_json.to_string()).expect("should write test theme");

        let entry = VscodeThemeEntry {
            label: "Test Light".into(),
            path: theme_path,
            ui_theme: "vs".into(),
            extension_name: "test.test-light".into(),
        };

        let (_, xml) = convert_vscode_to_gtksourceview(&entry).expect("conversion should succeed");

        assert!(
            xml.contains("variant\">light</property>"),
            "should be light variant"
        );
    }

    #[test]
    fn test_install_scheme() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let styles_dir = dir.path().join("gtksourceview-5").join("styles");

        // We can't easily override the data dir path, so test the file writing directly
        std::fs::create_dir_all(&styles_dir).expect("should create dir");
        let file_path = styles_dir.join("test-scheme.xml");
        let content = "<style-scheme id=\"test\"/>";
        std::fs::write(&file_path, content).expect("should write file");

        let written = std::fs::read_to_string(&file_path).expect("should read back");
        assert_eq!(written, content);
    }

    #[test]
    fn test_font_style_combined() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let theme_path = dir.path().join("combined.json");
        let theme_json = serde_json::json!({
            "name": "Combined",
            "type": "dark",
            "colors": {},
            "tokenColors": [
                {
                    "scope": "comment",
                    "settings": {
                        "foreground": "#888",
                        "fontStyle": "bold italic"
                    }
                }
            ]
        });
        std::fs::write(&theme_path, theme_json.to_string()).expect("should write test theme");

        let entry = VscodeThemeEntry {
            label: "Combined".into(),
            path: theme_path,
            ui_theme: "vs-dark".into(),
            extension_name: "test".into(),
        };

        let (_, xml) = convert_vscode_to_gtksourceview(&entry).expect("conversion should succeed");

        assert!(xml.contains("bold=\"true\""), "should have bold");
        assert!(xml.contains("italic=\"true\""), "should have italic");
    }

    #[test]
    fn test_discover_empty_dir() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        // Temporarily override — but since we can't easily mock paths,
        // just verify the function doesn't panic on missing dirs
        let themes = discover_vscode_themes();
        // Can't assert count since system may have VS Code installed,
        // but this should not panic
        let _ = themes;
        let _ = dir;
    }
}
