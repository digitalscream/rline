//! XDG-compliant path resolution for rline configuration and data.

use std::path::PathBuf;

use crate::error::ConfigError;

/// Returns the configuration directory for rline (`~/.config/rline/`).
pub fn config_dir() -> Result<PathBuf, ConfigError> {
    let dirs = directories::ProjectDirs::from("", "", "rline").ok_or(ConfigError::NoConfigDir)?;
    Ok(dirs.config_dir().to_path_buf())
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
