//! XDG-compliant path resolution for rline configuration and data.

use std::path::PathBuf;

use crate::error::ConfigError;

/// Returns the configuration directory for rline (`~/.config/rline/`).
pub fn config_dir() -> Result<PathBuf, ConfigError> {
    let dirs = directories::ProjectDirs::from("", "", "rline").ok_or(ConfigError::NoConfigDir)?;
    Ok(dirs.config_dir().to_path_buf())
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
