//! Simple Markdown-to-Pango markup converter for AI responses.
//!
//! Handles the most common Markdown constructs that appear in AI output:
//! headings, bold, italic, inline code, code blocks, and lists.
//! The output is valid Pango markup for use with `gtk4::Label::set_markup()`.

/// Convert a Markdown string to Pango markup.
///
/// Supports: headings (#–###), bold (**), italic (*), inline code (`),
/// fenced code blocks (```), and unordered lists (- / *).
pub fn markdown_to_pango(input: &str) -> String {
    let mut output = String::with_capacity(input.len() * 2);
    let mut in_code_block = false;
    let mut code_block_buf = String::new();

    for line in input.lines() {
        if line.starts_with("```") {
            if in_code_block {
                // End code block — emit the buffered content.
                output.push_str("<tt>");
                output.push_str(&glib::markup_escape_text(&code_block_buf));
                output.push_str("</tt>\n");
                code_block_buf.clear();
                in_code_block = false;
            } else {
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            if !code_block_buf.is_empty() {
                code_block_buf.push('\n');
            }
            code_block_buf.push_str(line);
            continue;
        }

        // Headings.
        if let Some(rest) = line.strip_prefix("### ") {
            output.push_str(&format!(
                "<b>{}</b>\n",
                &convert_inline(&glib::markup_escape_text(rest.trim()))
            ));
            continue;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            output.push_str(&format!(
                "<b><big>{}</big></b>\n",
                &convert_inline(&glib::markup_escape_text(rest.trim()))
            ));
            continue;
        }
        if let Some(rest) = line.strip_prefix("# ") {
            output.push_str(&format!(
                "<b><big><big>{}</big></big></b>\n",
                &convert_inline(&glib::markup_escape_text(rest.trim()))
            ));
            continue;
        }

        // Unordered list items.
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            let indent_count = line.len() - trimmed.len();
            let indent = "  ".repeat(indent_count / 2);
            output.push_str(&format!(
                "{indent} \u{2022} {}\n",
                &convert_inline(&glib::markup_escape_text(rest))
            ));
            continue;
        }

        // Regular paragraph line.
        let escaped = glib::markup_escape_text(line);
        output.push_str(&convert_inline(&escaped));
        output.push('\n');
    }

    // Unclosed code block — emit what we have.
    if in_code_block && !code_block_buf.is_empty() {
        output.push_str("<tt>");
        output.push_str(&glib::markup_escape_text(&code_block_buf));
        output.push_str("</tt>\n");
    }

    // Trim trailing newline.
    while output.ends_with('\n') {
        output.pop();
    }

    output
}

/// Convert inline Markdown (bold, italic, code) within an already-escaped line.
fn convert_inline(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Inline code: `...`
        if chars[i] == '`' {
            if let Some(end) = find_closing(&chars, i + 1, '`') {
                result.push_str("<tt>");
                result.extend(&chars[i + 1..end]);
                result.push_str("</tt>");
                i = end + 1;
                continue;
            }
        }

        // Bold: **...**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_closing_pair(&chars, i + 2, '*', '*') {
                result.push_str("<b>");
                result.extend(&chars[i + 2..end]);
                result.push_str("</b>");
                i = end + 2;
                continue;
            }
        }

        // Italic: *...*  (single asterisk, not followed by another)
        if chars[i] == '*' && (i + 1 >= len || chars[i + 1] != '*') {
            if let Some(end) = find_closing(&chars, i + 1, '*') {
                // Make sure it's not empty.
                if end > i + 1 {
                    result.push_str("<i>");
                    result.extend(&chars[i + 1..end]);
                    result.push_str("</i>");
                    i = end + 1;
                    continue;
                }
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

/// Find the position of a closing delimiter character.
fn find_closing(chars: &[char], start: usize, delim: char) -> Option<usize> {
    (start..chars.len()).find(|&i| chars[i] == delim)
}

/// Find the position of a closing two-character delimiter.
fn find_closing_pair(chars: &[char], start: usize, d1: char, d2: char) -> Option<usize> {
    (start..chars.len().saturating_sub(1)).find(|&i| chars[i] == d1 && chars[i + 1] == d2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text() {
        let result = markdown_to_pango("Hello world");
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn test_bold() {
        let result = markdown_to_pango("This is **bold** text");
        assert!(result.contains("<b>bold</b>"));
    }

    #[test]
    fn test_italic() {
        let result = markdown_to_pango("This is *italic* text");
        assert!(result.contains("<i>italic</i>"));
    }

    #[test]
    fn test_inline_code() {
        let result = markdown_to_pango("Use `cargo build` here");
        assert!(result.contains("<tt>cargo build</tt>"));
    }

    #[test]
    fn test_heading() {
        let result = markdown_to_pango("# Title");
        assert!(result.contains("<b><big><big>Title</big></big></b>"));
    }

    #[test]
    fn test_code_block() {
        let result = markdown_to_pango("```\nfn main() {}\n```");
        assert!(result.contains("<tt>fn main() {}</tt>"));
    }

    #[test]
    fn test_list_items() {
        let result = markdown_to_pango("- first\n- second");
        assert!(result.contains("\u{2022} first"));
        assert!(result.contains("\u{2022} second"));
    }

    #[test]
    fn test_escapes_angle_brackets() {
        let result = markdown_to_pango("Use Vec<String> here");
        assert!(result.contains("&lt;"));
        assert!(result.contains("&gt;"));
    }
}
