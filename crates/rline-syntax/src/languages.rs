//! Language grammar registry.
//!
//! Maps file extensions to tree-sitter language grammars and their associated
//! highlight queries. Each language is gated behind a feature flag so that
//! unused grammars can be excluded from the binary.

use tree_sitter_highlight::HighlightConfiguration;

use crate::error::SyntaxError;
use crate::scope_map::HIGHLIGHT_NAMES;

/// A language supported by the tree-sitter highlighting engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportedLanguage {
    /// Rust (.rs)
    #[cfg(feature = "lang-rust")]
    Rust,
    /// Python (.py, .pyi)
    #[cfg(feature = "lang-python")]
    Python,
    /// JavaScript (.js, .jsx, .mjs)
    #[cfg(feature = "lang-javascript")]
    JavaScript,
    /// C (.c, .h)
    #[cfg(feature = "lang-c")]
    C,
    /// C++ (.cpp, .cc, .cxx, .hpp, .hxx)
    #[cfg(feature = "lang-cpp")]
    Cpp,
    /// JSON (.json)
    #[cfg(feature = "lang-json")]
    Json,
    /// Bash (.sh, .bash)
    #[cfg(feature = "lang-bash")]
    Bash,
    /// HTML (.html, .htm)
    #[cfg(feature = "lang-html")]
    Html,
    /// CSS (.css)
    #[cfg(feature = "lang-css")]
    Css,
    /// Markdown (.md, .markdown)
    #[cfg(feature = "lang-markdown")]
    Markdown,
    /// Ruby (.rb, .rake, .gemspec)
    #[cfg(feature = "lang-ruby")]
    Ruby,
}

/// Look up the tree-sitter language for a file extension.
///
/// Returns `None` if no grammar is available for the extension.
///
/// # Examples
///
/// ```
/// use rline_syntax::languages::language_for_extension;
///
/// let lang = language_for_extension("rs");
/// # #[cfg(feature = "lang-rust")]
/// assert!(lang.is_some());
/// ```
pub fn language_for_extension(ext: &str) -> Option<SupportedLanguage> {
    match ext {
        #[cfg(feature = "lang-rust")]
        "rs" => Some(SupportedLanguage::Rust),

        #[cfg(feature = "lang-python")]
        "py" | "pyi" | "pyw" => Some(SupportedLanguage::Python),

        #[cfg(feature = "lang-javascript")]
        "js" | "jsx" | "mjs" | "cjs" => Some(SupportedLanguage::JavaScript),

        #[cfg(feature = "lang-c")]
        "c" | "h" => Some(SupportedLanguage::C),

        #[cfg(feature = "lang-cpp")]
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Some(SupportedLanguage::Cpp),

        #[cfg(feature = "lang-json")]
        "json" => Some(SupportedLanguage::Json),

        #[cfg(feature = "lang-bash")]
        "sh" | "bash" | "zsh" => Some(SupportedLanguage::Bash),

        #[cfg(feature = "lang-html")]
        "html" | "htm" => Some(SupportedLanguage::Html),

        #[cfg(feature = "lang-css")]
        "css" => Some(SupportedLanguage::Css),

        #[cfg(feature = "lang-markdown")]
        "md" | "markdown" => Some(SupportedLanguage::Markdown),

        #[cfg(feature = "lang-ruby")]
        "rb" | "rake" | "gemspec" => Some(SupportedLanguage::Ruby),

        _ => None,
    }
}

/// Build a [`HighlightConfiguration`] for the given language.
///
/// The configuration is pre-configured with [`HIGHLIGHT_NAMES`] so that
/// highlight events carry the correct indices.
pub fn build_highlight_config(
    language: SupportedLanguage,
) -> Result<HighlightConfiguration, SyntaxError> {
    let (ts_language, highlights_query, injections_query, locals_query) =
        language_components(language);

    let mut config = HighlightConfiguration::new(
        ts_language,
        language_name(language),
        highlights_query,
        injections_query,
        locals_query,
    )
    .map_err(|e| SyntaxError::QueryError(e.to_string()))?;

    config.configure(HIGHLIGHT_NAMES);
    Ok(config)
}

