//! XDG-compliant path resolution for rline configuration and data.

use std::path::PathBuf;

use crate::error::ConfigError;

/// Returns the configuration directory for rline (`~/.config/rline/`).
pub fn config_dir() -> Result<PathBuf, ConfigError> {
    let dirs = directories::ProjectDirs::from("", "", "rline").ok_or(ConfigError::NoConfigDir)?;
    Ok(dirs.config_dir().to_path_buf())
}

/// Returns the path to the custom agent system prompt file (`~/.config/rline/system_prompt.md`).
pub fn system_prompt_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join("system_prompt.md"))
}

/// Returns the GtkSourceView 5 user styles directory.
///
/// This is where custom style scheme XML files should be installed so that
/// `StyleSchemeManager` discovers them automatically.
/// Typically `~/.local/share/gtksourceview-5/styles/`.
pub fn gtksourceview_styles_dir() -> Result<PathBuf, ConfigError> {
    let base = directories::BaseDirs::new().ok_or(ConfigError::NoDataDir)?;
    Ok(base.data_dir().join("gtksourceview-5").join("styles"))
}

/// Returns all VS Code extension directories found on this system.
///
/// Checks standard locations for VS Code, VS Code Insiders, VS Codium,
/// and Flatpak installations.
pub fn vscode_extension_dirs() -> Vec<PathBuf> {
    let Some(base) = directories::BaseDirs::new() else {
        return Vec::new();
    };
    let home = base.home_dir();

    let candidates = [
        home.join(".vscode/extensions"),
        home.join(".vscode-insiders/extensions"),
        home.join(".vscode-oss/extensions"),
        home.join(".var/app/com.visualstudio.code/data/vscode/extensions"),
    ];

    candidates.into_iter().filter(|p| p.is_dir()).collect()
}

/// Returns directories containing installed Zed theme extensions.
///
/// Checks `~/.local/share/zed/extensions/installed/` for extension
/// directories that may contain theme JSON files.
pub fn zed_extension_dirs() -> Vec<PathBuf> {
    let Some(base) = directories::BaseDirs::new() else {
        return Vec::new();
    };

    let installed = base
        .data_dir()
        .join("zed")
        .join("extensions")
        .join("installed");

    if !installed.is_dir() {
        return Vec::new();
    }

    match std::fs::read_dir(&installed) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Returns the Zed user themes directory (`~/.config/zed/themes/`).
pub fn zed_user_themes_dir() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    let dir = base.config_dir().join("zed").join("themes");
    if dir.is_dir() {
        Some(dir)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_dir_ends_with_rline() {
        let dir = config_dir().expect("config_dir should succeed in test environment");
        let dir_str = dir.to_string_lossy();
        assert!(
            dir_str.ends_with("rline"),
            "config_dir should end with 'rline', got: {dir_str}"
        );
    }

    #[test]
    fn test_config_dir_returns_ok() {
        let result = config_dir();
        assert!(
            result.is_ok(),
            "config_dir should return Ok on a system with a home directory"
        );
    }
}
