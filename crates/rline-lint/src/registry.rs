//! Registry that maps a [`SupportedLanguage`] to its formatter and lint
//! provider.
//!
//! Built from a [`LintSettings`] snapshot — toggling a language off or
//! overriding a binary path takes effect by rebuilding the registry.

use std::sync::Arc;

use rline_syntax::languages::SupportedLanguage;

use crate::provider::{Formatter, LintProvider};
use crate::settings::LintSettings;

/// Registered formatter and/or lint provider for a single language.
#[derive(Clone, Default)]
pub struct LanguageEntry {
    /// Formatter, if one is configured and enabled.
    pub formatter: Option<Arc<dyn Formatter>>,
    /// Lint provider, if one is configured and enabled.
    pub linter: Option<Arc<dyn LintProvider>>,
}

impl std::fmt::Debug for LanguageEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LanguageEntry")
            .field("formatter", &self.formatter.as_ref().map(|f| f.name()))
            .field("linter", &self.linter.as_ref().map(|l| l.name()))
            .finish()
    }
}

/// A snapshot of formatters and linters keyed by language.
#[derive(Debug, Clone, Default)]
pub struct LintRegistry {
    rust: LanguageEntry,
    python: LanguageEntry,
    javascript: LanguageEntry,
    ruby: LanguageEntry,
}

impl LintRegistry {
    /// Build a registry from settings.
    pub fn from_settings(settings: &LintSettings) -> Self {
        let mut reg = Self::default();

        if settings.rust_format_enabled {
            reg.rust.formatter = Some(Arc::new(crate::rust::RustFmt::new(
                settings.rust_fmt_binary().to_owned(),
            )));
        }
        if settings.rust_lint_enabled {
            reg.rust.linter = Some(Arc::new(crate::rust::CargoClippy::new(
                settings.rust_clippy_binary().to_owned(),
            )));
        }

        if settings.python_format_enabled {
            reg.python.formatter = Some(Arc::new(crate::python::RuffFormat::new(
                settings.python_ruff_binary().to_owned(),
            )));
        }
        if settings.python_lint_enabled {
            reg.python.linter = Some(Arc::new(crate::python::RuffCheck::new(
                settings.python_ruff_binary().to_owned(),
            )));
        }

        if settings.javascript_format_enabled {
            reg.javascript.formatter = Some(Arc::new(crate::javascript::Prettier::new(
                settings.prettier_binary().to_owned(),
            )));
        }
        if settings.javascript_lint_enabled {
            reg.javascript.linter = Some(Arc::new(crate::javascript::Eslint::new(
                settings.eslint_binary().to_owned(),
            )));
        }

        if settings.ruby_format_enabled {
            reg.ruby.formatter = Some(Arc::new(crate::ruby::RubocopFormat::new(
                settings.rubocop_binary().to_owned(),
            )));
        }
        if settings.ruby_lint_enabled {
            reg.ruby.linter = Some(Arc::new(crate::ruby::Rubocop::new(
                settings.rubocop_binary().to_owned(),
            )));
        }

        reg
    }

    /// Look up the entry for a language, or an empty entry if none is
    /// registered.
    pub fn entry(&self, language: SupportedLanguage) -> LanguageEntry {
        match language {
            SupportedLanguage::Rust => self.rust.clone(),
            SupportedLanguage::Python => self.python.clone(),
            SupportedLanguage::JavaScript => self.javascript.clone(),
            SupportedLanguage::Ruby => self.ruby.clone(),
            _ => LanguageEntry::default(),
        }
    }

    /// Iterate over every language that has a registered lint provider.
    pub fn linters(&self) -> Vec<(SupportedLanguage, Arc<dyn LintProvider>)> {
        let mut out: Vec<(SupportedLanguage, Arc<dyn LintProvider>)> = Vec::new();
        if let Some(ref l) = self.rust.linter {
            out.push((SupportedLanguage::Rust, l.clone()));
        }
        if let Some(ref l) = self.python.linter {
            out.push((SupportedLanguage::Python, l.clone()));
        }
        if let Some(ref l) = self.javascript.linter {
            out.push((SupportedLanguage::JavaScript, l.clone()));
        }
        if let Some(ref l) = self.ruby.linter {
            out.push((SupportedLanguage::Ruby, l.clone()));
        }
        out
    }
}
