//! Error types for skill discovery and loading.

/// Errors that can occur when parsing or loading a skill.
#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    /// The `SKILL.md` file does not begin with a `---` frontmatter fence.
    #[error("missing YAML frontmatter in {path}")]
    MissingFrontmatter {
        /// Path to the offending `SKILL.md`.
        path: std::path::PathBuf,
    },

    /// A required frontmatter field was missing or empty.
    #[error("missing required frontmatter field '{field}' in {path}")]
    MissingField {
        /// The missing field name.
        field: &'static str,
        /// Path to the offending `SKILL.md`.
        path: std::path::PathBuf,
    },

    /// The frontmatter `name` did not match the containing directory name.
    #[error("skill name '{name}' does not match directory name '{dir_name}' ({path})")]
    NameMismatch {
        /// The name declared in the frontmatter.
        name: String,
        /// The actual directory name.
        dir_name: String,
        /// Path to the offending `SKILL.md`.
        path: std::path::PathBuf,
    },

    /// Failed to read a skill file.
    #[error("I/O error reading {path}: {source}")]
    Io {
        /// Path to the file we tried to read.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to deserialize the YAML frontmatter.
    #[error("YAML parse error in {path}: {source}")]
    Yaml {
        /// Path to the offending `SKILL.md`.
        path: std::path::PathBuf,
        /// The underlying YAML error.
        #[source]
        source: serde_yaml::Error,
    },
}
