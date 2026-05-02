//! User-configurable lint and format settings.
//!
//! Lives here (rather than in `rline-config`) so the registry can reference
//! it without a config dependency. `rline-config::EditorSettings` embeds an
//! instance of [`LintSettings`].

use serde::{Deserialize, Serialize};

/// Per-language formatter and linter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LintSettings {
    /// Format every buffer before saving when a formatter is configured for
    /// its language. A formatter failure must never block the save itself.
    pub format_on_save: bool,

    // ── Rust ──
    /// Run `rustfmt` on Rust buffers.
    pub rust_format_enabled: bool,
    /// Run `cargo clippy` on the Rust project.
    pub rust_lint_enabled: bool,
    /// Per-language `format_on_save` override; `None` falls back to the
    /// global toggle.
    pub rust_format_on_save: Option<bool>,
    /// Optional override for the `rustfmt` binary (defaults to `rustfmt`).
    pub rust_fmt_binary_override: Option<String>,
    /// Optional override for the `cargo` binary (defaults to `cargo`).
    pub rust_cargo_binary_override: Option<String>,

    // ── Python ──
    /// Run `ruff format` on Python buffers.
    pub python_format_enabled: bool,
    /// Run `ruff check` on the Python project.
    pub python_lint_enabled: bool,
    /// Per-language `format_on_save` override.
    pub python_format_on_save: Option<bool>,
    /// Optional override for the `ruff` binary (defaults to `ruff`).
    pub python_ruff_binary_override: Option<String>,

    // ── JavaScript / TypeScript ──
    /// Run `prettier` on JS/TS buffers.
    pub javascript_format_enabled: bool,
    /// Run `eslint` on the JS/TS project.
    pub javascript_lint_enabled: bool,
    /// Per-language `format_on_save` override.
    pub javascript_format_on_save: Option<bool>,
    /// Optional override for the `prettier` binary (defaults to `prettier`).
    pub prettier_binary_override: Option<String>,
    /// Optional override for the `eslint` binary (defaults to `eslint`).
    pub eslint_binary_override: Option<String>,

    // ── Ruby ──
    /// Run `rubocop -A` on Ruby buffers.
    pub ruby_format_enabled: bool,
    /// Run `rubocop` on the Ruby project.
    pub ruby_lint_enabled: bool,
    /// Per-language `format_on_save` override.
    pub ruby_format_on_save: Option<bool>,
    /// Optional override for the `rubocop` binary (defaults to `rubocop`).
    pub rubocop_binary_override: Option<String>,
}

impl Default for LintSettings {
    fn default() -> Self {
        Self {
            format_on_save: false,
            rust_format_enabled: true,
            rust_lint_enabled: true,
            rust_format_on_save: None,
            rust_fmt_binary_override: None,
            rust_cargo_binary_override: None,
            python_format_enabled: true,
            python_lint_enabled: true,
            python_format_on_save: None,
            python_ruff_binary_override: None,
            javascript_format_enabled: true,
            javascript_lint_enabled: true,
            javascript_format_on_save: None,
            prettier_binary_override: None,
            eslint_binary_override: None,
            ruby_format_enabled: true,
            ruby_lint_enabled: true,
            ruby_format_on_save: None,
            rubocop_binary_override: None,
        }
    }
}

impl LintSettings {
    /// `rustfmt` binary, honoring any user override.
    pub fn rust_fmt_binary(&self) -> &str {
        self.rust_fmt_binary_override
            .as_deref()
            .unwrap_or("rustfmt")
    }
    /// `cargo` binary, honoring any user override.
    pub fn rust_clippy_binary(&self) -> &str {
        self.rust_cargo_binary_override
            .as_deref()
            .unwrap_or("cargo")
    }
    /// `ruff` binary, honoring any user override.
    pub fn python_ruff_binary(&self) -> &str {
        self.python_ruff_binary_override
            .as_deref()
            .unwrap_or("ruff")
    }
    /// `prettier` binary, honoring any user override.
    pub fn prettier_binary(&self) -> &str {
        self.prettier_binary_override
            .as_deref()
            .unwrap_or("prettier")
    }
    /// `eslint` binary, honoring any user override.
    pub fn eslint_binary(&self) -> &str {
        self.eslint_binary_override.as_deref().unwrap_or("eslint")
    }
    /// `rubocop` binary, honoring any user override.
    pub fn rubocop_binary(&self) -> &str {
        self.rubocop_binary_override.as_deref().unwrap_or("rubocop")
    }

    /// Resolve whether format-on-save applies to the given language identifier
    /// (one of `"rust"`, `"python"`, `"javascript"`, `"ruby"`). Returns
    /// `false` for any unknown identifier.
    pub fn should_format_on_save(&self, language_id: &str) -> bool {
        let per_lang = match language_id {
            "rust" => self.rust_format_on_save,
            "python" => self.python_format_on_save,
            "javascript" => self.javascript_format_on_save,
            "ruby" => self.ruby_format_on_save,
            // Unknown language: never auto-format. Only languages we
            // explicitly support inherit from the global toggle.
            _ => return false,
        };
        per_lang.unwrap_or(self.format_on_save)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sensible() {
        let s = LintSettings::default();
        assert!(!s.format_on_save, "format_on_save defaults off");
        assert!(s.rust_format_enabled);
        assert!(s.rust_lint_enabled);
        assert!(s.ruby_format_enabled);
    }

    #[test]
    fn binary_overrides_are_honored() {
        let s = LintSettings {
            rust_fmt_binary_override: Some("/opt/rustfmt".into()),
            ..LintSettings::default()
        };
        assert_eq!(s.rust_fmt_binary(), "/opt/rustfmt");
        assert_eq!(s.rust_clippy_binary(), "cargo", "fallback to default");
    }

    #[test]
    fn per_language_format_on_save_overrides_global() {
        let s = LintSettings {
            format_on_save: false,
            rust_format_on_save: Some(true),
            ..LintSettings::default()
        };
        assert!(s.should_format_on_save("rust"));
        assert!(!s.should_format_on_save("python"));
    }

    #[test]
    fn unknown_language_never_formats_on_save() {
        let s = LintSettings {
            format_on_save: true,
            ..LintSettings::default()
        };
        assert!(!s.should_format_on_save("brainfuck"));
    }

    #[test]
    fn round_trips_through_serde() {
        let original = LintSettings {
            format_on_save: true,
            rust_format_on_save: Some(false),
            rubocop_binary_override: Some("/usr/local/bin/rubocop".into()),
            ..LintSettings::default()
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: LintSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.format_on_save, original.format_on_save);
        assert_eq!(back.rust_format_on_save, original.rust_format_on_save);
        assert_eq!(
            back.rubocop_binary_override,
            original.rubocop_binary_override
        );
    }

    #[test]
    fn missing_fields_use_defaults() {
        let json = r#"{"format_on_save": true}"#;
        let s: LintSettings = serde_json::from_str(json).expect("deserialize");
        assert!(s.format_on_save);
        assert!(
            s.rust_format_enabled,
            "missing fields fall back to defaults"
        );
    }
}
