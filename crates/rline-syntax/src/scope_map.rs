//! Maps tree-sitter highlight capture names to theme scopes.
//!
//! Provides two mapping layers:
//! 1. **GtkSourceView fallback**: capture → `def:*` style ID (limited, ~20 categories)
//! 2. **TextMate scope**: capture → TextMate scope (rich, hierarchical matching against VS Code themes)
//!
//! The TextMate scope mapping enables VS Code themes to color tokens with full
//! granularity, while the GtkSourceView mapping serves as a fallback for native themes.

/// The set of highlight names recognized by the engine.
///
/// These are passed to `HighlightConfiguration::configure()` to determine which
/// captures produce highlight events. The order defines the index used in
/// [`HighlightSpan::highlight_index`](crate::HighlightSpan::highlight_index).
///
/// This list covers all captures produced by the supported tree-sitter grammars
/// (Rust, Python, JavaScript, C, C++, JSON, Bash, HTML, CSS, Markdown).
pub const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "comment.documentation",
    "constant",
    "constant.builtin",
    "constant.character",
    "constructor",
    "delimiter",
    "embedded",
    "escape",
    "function",
    "function.builtin",
    "function.macro",
    "function.method",
    "function.method.builtin",
    "function.special",
    "keyword",
    "keyword.directive",
    "keyword.operator",
    "label",
    "module",
    "namespace",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "string.escape",
    "string.special",
    "string.special.key",
    "string.special.regex",
    "string.special.symbol",
    "tag",
    "tag.error",
    "text.emphasis",
    "text.literal",
    "text.reference",
    "text.strong",
    "text.title",
    "text.uri",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

/// Maps a highlight name (by index into [`HIGHLIGHT_NAMES`]) to a TextMate scope.
///
/// This is used for resolving colors from VS Code themes, which use TextMate's
/// hierarchical scope system. Returns `None` for captures that have no meaningful
/// TextMate equivalent.
pub fn highlight_to_textmate_scope(index: usize) -> Option<&'static str> {
    let name = HIGHLIGHT_NAMES.get(index)?;
    match *name {
        "attribute" => Some("entity.other.attribute-name"),
        "comment" => Some("comment"),
        "comment.documentation" => Some("comment.block.documentation"),
        "constant" => Some("constant.other"),
        "constant.builtin" => Some("constant.language"),
        "constant.character" => Some("constant.character"),
        "constructor" => Some("entity.name.function.constructor"),
        "delimiter" => Some("punctuation.separator"),
        "embedded" => Some("meta.embedded"),
        "escape" => Some("constant.character.escape"),
        "function" => Some("entity.name.function"),
        "function.builtin" => Some("support.function"),
        "function.macro" => Some("entity.name.function.macro"),
        "function.method" => Some("entity.name.function.member"),
        "function.method.builtin" => Some("support.function.builtin"),
        "function.special" => Some("support.function.special"),
        "keyword" => Some("keyword"),
        "keyword.directive" => Some("keyword.control.directive"),
        "keyword.operator" => Some("keyword.operator"),
        "label" => Some("entity.name.label"),
        "module" => Some("entity.name.namespace"),
        "namespace" => Some("entity.name.namespace"),
        "number" => Some("constant.numeric"),
        "operator" => Some("keyword.operator"),
        "property" => Some("variable.other.property"),
        "punctuation" => Some("punctuation"),
        "punctuation.bracket" => Some("punctuation.bracket"),
        "punctuation.delimiter" => Some("punctuation.separator"),
        "punctuation.special" => Some("punctuation.special"),
        "string" => Some("string"),
        "string.escape" => Some("constant.character.escape"),
        "string.special" => Some("string.special"),
        "string.special.key" => Some("support.type.property-name"),
        "string.special.regex" => Some("string.regexp"),
        "string.special.symbol" => Some("constant.other.symbol"),
        "tag" => Some("entity.name.tag"),
        "tag.error" => Some("invalid.illegal"),
        "text.emphasis" => Some("markup.italic"),
        "text.literal" => Some("markup.inline.raw"),
        "text.reference" => Some("markup.underline.link"),
        "text.strong" => Some("markup.bold"),
        "text.title" => Some("markup.heading"),
        "text.uri" => Some("markup.underline.link"),
        "type" => Some("entity.name.type"),
        "type.builtin" => Some("support.type"),
        "variable" => Some("variable"),
        "variable.builtin" => Some("variable.language"),
        "variable.parameter" => Some("variable.parameter"),
        _ => None,
    }
}