/// Returns the human-readable name for a language.
pub fn language_name(language: SupportedLanguage) -> &'static str {
    match language {
        #[cfg(feature = "lang-rust")]
        SupportedLanguage::Rust => "rust",
        #[cfg(feature = "lang-python")]
        SupportedLanguage::Python => "python",
        #[cfg(feature = "lang-javascript")]
        SupportedLanguage::JavaScript => "javascript",
        #[cfg(feature = "lang-c")]
        SupportedLanguage::C => "c",
        #[cfg(feature = "lang-cpp")]
        SupportedLanguage::Cpp => "cpp",
        #[cfg(feature = "lang-json")]
        SupportedLanguage::Json => "json",
        #[cfg(feature = "lang-bash")]
        SupportedLanguage::Bash => "bash",
        #[cfg(feature = "lang-html")]
        SupportedLanguage::Html => "html",
        #[cfg(feature = "lang-css")]
        SupportedLanguage::Css => "css",
        #[cfg(feature = "lang-markdown")]
        SupportedLanguage::Markdown => "markdown",
        #[cfg(feature = "lang-ruby")]
        SupportedLanguage::Ruby => "ruby",
    }
}

/// Returns the raw `tree_sitter::Language` for the given supported language.
///
/// Used by [`HighlightEngine`](crate::engine::HighlightEngine) to configure its parser.
pub fn ts_language(language: SupportedLanguage) -> tree_sitter::Language {
    match language {
        #[cfg(feature = "lang-rust")]
        SupportedLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
        #[cfg(feature = "lang-python")]
        SupportedLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        #[cfg(feature = "lang-javascript")]
        SupportedLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        #[cfg(feature = "lang-c")]
        SupportedLanguage::C => tree_sitter_c::LANGUAGE.into(),
        #[cfg(feature = "lang-cpp")]
        SupportedLanguage::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        #[cfg(feature = "lang-json")]
        SupportedLanguage::Json => tree_sitter_json::LANGUAGE.into(),
        #[cfg(feature = "lang-bash")]
        SupportedLanguage::Bash => tree_sitter_bash::LANGUAGE.into(),
        #[cfg(feature = "lang-html")]
        SupportedLanguage::Html => tree_sitter_html::LANGUAGE.into(),
        #[cfg(feature = "lang-css")]
        SupportedLanguage::Css => tree_sitter_css::LANGUAGE.into(),
        #[cfg(feature = "lang-markdown")]
        SupportedLanguage::Markdown => tree_sitter_md::LANGUAGE.into(),
        #[cfg(feature = "lang-ruby")]
        SupportedLanguage::Ruby => tree_sitter_ruby::LANGUAGE.into(),
    }
}

