//! Maps tree-sitter highlight capture names to GtkSourceView style IDs.
//!
//! Tree-sitter highlight queries produce capture names like `keyword`, `function`,
//! `type.builtin`, etc. GtkSourceView themes define colors for style IDs like
//! `def:keyword`, `def:function`, `def:type`. This module provides the mapping
//! between the two systems.

/// The set of highlight names recognized by the engine.
///
/// These are passed to `HighlightConfiguration::configure()` to determine which
/// captures produce highlight events. The order defines the index used in
/// [`HighlightSpan::highlight_index`](crate::HighlightSpan::highlight_index).
pub const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "function",
    "function.builtin",
    "function.macro",
    "keyword",
    "label",
    "module",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

/// Maps a highlight name (by index into [`HIGHLIGHT_NAMES`]) to a GtkSourceView
/// style scheme ID.
///
/// Returns `None` for captures that should not be styled (e.g. plain punctuation).
pub fn highlight_to_style_id(index: usize) -> Option<&'static str> {
    let name = HIGHLIGHT_NAMES.get(index)?;
    match *name {
        "attribute" => Some("def:preprocessor"),
        "comment" => Some("def:comment"),
        "constant" => Some("def:constant"),
        "constant.builtin" => Some("def:special-constant"),
        "constructor" => Some("def:type"),
        "embedded" => Some("def:preprocessor"),
        "function" | "function.builtin" => Some("def:function"),
        "function.macro" => Some("def:preprocessor"),
        "keyword" => Some("def:keyword"),
        "label" => Some("def:identifier"),
        "module" => Some("def:identifier"),
        "number" => Some("def:number"),
        "operator" => Some("def:operator"),
        "property" => Some("def:identifier"),
        "string" => Some("def:string"),
        "string.special" => Some("def:special-char"),
        "tag" => Some("def:keyword"),
        "type" | "type.builtin" => Some("def:type"),
        "variable" => None, // Don't style regular variables — too noisy
        "variable.builtin" => Some("def:builtin"),
        "variable.parameter" => None,
        // Punctuation — don't style
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
    fn test_punctuation_not_styled() {
        let idx = HIGHLIGHT_NAMES
            .iter()
            .position(|&n| n == "punctuation.bracket")
            .expect("punctuation.bracket should be in HIGHLIGHT_NAMES");
        assert_eq!(
            highlight_to_style_id(idx),
            None,
            "punctuation should not be styled"
        );
    }

    #[test]
    fn test_out_of_bounds_returns_none() {
        assert_eq!(
            highlight_to_style_id(999),
            None,
            "out-of-bounds index should return None"
        );
    }

    #[test]
    fn test_all_highlight_names_have_mapping() {
        // Every name should be handled (return Some or explicitly None)
        // This test just ensures no panic occurs
        for i in 0..HIGHLIGHT_NAMES.len() {
            let _ = highlight_to_style_id(i);
        }
    }
}