/// Maps a highlight name (by index into [`HIGHLIGHT_NAMES`]) to a GtkSourceView
/// style scheme ID.
///
/// This is the fallback used when no VS Code theme is active. Returns `None`
/// for captures that should not be styled under GtkSourceView's limited scheme.
pub fn highlight_to_style_id(index: usize) -> Option<&'static str> {
    let name = HIGHLIGHT_NAMES.get(index)?;
    match *name {
        "attribute" => Some("def:preprocessor"),
        "comment" | "comment.documentation" => Some("def:comment"),
        "constant" | "constant.character" => Some("def:constant"),
        "constant.builtin" => Some("def:special-constant"),
        "constructor" => Some("def:type"),
        "delimiter" => None,
        "embedded" => Some("def:preprocessor"),
        "escape" | "string.escape" => Some("def:special-char"),
        "function" | "function.method" => Some("def:function"),
        "function.builtin" | "function.method.builtin" | "function.special" => Some("def:builtin"),
        "function.macro" => Some("def:preprocessor"),
        "keyword" | "keyword.directive" | "keyword.operator" => Some("def:keyword"),
        "label" => Some("def:identifier"),
        "module" | "namespace" => Some("def:identifier"),
        "number" => Some("def:number"),
        "operator" => Some("def:operator"),
        "property" => Some("def:identifier"),
        "string" | "string.special" | "string.special.regex" | "string.special.symbol" => {
            Some("def:string")
        }
        "string.special.key" => Some("def:constant"),
        "tag" => Some("def:keyword"),
        "tag.error" => Some("def:error"),
        "text.emphasis" => Some("def:emphasis"),
        "text.literal" => Some("def:string"),
        "text.reference" | "text.uri" => Some("def:underlined"),
        "text.strong" => Some("def:strong-emphasis"),
        "text.title" => Some("def:keyword"),
        "type" | "type.builtin" => Some("def:type"),
        "variable" | "variable.parameter" => None,
        "variable.builtin" => Some("def:builtin"),
        "punctuation" | "punctuation.bracket" | "punctuation.delimiter" | "punctuation.special" => {
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_maps_to_def_keyword() {
        let idx = HIGHLIGHT_NAMES
            .iter()
            .position(|&n| n == "keyword")
            .expect("keyword should be in HIGHLIGHT_NAMES");
        assert_eq!(
            highlight_to_style_id(idx),
            Some("def:keyword"),
            "keyword should map to def:keyword"
        );
    }

    #[test]
    fn test_keyword_maps_to_textmate_keyword() {
        let idx = HIGHLIGHT_NAMES
            .iter()
            .position(|&n| n == "keyword")
            .expect("keyword should be in HIGHLIGHT_NAMES");
        assert_eq!(
            highlight_to_textmate_scope(idx),
            Some("keyword"),
            "keyword should map to textmate keyword"
        );
    }

    #[test]
    fn test_function_method_maps_to_textmate() {
        let idx = HIGHLIGHT_NAMES
            .iter()
            .position(|&n| n == "function.method")
            .expect("function.method should be in HIGHLIGHT_NAMES");
        assert_eq!(
            highlight_to_textmate_scope(idx),
            Some("entity.name.function.member"),
        );
    }

    #[test]
    fn test_variable_has_textmate_scope() {
        let idx = HIGHLIGHT_NAMES
            .iter()
            .position(|&n| n == "variable")
            .expect("variable should be in HIGHLIGHT_NAMES");
        assert_eq!(
            highlight_to_textmate_scope(idx),
            Some("variable"),
            "variable should map to textmate variable scope"
        );
    }

    #[test]
    fn test_string_special_symbol_maps_to_textmate() {
        let idx = HIGHLIGHT_NAMES
            .iter()
            .position(|&n| n == "string.special.symbol")
            .expect("string.special.symbol should be in HIGHLIGHT_NAMES");
        assert_eq!(
            highlight_to_textmate_scope(idx),
            Some("constant.other.symbol"),
        );
    }

    #[test]
    fn test_punctuation_not_styled_in_gtksourceview() {
        let idx = HIGHLIGHT_NAMES
            .iter()
            .position(|&n| n == "punctuation.bracket")
            .expect("punctuation.bracket should be in HIGHLIGHT_NAMES");
        assert_eq!(highlight_to_style_id(idx), None);
    }

    #[test]
    fn test_out_of_bounds_returns_none() {
        assert_eq!(highlight_to_style_id(999), None);
        assert_eq!(highlight_to_textmate_scope(999), None);
    }

    #[test]
    fn test_all_highlight_names_have_textmate_mapping() {
        for (i, &name) in HIGHLIGHT_NAMES.iter().enumerate() {
            let scope = highlight_to_textmate_scope(i);
            // All names should have a TextMate mapping except delimiter
            if name != "delimiter" {
                assert!(
                    scope.is_some(),
                    "capture {name} (index {i}) should have a TextMate scope mapping"
                );
            }
        }
    }
}