/// Returns the raw components needed to build a HighlightConfiguration.
fn language_components(
    language: SupportedLanguage,
) -> (
    tree_sitter::Language,
    &'static str,
    &'static str,
    &'static str,
) {
    match language {
        #[cfg(feature = "lang-rust")]
        SupportedLanguage::Rust => (
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            "",
        ),

        #[cfg(feature = "lang-python")]
        SupportedLanguage::Python => (
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::HIGHLIGHTS_QUERY,
            "",
            "",
        ),

        #[cfg(feature = "lang-javascript")]
        SupportedLanguage::JavaScript => (
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        ),

        #[cfg(feature = "lang-c")]
        SupportedLanguage::C => (
            tree_sitter_c::LANGUAGE.into(),
            tree_sitter_c::HIGHLIGHT_QUERY,
            "",
            "",
        ),

        #[cfg(feature = "lang-cpp")]
        SupportedLanguage::Cpp => (
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp::HIGHLIGHT_QUERY,
            "",
            "",
        ),

        #[cfg(feature = "lang-json")]
        SupportedLanguage::Json => (
            tree_sitter_json::LANGUAGE.into(),
            tree_sitter_json::HIGHLIGHTS_QUERY,
            "",
            "",
        ),

        #[cfg(feature = "lang-bash")]
        SupportedLanguage::Bash => (
            tree_sitter_bash::LANGUAGE.into(),
            tree_sitter_bash::HIGHLIGHT_QUERY,
            "",
            "",
        ),

        #[cfg(feature = "lang-html")]
        SupportedLanguage::Html => (
            tree_sitter_html::LANGUAGE.into(),
            tree_sitter_html::HIGHLIGHTS_QUERY,
            tree_sitter_html::INJECTIONS_QUERY,
            "",
        ),

        #[cfg(feature = "lang-css")]
        SupportedLanguage::Css => (
            tree_sitter_css::LANGUAGE.into(),
            tree_sitter_css::HIGHLIGHTS_QUERY,
            "",
            "",
        ),

        #[cfg(feature = "lang-markdown")]
        SupportedLanguage::Markdown => (
            tree_sitter_md::LANGUAGE.into(),
            tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
            tree_sitter_md::INJECTION_QUERY_BLOCK,
            "",
        ),

        #[cfg(feature = "lang-ruby")]
        SupportedLanguage::Ruby => (
            tree_sitter_ruby::LANGUAGE.into(),
            tree_sitter_ruby::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_ruby::LOCALS_QUERY,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_extension_mapping() {
        #[cfg(feature = "lang-rust")]
        assert_eq!(
            language_for_extension("rs"),
            Some(SupportedLanguage::Rust),
            ".rs should map to Rust"
        );
    }

    #[test]
    fn test_unknown_extension_returns_none() {
        assert_eq!(
            language_for_extension("xyz"),
            None,
            "unknown extension should return None"
        );
    }

    #[test]
    fn test_python_extensions() {
        #[cfg(feature = "lang-python")]
        {
            assert_eq!(
                language_for_extension("py"),
                Some(SupportedLanguage::Python)
            );
            assert_eq!(
                language_for_extension("pyi"),
                Some(SupportedLanguage::Python)
            );
        }
    }

    #[test]
    fn test_javascript_extensions() {
        #[cfg(feature = "lang-javascript")]
        {
            assert_eq!(
                language_for_extension("js"),
                Some(SupportedLanguage::JavaScript)
            );
            assert_eq!(
                language_for_extension("jsx"),
                Some(SupportedLanguage::JavaScript)
            );
            assert_eq!(
                language_for_extension("mjs"),
                Some(SupportedLanguage::JavaScript)
            );
        }
    }

    #[test]
    fn test_build_rust_config() {
        #[cfg(feature = "lang-rust")]
        {
            let config = build_highlight_config(SupportedLanguage::Rust);
            assert!(
                config.is_ok(),
                "Rust highlight config should build successfully"
            );
        }
    }

    #[test]
    fn test_build_all_configs() {
        let languages = [
            #[cfg(feature = "lang-rust")]
            SupportedLanguage::Rust,
            #[cfg(feature = "lang-python")]
            SupportedLanguage::Python,
            #[cfg(feature = "lang-javascript")]
            SupportedLanguage::JavaScript,
            #[cfg(feature = "lang-c")]
            SupportedLanguage::C,
            #[cfg(feature = "lang-cpp")]
            SupportedLanguage::Cpp,
            #[cfg(feature = "lang-json")]
            SupportedLanguage::Json,
            #[cfg(feature = "lang-bash")]
            SupportedLanguage::Bash,
            #[cfg(feature = "lang-html")]
            SupportedLanguage::Html,
            #[cfg(feature = "lang-css")]
            SupportedLanguage::Css,
            #[cfg(feature = "lang-markdown")]
            SupportedLanguage::Markdown,
            #[cfg(feature = "lang-ruby")]
            SupportedLanguage::Ruby,
        ];
        for lang in languages {
            let config = build_highlight_config(lang);
            assert!(
                config.is_ok(),
                "highlight config for {:?} should build: {:?}",
                lang,
                config.err()
            );
        }
    }
}
