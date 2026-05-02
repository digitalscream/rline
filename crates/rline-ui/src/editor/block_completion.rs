//! Language-aware block completion, auto-dedent, comment continuation, and
//! HTML/XML tag closing.
//!
//! Intercepts Enter and character keys to provide structural editing helpers:
//!
//! 1. **Brace expansion** — Enter between `{` and `}` expands to a multi-line block.
//! 2. **Keyword block completion** — Enter after a block keyword inserts the closing
//!    construct (Ruby `end`, Bash `fi`/`done`/`esac`, Python extra indent).
//! 3. **HTML/XML auto-close** — Enter between `<tag>` and `</tag>` expands; typing
//!    `</` auto-completes the closing tag.
//! 4. **Auto-dedent** — Typing a closing keyword (`end`, `fi`, `}`, …) at the start
//!    of a line reduces indentation by one level.
//! 5. **Smart comment continuation** — Enter inside a comment inserts the comment
//!    prefix on the next line; an empty comment line exits the continuation.

use std::cell::Cell;
use std::rc::Rc;

use glib::Propagation;
use gtk4::prelude::*;
use sourceview5::prelude::*;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Extract the leading whitespace from the line at `line_number`.
fn get_line_indent(buffer: &sourceview5::Buffer, line_number: i32) -> String {
    let Some(start) = buffer.iter_at_line(line_number) else {
        return String::new();
    };
    let mut end = start;
    while !end.ends_line() && end.char().is_whitespace() {
        if !end.forward_char() {
            break;
        }
    }
    buffer.text(&start, &end, false).to_string()
}

/// Build a single indentation unit from the view's settings.
fn make_indent_unit(view: &sourceview5::View) -> String {
    if view.is_insert_spaces_instead_of_tabs() {
        let width = view.indent_width().max(1) as usize;
        " ".repeat(width)
    } else {
        "\t".to_string()
    }
}

/// Return the sourceview5 language ID (e.g. `"rust"`, `"ruby"`, `"sh"`).
fn language_id(buffer: &sourceview5::Buffer) -> Option<String> {
    buffer.language().map(|l| l.id().to_string())
}

/// Return the full text of the given line.
fn line_text(buffer: &sourceview5::Buffer, line_number: i32) -> String {
    let Some(start) = buffer.iter_at_line(line_number) else {
        return String::new();
    };
    let mut end = start;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    buffer.text(&start, &end, false).to_string()
}

/// Return the text from the start of the line to `iter`.
fn text_before_cursor(buffer: &sourceview5::Buffer, iter: &gtk4::TextIter) -> String {
    let line = iter.line();
    let Some(start) = buffer.iter_at_line(line) else {
        return String::new();
    };
    buffer.text(&start, iter, false).to_string()
}

/// Return the text from `iter` to the end of the line.
fn text_after_cursor(buffer: &sourceview5::Buffer, iter: &gtk4::TextIter) -> String {
    let mut end = *iter;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    buffer.text(iter, &end, false).to_string()
}

// ---------------------------------------------------------------------------
// Feature 2: Keyword block completion tables
// ---------------------------------------------------------------------------

/// How a block keyword should be closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockStyle {
    /// Insert `end` on the closing line (Ruby).
    End,
    /// Bash `if` → `then` / `fi`.
    BashIf,
    /// Bash `for`/`while`/`until` → `do` / `done`.
    BashLoop,
    /// Bash `case` → `in` / `esac`.
    BashCase,
    /// Python — just add one extra indent level (no closing keyword).
    IndentOnly,
}

/// A keyword that opens a block.
#[derive(Debug, Clone, Copy)]
struct BlockRule {
    keyword: &'static str,
    style: BlockStyle,
}

/// Return the block-opening rules for the given sourceview language ID.
fn block_rules(lang_id: &str) -> &'static [BlockRule] {
    match lang_id {
        "ruby" => &[
            BlockRule {
                keyword: "if",
                style: BlockStyle::End,
            },
            BlockRule {
                keyword: "unless",
                style: BlockStyle::End,
            },
            BlockRule {
                keyword: "while",
                style: BlockStyle::End,
            },
            BlockRule {
                keyword: "until",
                style: BlockStyle::End,
            },
            BlockRule {
                keyword: "for",
                style: BlockStyle::End,
            },
            BlockRule {
                keyword: "def",
                style: BlockStyle::End,
            },
            BlockRule {
                keyword: "class",
                style: BlockStyle::End,
            },
            BlockRule {
                keyword: "module",
                style: BlockStyle::End,
            },
            BlockRule {
                keyword: "begin",
                style: BlockStyle::End,
            },
            BlockRule {
                keyword: "case",
                style: BlockStyle::End,
            },
        ],
        "sh" => &[
            BlockRule {
                keyword: "if",
                style: BlockStyle::BashIf,
            },
            BlockRule {
                keyword: "for",
                style: BlockStyle::BashLoop,
            },
            BlockRule {
                keyword: "while",
                style: BlockStyle::BashLoop,
            },
            BlockRule {
                keyword: "until",
                style: BlockStyle::BashLoop,
            },
            BlockRule {
                keyword: "case",
                style: BlockStyle::BashCase,
            },
        ],
        "python" | "python3" => &[
            BlockRule {
                keyword: "if",
                style: BlockStyle::IndentOnly,
            },
            BlockRule {
                keyword: "elif",
                style: BlockStyle::IndentOnly,
            },
            BlockRule {
                keyword: "else",
                style: BlockStyle::IndentOnly,
            },
            BlockRule {
                keyword: "while",
                style: BlockStyle::IndentOnly,
            },
            BlockRule {
                keyword: "for",
                style: BlockStyle::IndentOnly,
            },
            BlockRule {
                keyword: "def",
                style: BlockStyle::IndentOnly,
            },
            BlockRule {
                keyword: "class",
                style: BlockStyle::IndentOnly,
            },
            BlockRule {
                keyword: "with",
                style: BlockStyle::IndentOnly,
            },
            BlockRule {
                keyword: "try",
                style: BlockStyle::IndentOnly,
            },
            BlockRule {
                keyword: "except",
                style: BlockStyle::IndentOnly,
            },
            BlockRule {
                keyword: "finally",
                style: BlockStyle::IndentOnly,
            },
        ],
        _ => &[],
    }
}

/// Return the first identifier-like word of a line (after leading whitespace).
///
/// Splits on the same predicate used by [`match_block_rule`]: anything that
/// is not alphanumeric or `_`. Returns `None` if the line is blank or starts
/// with a separator.
fn first_word(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let word = trimmed
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()?;
    if word.is_empty() {
        None
    } else {
        Some(word)
    }
}

/// Whether a line is comment-only (first non-whitespace char is `#`).
fn is_hash_comment_line(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

/// Whether a Ruby line opens an `end`-terminated block (first-word keyword or
/// trailing `do` / `do |…|`).
fn ruby_line_is_opener(line: &str) -> bool {
    let trimmed = line.trim_start();
    if ruby_line_ends_with_do(trimmed) {
        return true;
    }
    matches!(
        first_word(trimmed),
        Some("if")
            | Some("unless")
            | Some("while")
            | Some("until")
            | Some("for")
            | Some("def")
            | Some("class")
            | Some("module")
            | Some("begin")
            | Some("case")
    )
}

/// Returns `true` if the block opener at `opener_line_idx` already has a
/// matching closer in the buffer.
///
/// For keyword-counting styles (Ruby `End`, Bash `BashIf`/`BashLoop`/
/// `BashCase`), this counts openers vs. closers across the **entire**
/// buffer. The opener under the cursor is itself one of the openers in the
/// count. If `openers > closers`, the file is missing a closer (most likely
/// for the line just typed) and we should insert one; if `openers <=
/// closers`, the file is already fully closed and we must not add another.
///
/// For [`BlockStyle::IndentOnly`] (Python) there is no closing keyword; the
/// helper looks at the next non-blank, non-comment line after the opener and
/// reports `true` if it is indented strictly deeper than the opener.
///
/// The keyword-counting heuristic is intentionally simple — it does not parse
/// strings or heredocs, so a literal `"end"` inside a Ruby string can throw
/// the count off. This matches the level of analysis other editors do for
/// the same feature and is good enough in practice.
fn block_already_closed_in_text(style: BlockStyle, text: &str, opener_line_idx: usize) -> bool {
    match style {
        BlockStyle::IndentOnly => {
            let mut lines = text.lines().skip(opener_line_idx);
            let Some(opener_line) = lines.next() else {
                return false;
            };
            let opener_indent = opener_line.len() - opener_line.trim_start().len();
            for line in lines {
                if line.trim().is_empty() || is_hash_comment_line(line) {
                    continue;
                }
                let indent = line.len() - line.trim_start().len();
                return indent > opener_indent;
            }
            false
        }
        BlockStyle::End => {
            let mut balance: i32 = 0;
            for line in text.lines() {
                if is_hash_comment_line(line) {
                    continue;
                }
                if ruby_line_is_opener(line) {
                    balance += 1;
                }
                if first_word(line) == Some("end") {
                    balance -= 1;
                }
            }
            balance <= 0
        }
        BlockStyle::BashIf | BlockStyle::BashLoop | BlockStyle::BashCase => {
            let (openers, closer, companions): (&[&str], &str, &[&str]) = match style {
                BlockStyle::BashIf => (&["if"], "fi", &[" then", ";then", "; then"]),
                BlockStyle::BashLoop => {
                    (&["for", "while", "until"], "done", &[" do", ";do", "; do"])
                }
                BlockStyle::BashCase => (&["case"], "esac", &[" in"]),
                _ => unreachable!(),
            };
            let mut balance: i32 = 0;
            for line in text.lines() {
                if is_hash_comment_line(line) {
                    continue;
                }
                let trimmed = line.trim_start();
                let fw = first_word(trimmed);
                let has_companion = companions.iter().any(|c| trimmed.contains(c));

                if fw.is_some_and(|w| openers.contains(&w)) && !has_companion {
                    balance += 1;
                }
                if fw == Some(closer) {
                    balance -= 1;
                }
            }
            balance <= 0
        }
    }
}

/// Sentinel rule returned when Ruby `do` (with optional block params) is
/// detected at the end of a line.
const RUBY_DO_RULE: BlockRule = BlockRule {
    keyword: "do",
    style: BlockStyle::End,
};

/// Find a matching block rule for the current line.
///
/// Returns the rule if the first word on the line matches a keyword for the
/// current language, and the companion keyword (Bash `then`/`do`/`in`) is
/// **not** already present on the line.  Also detects Ruby `do` / `do |…|`
/// at the end of a line regardless of what precedes it.
fn match_block_rule(lang_id: &str, line: &str) -> Option<&'static BlockRule> {
    let trimmed = line.trim_start();

    // --- Ruby `do` at end-of-line (including `do |args|`) ---
    if lang_id == "ruby" && ruby_line_ends_with_do(trimmed) {
        return Some(&RUBY_DO_RULE);
    }

    let first_word = trimmed
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()?;
    let rules = block_rules(lang_id);
    let rule = rules.iter().find(|r| r.keyword == first_word)?;

    // For Python, the line must end with `:`.
    if rule.style == BlockStyle::IndentOnly && !trimmed.trim_end().ends_with(':') {
        return None;
    }

    // For Bash, reject if the companion keyword is already on the line.
    match rule.style {
        BlockStyle::BashIf
            if trimmed.contains(" then")
                || trimmed.contains(";then")
                || trimmed.contains("; then") =>
        {
            return None;
        }
        BlockStyle::BashLoop
            if trimmed.contains(" do") || trimmed.contains(";do") || trimmed.contains("; do") =>
        {
            return None;
        }
        BlockStyle::BashCase if trimmed.contains(" in") => {
            return None;
        }
        _ => {}
    }

    Some(rule)
}

/// Check whether a trimmed Ruby line ends with `do` or `do |…|`.
///
/// Matches patterns like:
/// - `items.each do`
/// - `items.each do |item|`
/// - `5.times do |i, j|`
fn ruby_line_ends_with_do(trimmed: &str) -> bool {
    let s = trimmed.trim_end();

    // `do |…|` — strip the block-parameter list first.
    let s = if let Some(without_last) = s.strip_suffix('|') {
        // Find the matching opening `|`.
        if let Some(open_pipe) = without_last.rfind('|') {
            without_last[..open_pipe].trim_end()
        } else {
            return false;
        }
    } else {
        s
    };

    // Now `s` should end with `do` preceded by a non-word char or start-of-string.
    if let Some(before_do) = s.strip_suffix("do") {
        before_do.is_empty() || before_do.ends_with(|c: char| !c.is_alphanumeric() && c != '_')
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Feature 4: Auto-dedent tables
// ---------------------------------------------------------------------------

/// Return the set of closing keywords that should trigger auto-dedent for the
/// given language.
fn dedent_keywords(lang_id: &str) -> &'static [&'static str] {
    match lang_id {
        "ruby" => &["end"],
        "sh" => &["then", "fi", "do", "done", "in", "esac"],
        "python" | "python3" => &["elif", "else", "except", "finally"],
        // Brace languages: `}` is handled separately.
        _ => &[],
    }
}

/// Returns `true` if the language uses `}` as a block closer (and thus `}`
/// should trigger auto-dedent).
fn is_brace_language(lang_id: &str) -> bool {
    matches!(
        lang_id,
        "rust" | "c" | "cpp" | "javascript" | "java" | "css" | "sh" | "go" | "typescript" | "json"
    )
}

// ---------------------------------------------------------------------------
// Feature 3: HTML / XML helpers
// ---------------------------------------------------------------------------

/// Languages where HTML/XML tag handling applies.
fn is_markup_language(lang_id: &str) -> bool {
    matches!(lang_id, "html" | "xml" | "svg")
}

/// Extract the tag name from the text immediately before the cursor, assuming
/// it ends with `>`. Scans backwards for `<tagname ...>`. Returns `None` if
/// no valid opening tag is found.
fn opening_tag_before(text_before: &str) -> Option<&str> {
    let text = text_before.trim_end();
    if !text.ends_with('>') {
        return None;
    }
    // Find the matching '<'
    let open = text.rfind('<')?;
    let inner = &text[open + 1..text.len() - 1]; // between < and >
                                                 // Skip closing tags or self-closing tags.
    if inner.starts_with('/') || inner.ends_with('/') {
        return None;
    }
    // The tag name is everything up to the first whitespace.
    let tag_name = inner.split_whitespace().next()?;
    if tag_name.is_empty() || !tag_name.chars().next()?.is_alphabetic() {
        return None;
    }
    Some(tag_name)
}

/// Check whether the text after the cursor starts with `</tagname>`.
fn closing_tag_after(text_after: &str, expected_tag: &str) -> bool {
    let trimmed = text_after.trim_start();
    let expected = format!("</{expected_tag}>");
    trimmed.starts_with(&expected)
}

/// Walk backwards through the buffer to find the most recent unclosed opening
/// tag. Returns the tag name, or `None` if not found.
fn find_unclosed_tag(buffer: &sourceview5::Buffer, from: &gtk4::TextIter) -> Option<String> {
    let text = {
        let start = buffer.start_iter();
        buffer.text(&start, from, false).to_string()
    };

    // Simple stack-based approach: scan for <tag> and </tag> patterns.
    let mut stack: Vec<String> = Vec::new();
    let mut i = 0;
    let bytes = text.as_bytes();

    while i < bytes.len() {
        if bytes[i] == b'<' {
            // Find the end of this tag.
            if let Some(close_pos) = text[i..].find('>') {
                let tag_content = &text[i + 1..i + close_pos];

                if let Some(stripped) = tag_content.strip_prefix('/') {
                    // Closing tag: pop from stack if it matches.
                    let tag_name = stripped.split_whitespace().next().unwrap_or("");
                    if let Some(pos) = stack.iter().rposition(|t| t == tag_name) {
                        stack.truncate(pos);
                    }
                } else if !tag_content.ends_with('/')
                    && !tag_content.starts_with('!')
                    && !tag_content.starts_with('?')
                {
                    // Opening tag (not self-closing, not comment/doctype/PI).
                    let tag_name = tag_content
                        .split(|c: char| c.is_whitespace() || c == '/')
                        .next()
                        .unwrap_or("");
                    if !tag_name.is_empty()
                        && tag_name.chars().next().is_some_and(|c| c.is_alphabetic())
                        && !is_void_element(tag_name)
                    {
                        stack.push(tag_name.to_string());
                    }
                }

                i += close_pos + 1;
                continue;
            }
        }
        i += 1;
    }

    stack.pop()
}

/// HTML void elements that never have closing tags.
fn is_void_element(tag: &str) -> bool {
    matches!(
        tag.to_ascii_lowercase().as_str(),
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

// ---------------------------------------------------------------------------
// Feature 5: Smart comment continuation
// ---------------------------------------------------------------------------

/// Detected comment prefix on the current line.
#[derive(Debug, Clone)]
struct CommentPrefix {
    /// The full prefix to insert on the next line, including whitespace.
    /// E.g. `"    /// "` or `"  # "`.
    continuation: String,
    /// Whether the current line is "empty" (only the comment marker, no
    /// content after it). If true, we should exit the comment continuation
    /// by removing the prefix instead of continuing.
    is_empty: bool,
}

/// Try to detect a comment prefix on the current line.
///
/// Returns `None` if the cursor is not in a comment or the line does not
/// match any known comment pattern.
fn detect_comment_prefix(before_cursor: &str) -> Option<CommentPrefix> {
    // Order matters: check longer prefixes first.
    let patterns: &[&str] = &[
        "/// ", "///", "//! ", "//!", "// ", "//", "* ", "*", "# ", "#",
    ];

    let trimmed_start = before_cursor.len() - before_cursor.trim_start().len();
    let leading_ws = &before_cursor[..trimmed_start];
    let trimmed = before_cursor.trim_start();

    for &pat in patterns {
        if let Some(after_prefix) = trimmed.strip_prefix(pat) {
            let is_empty = after_prefix.trim().is_empty();
            // Normalize: ensure the continuation has a trailing space if the
            // pattern does not already end with one.
            let cont = if pat.ends_with(' ') {
                format!("{leading_ws}{pat}")
            } else {
                format!("{leading_ws}{pat} ")
            };
            return Some(CommentPrefix {
                continuation: cont,
                is_empty,
            });
        }
    }
    None
}

// ---------------------------------------------------------------------------
// BlockCompletion struct
// ---------------------------------------------------------------------------

/// Manages block completion, auto-dedent, comment continuation, and HTML
/// tag closing for a single editor buffer.
#[derive(Debug, Clone)]
pub struct BlockCompletion {
    /// The sourceview5 buffer.
    buffer: sourceview5::Buffer,
    /// The sourceview5 view.
    view: sourceview5::View,
    /// Re-entrancy guard.
    suppressing: Rc<Cell<bool>>,
}

impl BlockCompletion {
    /// Create and wire up block completion for the given view and buffer.
    ///
    /// The `EventControllerKey` is added in the Capture phase. It must be
    /// added *after* `BracketCompletion` so it has higher priority (for Enter
    /// handling — no conflict with bracket keys).
    pub fn new(view: &sourceview5::View, buffer: &sourceview5::Buffer) -> Self {
        let bc = Self {
            buffer: buffer.clone(),
            view: view.clone(),
            suppressing: Rc::new(Cell::new(false)),
        };
        bc.setup_key_controller();
        bc
    }

    /// Attach the key-press controller.
    fn setup_key_controller(&self) {
        let key_ctrl = gtk4::EventControllerKey::new();
        key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);

        let bc = self.clone();
        key_ctrl.connect_key_pressed(move |_ctrl, key, _code, mods| {
            if bc.suppressing.get() {
                return Propagation::Proceed;
            }

            // Ignore modifier combos other than bare Shift.
            let dominated = mods.difference(gtk4::gdk::ModifierType::SHIFT_MASK);
            if !dominated.is_empty() {
                return Propagation::Proceed;
            }

            // --- Enter key ---
            if key == gtk4::gdk::Key::Return || key == gtk4::gdk::Key::KP_Enter {
                return bc.handle_enter();
            }

            // --- Character keys (auto-dedent + HTML auto-close) ---
            if let Some(ch) = key.to_unicode() {
                return bc.handle_char(ch);
            }

            Propagation::Proceed
        });

        self.view.add_controller(key_ctrl);
    }

    // -----------------------------------------------------------------------
    // Enter handler
    // -----------------------------------------------------------------------

    /// Handle Enter key: try each feature in priority order.
    fn handle_enter(&self) -> Propagation {
        if self.buffer.has_selection() {
            return Propagation::Proceed;
        }

        let cursor = self.buffer.iter_at_mark(&self.buffer.get_insert());

        // Guard: do not act inside strings.
        if self.buffer.iter_has_context_class(&cursor, "string") {
            return Propagation::Proceed;
        }

        // Feature 5: Smart comment continuation (check BEFORE brace expansion
        // so that comments inside braces are handled correctly).
        if self.buffer.iter_has_context_class(&cursor, "comment") {
            if let Some(result) = self.try_comment_continuation(&cursor) {
                return result;
            }
        }

        // Feature 1: Brace expansion.
        if let Some(result) = self.try_brace_expansion(&cursor) {
            return result;
        }

        // Feature 3a: HTML/XML tag pair expansion.
        if let Some(result) = self.try_tag_expansion(&cursor) {
            return result;
        }

        // Feature 2: Keyword block completion.
        if let Some(result) = self.try_keyword_completion(&cursor) {
            return result;
        }

        Propagation::Proceed
    }

    // -----------------------------------------------------------------------
    // Feature 1: Brace expansion
    // -----------------------------------------------------------------------

    /// If the cursor is between `{` and `}`, expand to a multi-line block.
    fn try_brace_expansion(&self, cursor: &gtk4::TextIter) -> Option<Propagation> {
        let before = text_before_cursor(&self.buffer, cursor);
        let after = text_after_cursor(&self.buffer, cursor);

        // Check that the character before cursor (ignoring trailing spaces) is `{`.
        if before.trim_end().ends_with('{') && after.trim_start().starts_with('}') {
            let line = cursor.line();
            let indent = get_line_indent(&self.buffer, line);
            let tab = make_indent_unit(&self.view);

            // Delete any whitespace between { and }.
            let before_brace_offset = {
                let t = before.trim_end();
                // Position right after the `{`.
                let start = self.buffer.iter_at_line(line)?;
                start.offset() + t.len() as i32
            };
            let after_brace_offset = {
                let trimmed_len = after.len() - after.trim_start().len();
                cursor.offset() + trimmed_len as i32
            };

            self.suppressing.set(true);
            self.buffer.begin_user_action();

            // Delete whitespace between { and }.
            let mut del_start = self.buffer.iter_at_offset(before_brace_offset);
            let mut del_end = self.buffer.iter_at_offset(after_brace_offset);
            if del_start.offset() < del_end.offset() {
                self.buffer.delete(&mut del_start, &mut del_end);
            }

            // Insert the expansion at the position right after `{`.
            let insert_text = format!("\n{indent}{tab}\n{indent}");
            let mut insert_iter = self.buffer.iter_at_offset(before_brace_offset);
            self.buffer.insert(&mut insert_iter, &insert_text);

            // Place cursor at the end of the middle line.
            let cursor_offset = before_brace_offset + 1 + indent.len() as i32 + tab.len() as i32;
            let new_cursor = self.buffer.iter_at_offset(cursor_offset);
            self.buffer.place_cursor(&new_cursor);

            self.buffer.end_user_action();
            self.suppressing.set(false);

            return Some(Propagation::Stop);
        }
        None
    }

    // -----------------------------------------------------------------------
    // Feature 2: Keyword block completion
    // -----------------------------------------------------------------------

    /// Return `true` if the block opener at `line_num` already has a matching
    /// closer somewhere later in the buffer (or, for indent-only languages,
    /// already has an indented body below).
    fn block_already_closed(&self, style: BlockStyle, line_num: i32) -> bool {
        let start = self.buffer.start_iter();
        let end = self.buffer.end_iter();
        let text = self.buffer.text(&start, &end, false).to_string();
        let opener_idx = line_num.max(0) as usize;
        block_already_closed_in_text(style, &text, opener_idx)
    }

    /// If the cursor is at the end of a line starting with a block keyword,
    /// insert the closing construct.
    fn try_keyword_completion(&self, cursor: &gtk4::TextIter) -> Option<Propagation> {
        let lang = language_id(&self.buffer)?;
        let line_num = cursor.line();
        let line = line_text(&self.buffer, line_num);

        // Cursor must be at or near the end of the line.
        let after = text_after_cursor(&self.buffer, cursor);
        if !after.trim().is_empty() {
            return None;
        }

        let rule = match_block_rule(&lang, &line)?;

        if self.block_already_closed(rule.style, line_num) {
            return None;
        }

        let indent = get_line_indent(&self.buffer, line_num);
        let tab = make_indent_unit(&self.view);

        let insert = match rule.style {
            BlockStyle::End => {
                format!("\n{indent}{tab}\n{indent}end")
            }
            BlockStyle::BashIf => {
                format!("\n{indent}then\n{indent}{tab}\n{indent}fi")
            }
            BlockStyle::BashLoop => {
                format!("\n{indent}do\n{indent}{tab}\n{indent}done")
            }
            BlockStyle::BashCase => {
                format!(" in\n{indent}{tab}\n{indent}esac")
            }
            BlockStyle::IndentOnly => {
                format!("\n{indent}{tab}")
            }
        };

        // Cursor should land on the indented blank line.
        let cursor_line_offset = match rule.style {
            BlockStyle::End => indent.len() + tab.len() + 1, // after \n + indent + tab
            BlockStyle::BashIf => {
                // \n + indent + "then" + \n + indent + tab
                1 + indent.len() + "then".len() + 1 + indent.len() + tab.len()
            }
            BlockStyle::BashLoop => 1 + indent.len() + "do".len() + 1 + indent.len() + tab.len(),
            BlockStyle::BashCase => " in".len() + 1 + indent.len() + tab.len(),
            BlockStyle::IndentOnly => 1 + indent.len() + tab.len(),
        };

        self.suppressing.set(true);
        self.buffer.begin_user_action();

        // Insert at end of line.
        let mut end_of_line = *cursor;
        if !end_of_line.ends_line() {
            end_of_line.forward_to_line_end();
        }
        let insert_offset = end_of_line.offset();
        self.buffer.insert(&mut end_of_line, &insert);

        let new_cursor = self
            .buffer
            .iter_at_offset(insert_offset + cursor_line_offset as i32);
        self.buffer.place_cursor(&new_cursor);

        self.buffer.end_user_action();
        self.suppressing.set(false);

        Some(Propagation::Stop)
    }

    // -----------------------------------------------------------------------
    // Feature 3a: HTML/XML tag pair expansion
    // -----------------------------------------------------------------------

    /// If the cursor is between `<tag>` and `</tag>`, expand to multi-line.
    fn try_tag_expansion(&self, cursor: &gtk4::TextIter) -> Option<Propagation> {
        let lang = language_id(&self.buffer)?;
        if !is_markup_language(&lang) {
            return None;
        }

        let before = text_before_cursor(&self.buffer, cursor);
        let after = text_after_cursor(&self.buffer, cursor);

        let tag_name = opening_tag_before(&before)?;
        if !closing_tag_after(&after, tag_name) {
            return None;
        }

        let line = cursor.line();
        let indent = get_line_indent(&self.buffer, line);
        let tab = make_indent_unit(&self.view);

        let insert_text = format!("\n{indent}{tab}\n{indent}");

        self.suppressing.set(true);
        self.buffer.begin_user_action();

        let cursor_offset = cursor.offset();
        let mut insert_iter = self.buffer.iter_at_offset(cursor_offset);
        self.buffer.insert(&mut insert_iter, &insert_text);

        let new_cursor_offset = cursor_offset + 1 + indent.len() as i32 + tab.len() as i32;
        let new_cursor = self.buffer.iter_at_offset(new_cursor_offset);
        self.buffer.place_cursor(&new_cursor);

        self.buffer.end_user_action();
        self.suppressing.set(false);

        Some(Propagation::Stop)
    }

    // -----------------------------------------------------------------------
    // Feature 3b: HTML auto-close tag on `</`
    // -----------------------------------------------------------------------

    /// When the user types `/` immediately after `<`, auto-insert the closing
    /// tag name and `>`.
    fn try_html_auto_close(&self, cursor: &gtk4::TextIter) -> Option<Propagation> {
        let lang = language_id(&self.buffer)?;
        if !is_markup_language(&lang) {
            return None;
        }

        // Check that the character before cursor is `<`.
        let mut before_iter = *cursor;
        if !before_iter.backward_char() {
            return None;
        }
        if before_iter.char() != '<' {
            return None;
        }

        // Find the most recent unclosed opening tag.
        let tag_name = find_unclosed_tag(&self.buffer, &before_iter)?;

        self.suppressing.set(true);
        self.buffer.begin_user_action();

        // Insert `/ + tagname + >` at cursor.
        let insert_text = format!("/{tag_name}>");
        let cursor_offset = cursor.offset();
        let mut insert_iter = self.buffer.iter_at_offset(cursor_offset);
        self.buffer.insert(&mut insert_iter, &insert_text);

        // Place cursor after the inserted text.
        let new_cursor = self
            .buffer
            .iter_at_offset(cursor_offset + insert_text.len() as i32);
        self.buffer.place_cursor(&new_cursor);

        self.buffer.end_user_action();
        self.suppressing.set(false);

        Some(Propagation::Stop)
    }

    // -----------------------------------------------------------------------
    // Feature 4: Auto-dedent
    // -----------------------------------------------------------------------

    /// After a character is typed, check if the text from line-start to cursor
    /// matches a dedent keyword. If so, reduce indentation by one level.
    fn try_auto_dedent(&self) -> Option<Propagation> {
        let cursor = self.buffer.iter_at_mark(&self.buffer.get_insert());
        let line_num = cursor.line();
        let before = text_before_cursor(&self.buffer, &cursor);

        let word = before.trim();
        if word.is_empty() {
            return None;
        }

        let lang = language_id(&self.buffer);
        let lang_ref = lang.as_deref().unwrap_or("");

        let should_dedent = if word == "}" {
            is_brace_language(lang_ref)
        } else {
            dedent_keywords(lang_ref).contains(&word)
        };

        if !should_dedent {
            return None;
        }

        // Only dedent if there's nothing after the keyword on this line.
        let after = text_after_cursor(&self.buffer, &cursor);
        if !after.trim().is_empty() {
            return None;
        }

        let current_indent = get_line_indent(&self.buffer, line_num);
        let unit = make_indent_unit(&self.view);

        // Remove one indent level. If the indent doesn't contain a full unit,
        // there's nothing to dedent.
        if current_indent.len() < unit.len() {
            return None;
        }

        let new_indent = &current_indent[..current_indent.len() - unit.len()];
        let new_line_text = format!("{new_indent}{word}");

        self.suppressing.set(true);
        self.buffer.begin_user_action();

        // Replace the entire line content (up to cursor) with dedented version.
        let Some(line_start) = self.buffer.iter_at_line(line_num) else {
            self.buffer.end_user_action();
            self.suppressing.set(false);
            return None;
        };
        let mut cursor_end = self.buffer.iter_at_mark(&self.buffer.get_insert());
        let mut line_start_mut = line_start;
        self.buffer.delete(&mut line_start_mut, &mut cursor_end);

        let mut insert_iter = self.buffer.iter_at_line(line_num).unwrap_or(line_start_mut);
        self.buffer.insert(&mut insert_iter, &new_line_text);

        // Place cursor at end of the dedented keyword.
        let new_cursor_offset = insert_iter.offset();
        let new_cursor = self.buffer.iter_at_offset(new_cursor_offset);
        self.buffer.place_cursor(&new_cursor);

        self.buffer.end_user_action();
        self.suppressing.set(false);

        // We already let the character through (it's in the buffer). We just
        // rearranged whitespace. Return None so the caller returns Proceed.
        None
    }

    // -----------------------------------------------------------------------
    // Feature 5: Smart comment continuation
    // -----------------------------------------------------------------------

    /// If the cursor is inside a comment, continue the comment prefix on Enter.
    fn try_comment_continuation(&self, cursor: &gtk4::TextIter) -> Option<Propagation> {
        let before = text_before_cursor(&self.buffer, cursor);
        let prefix = detect_comment_prefix(&before)?;

        // If the comment line is empty (only the prefix), exit continuation:
        // remove the prefix and let the user start a non-comment line.
        if prefix.is_empty {
            let line_num = cursor.line();
            let indent = get_line_indent(&self.buffer, line_num);

            self.suppressing.set(true);
            self.buffer.begin_user_action();

            // Replace line content with just the indent + newline.
            let Some(line_start) = self.buffer.iter_at_line(line_num) else {
                self.buffer.end_user_action();
                self.suppressing.set(false);
                return None;
            };
            let mut end = *cursor;
            if !end.ends_line() {
                end.forward_to_line_end();
            }
            let mut line_start_mut = line_start;
            self.buffer.delete(&mut line_start_mut, &mut end);

            let mut insert_iter = self.buffer.iter_at_line(line_num).unwrap_or(line_start_mut);
            let replacement = format!("{indent}\n{indent}");
            self.buffer.insert(&mut insert_iter, &replacement);

            // Place cursor on the new blank line.
            let new_offset = self.buffer.iter_at_line(line_num).map_or(0, |it| it.offset())
                    + indent.len() as i32
                    + 1 // the newline
                    + indent.len() as i32;
            let new_cursor = self.buffer.iter_at_offset(new_offset);
            self.buffer.place_cursor(&new_cursor);

            self.buffer.end_user_action();
            self.suppressing.set(false);

            return Some(Propagation::Stop);
        }

        // Normal continuation: insert newline + comment prefix.
        self.suppressing.set(true);
        self.buffer.begin_user_action();

        let insert_text = format!("\n{}", prefix.continuation);
        let cursor_offset = cursor.offset();
        let mut insert_iter = self.buffer.iter_at_offset(cursor_offset);
        self.buffer.insert(&mut insert_iter, &insert_text);

        let new_cursor = self
            .buffer
            .iter_at_offset(cursor_offset + insert_text.len() as i32);
        self.buffer.place_cursor(&new_cursor);

        self.buffer.end_user_action();
        self.suppressing.set(false);

        Some(Propagation::Stop)
    }

    // -----------------------------------------------------------------------
    // Character handler
    // -----------------------------------------------------------------------

    /// Handle a typed character for auto-dedent and HTML auto-close.
    fn handle_char(&self, ch: char) -> Propagation {
        // Feature 3b: HTML auto-close on `/` after `<`.
        if ch == '/' {
            let cursor = self.buffer.iter_at_mark(&self.buffer.get_insert());
            if let Some(result) = self.try_html_auto_close(&cursor) {
                return result;
            }
        }

        // Feature 4: Auto-dedent is checked after the character is inserted,
        // but we cannot do that in the Capture phase (the character hasn't
        // been inserted yet). Instead, schedule a check after the key is
        // processed.
        if ch.is_alphabetic() || ch == '}' {
            let bc = self.clone();
            glib::idle_add_local_once(move || {
                if !bc.suppressing.get() {
                    bc.try_auto_dedent();
                }
            });
        }

        Propagation::Proceed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Feature 2: keyword matching --

    #[test]
    fn test_match_ruby_if() {
        let rule = match_block_rule("ruby", "  if condition");
        assert!(rule.is_some(), "should match Ruby `if`");
        assert_eq!(rule.unwrap().style, BlockStyle::End);
    }

    #[test]
    fn test_match_ruby_def() {
        let rule = match_block_rule("ruby", "def my_method");
        assert!(rule.is_some(), "should match Ruby `def`");
    }

    #[test]
    fn test_match_ruby_class() {
        let rule = match_block_rule("ruby", "    class MyClass < Base");
        assert!(rule.is_some(), "should match Ruby `class`");
    }

    #[test]
    fn test_no_match_ruby_mid_line() {
        // `puts` is not a block keyword.
        let rule = match_block_rule("ruby", "  puts 'hello'");
        assert!(rule.is_none(), "should not match non-keyword");
    }

    #[test]
    fn test_match_ruby_do_end_of_line() {
        let rule = match_block_rule("ruby", "  items.each do");
        assert!(rule.is_some(), "should match Ruby `do` at end of line");
        assert_eq!(rule.unwrap().style, BlockStyle::End);
    }

    #[test]
    fn test_match_ruby_do_with_block_params() {
        let rule = match_block_rule("ruby", "  items.each do |item|");
        assert!(rule.is_some(), "should match Ruby `do |item|`");
        assert_eq!(rule.unwrap().style, BlockStyle::End);
    }

    #[test]
    fn test_match_ruby_do_multiple_params() {
        let rule = match_block_rule("ruby", "  hash.each do |key, value|");
        assert!(rule.is_some(), "should match Ruby `do |key, value|`");
    }

    #[test]
    fn test_match_ruby_bare_do() {
        // `do` alone on a line (e.g. after `loop`)
        let rule = match_block_rule("ruby", "  do");
        assert!(rule.is_some(), "should match bare `do`");
    }

    #[test]
    fn test_no_match_ruby_redo() {
        // `redo` ends with `do` but is not the `do` keyword.
        let rule = match_block_rule("ruby", "  redo");
        assert!(rule.is_none(), "should not match `redo`");
    }

    #[test]
    fn test_match_bash_if() {
        let rule = match_block_rule("sh", "if [ -f file ]");
        assert!(rule.is_some());
        assert_eq!(rule.unwrap().style, BlockStyle::BashIf);
    }

    #[test]
    fn test_bash_if_with_then_already() {
        let rule = match_block_rule("sh", "if [ -f file ]; then");
        assert!(rule.is_none(), "should not match if `then` already present");
    }

    #[test]
    fn test_match_bash_for() {
        let rule = match_block_rule("sh", "for i in 1 2 3");
        assert!(rule.is_some());
        assert_eq!(rule.unwrap().style, BlockStyle::BashLoop);
    }

    #[test]
    fn test_bash_for_with_do_already() {
        let rule = match_block_rule("sh", "for i in 1 2 3; do");
        assert!(rule.is_none(), "should not match if `do` already present");
    }

    #[test]
    fn test_match_bash_case() {
        let rule = match_block_rule("sh", "case $var");
        assert!(rule.is_some());
        assert_eq!(rule.unwrap().style, BlockStyle::BashCase);
    }

    #[test]
    fn test_bash_case_with_in_already() {
        let rule = match_block_rule("sh", "case $var in");
        assert!(rule.is_none(), "should not match if `in` already present");
    }

    #[test]
    fn test_match_python_def() {
        let rule = match_block_rule("python", "    def foo():");
        assert!(rule.is_some());
        assert_eq!(rule.unwrap().style, BlockStyle::IndentOnly);
    }

    #[test]
    fn test_python_no_colon() {
        let rule = match_block_rule("python", "    def foo()");
        assert!(rule.is_none(), "Python block must end with `:`");
    }

    #[test]
    fn test_match_python_if() {
        let rule = match_block_rule("python", "if x > 0:");
        assert!(rule.is_some());
    }

    #[test]
    fn test_no_match_unknown_language() {
        let rule = match_block_rule("json", "if something");
        assert!(rule.is_none());
    }

    // -- block_already_closed_in_text --

    #[test]
    fn test_ruby_already_closed_simple() {
        let text = "if foo\n  body\nend\n";
        assert!(block_already_closed_in_text(BlockStyle::End, text, 0));
    }

    #[test]
    fn test_ruby_not_closed_alone() {
        let text = "if foo\n";
        assert!(!block_already_closed_in_text(BlockStyle::End, text, 0));
    }

    #[test]
    fn test_ruby_nested_outer_not_closed() {
        // Outer if has only one `end` for the inner one — outer remains open.
        let text = "if outer\n  if inner\n    body\n  end\n";
        assert!(!block_already_closed_in_text(BlockStyle::End, text, 0));
    }

    #[test]
    fn test_ruby_nested_outer_closed() {
        let text = "if outer\n  if inner\n    body\n  end\nend\n";
        assert!(block_already_closed_in_text(BlockStyle::End, text, 0));
    }

    #[test]
    fn test_ruby_do_block_already_closed() {
        let text = "items.each do |item|\n  puts item\nend\n";
        assert!(block_already_closed_in_text(BlockStyle::End, text, 0));
    }

    #[test]
    fn test_ruby_postfix_modifier_not_opener() {
        // `puts x if y` is a postfix modifier, must not count as an opener.
        let text = "if foo\n  puts x if y\n";
        assert!(!block_already_closed_in_text(BlockStyle::End, text, 0));
    }

    #[test]
    fn test_ruby_comment_end_ignored() {
        // `end` inside a comment line should not be counted.
        let text = "if foo\n  body\n# end of section\n";
        assert!(!block_already_closed_in_text(BlockStyle::End, text, 0));
    }

    #[test]
    fn test_ruby_new_def_inside_balanced_class() {
        // User typed a fresh `def blah` inside an existing balanced class.
        // Total file: openers (class, def existing, def blah) = 3,
        // closers (end, end) = 2 → openers > closers, must add.
        let text = "class Foo\n  def existing\n    body\n  end\n  def blah\nend\n";
        let opener_line = 4; // "  def blah"
        assert!(!block_already_closed_in_text(
            BlockStyle::End,
            text,
            opener_line
        ));
    }

    #[test]
    fn test_ruby_new_def_at_top_of_balanced_file() {
        // Adding `def blah` above an already-balanced class; total openers (4)
        // exceeds closers (2) → must add.
        let text = "def blah\nclass Foo\n  def existing\n    body\n  end\nend\n";
        assert!(!block_already_closed_in_text(BlockStyle::End, text, 0));
    }

    #[test]
    fn test_bash_if_already_closed() {
        let text = "if [ -f x ]\n  echo hi\nfi\n";
        assert!(block_already_closed_in_text(BlockStyle::BashIf, text, 0));
    }

    #[test]
    fn test_bash_if_inline_pair_balanced() {
        // Outer `if foo` not closed; inner one-line `if x; then …; fi` is
        // balanced (companion present → not counted as opener).
        let text = "if foo\n  if x; then echo; fi\n  body\n";
        assert!(!block_already_closed_in_text(BlockStyle::BashIf, text, 0));
    }

    #[test]
    fn test_bash_loop_already_closed() {
        let text = "for i in 1 2 3\n  echo $i\ndone\n";
        assert!(block_already_closed_in_text(BlockStyle::BashLoop, text, 0));
    }

    #[test]
    fn test_bash_case_already_closed() {
        let text = "case $x\n  a) ;;\nesac\n";
        assert!(block_already_closed_in_text(BlockStyle::BashCase, text, 0));
    }

    #[test]
    fn test_bash_new_if_inside_balanced_file() {
        // Existing `if/fi` pair plus a freshly typed `if other` — total
        // openers (2) > closers (1), must add.
        let text = "if old\n  echo a\nfi\nif other\n";
        assert!(!block_already_closed_in_text(BlockStyle::BashIf, text, 3));
    }

    #[test]
    fn test_python_indent_only_body_present() {
        let text = "if foo:\n    body()\n";
        assert!(block_already_closed_in_text(
            BlockStyle::IndentOnly,
            text,
            0
        ));
    }

    #[test]
    fn test_python_indent_only_no_body() {
        let text = "if foo:\n";
        assert!(!block_already_closed_in_text(
            BlockStyle::IndentOnly,
            text,
            0
        ));
    }

    #[test]
    fn test_python_indent_only_next_line_same_indent() {
        let text = "if foo:\nbar()\n";
        assert!(!block_already_closed_in_text(
            BlockStyle::IndentOnly,
            text,
            0
        ));
    }

    #[test]
    fn test_python_indent_only_skips_blank_and_comment() {
        let text = "if foo:\n\n# note\n    body()\n";
        assert!(block_already_closed_in_text(
            BlockStyle::IndentOnly,
            text,
            0
        ));
    }

    #[test]
    fn test_python_indent_only_indented_opener() {
        // Opener at indent 4; existing body at indent 8 → already has body.
        let text = "    if foo:\n        body()\n";
        assert!(block_already_closed_in_text(
            BlockStyle::IndentOnly,
            text,
            0
        ));
    }

    #[test]
    fn test_python_indent_only_with_preceding_code() {
        // Opener is at line 2; preceding lines must not affect the decision.
        let text = "import os\n\nif foo:\n    body()\n";
        assert!(block_already_closed_in_text(
            BlockStyle::IndentOnly,
            text,
            2
        ));
    }

    #[test]
    fn test_first_word_basic() {
        assert_eq!(first_word("  if foo"), Some("if"));
        assert_eq!(first_word("def my_method()"), Some("def"));
        assert_eq!(first_word(""), None);
        assert_eq!(first_word("   "), None);
    }

    // -- Feature 3: HTML tag helpers --

    #[test]
    fn test_opening_tag_before_simple() {
        assert_eq!(opening_tag_before("<div>"), Some("div"));
    }

    #[test]
    fn test_opening_tag_before_with_attrs() {
        assert_eq!(opening_tag_before("<div class=\"foo\">"), Some("div"));
    }

    #[test]
    fn test_opening_tag_before_closing_tag() {
        assert_eq!(opening_tag_before("</div>"), None);
    }

    #[test]
    fn test_opening_tag_before_self_closing() {
        assert_eq!(opening_tag_before("<br/>"), None);
    }

    #[test]
    fn test_opening_tag_before_no_tag() {
        assert_eq!(opening_tag_before("hello world"), None);
    }

    #[test]
    fn test_closing_tag_after_match() {
        assert!(closing_tag_after("</div>", "div"));
    }

    #[test]
    fn test_closing_tag_after_no_match() {
        assert!(!closing_tag_after("</span>", "div"));
    }

    #[test]
    fn test_closing_tag_after_with_space() {
        assert!(closing_tag_after("  </div>", "div"));
    }

    #[test]
    fn test_is_void_element_true() {
        assert!(is_void_element("br"));
        assert!(is_void_element("img"));
        assert!(is_void_element("input"));
    }

    #[test]
    fn test_is_void_element_false() {
        assert!(!is_void_element("div"));
        assert!(!is_void_element("span"));
    }

    // -- Feature 4: dedent keywords --

    #[test]
    fn test_dedent_keywords_ruby() {
        let kw = dedent_keywords("ruby");
        assert!(kw.contains(&"end"));
    }

    #[test]
    fn test_dedent_keywords_bash() {
        let kw = dedent_keywords("sh");
        assert!(kw.contains(&"fi"));
        assert!(kw.contains(&"done"));
        assert!(kw.contains(&"esac"));
    }

    #[test]
    fn test_dedent_keywords_python() {
        let kw = dedent_keywords("python");
        assert!(kw.contains(&"else"));
        assert!(kw.contains(&"elif"));
    }

    #[test]
    fn test_is_brace_language() {
        assert!(is_brace_language("rust"));
        assert!(is_brace_language("javascript"));
        assert!(is_brace_language("c"));
        assert!(!is_brace_language("ruby"));
        assert!(!is_brace_language("python"));
    }

    // -- Feature 5: comment prefix detection --

    #[test]
    fn test_detect_rust_doc_comment() {
        let p = detect_comment_prefix("    /// some docs");
        assert!(p.is_some());
        let p = p.unwrap();
        assert_eq!(p.continuation, "    /// ");
        assert!(!p.is_empty);
    }

    #[test]
    fn test_detect_rust_doc_comment_empty() {
        let p = detect_comment_prefix("    /// ");
        assert!(p.is_some());
        let p = p.unwrap();
        assert!(p.is_empty);
    }

    #[test]
    fn test_detect_rust_module_doc() {
        let p = detect_comment_prefix("//! module doc");
        assert!(p.is_some());
        let p = p.unwrap();
        assert_eq!(p.continuation, "//! ");
        assert!(!p.is_empty);
    }

    #[test]
    fn test_detect_line_comment() {
        let p = detect_comment_prefix("  // a comment");
        assert!(p.is_some());
        let p = p.unwrap();
        assert_eq!(p.continuation, "  // ");
        assert!(!p.is_empty);
    }

    #[test]
    fn test_detect_hash_comment() {
        let p = detect_comment_prefix("  # a comment");
        assert!(p.is_some());
        let p = p.unwrap();
        assert_eq!(p.continuation, "  # ");
    }

    #[test]
    fn test_detect_block_comment_star() {
        let p = detect_comment_prefix("   * some text");
        assert!(p.is_some());
        let p = p.unwrap();
        assert_eq!(p.continuation, "   * ");
    }

    #[test]
    fn test_detect_no_comment() {
        let p = detect_comment_prefix("    let x = 42;");
        assert!(p.is_none());
    }

    #[test]
    fn test_detect_hash_comment_empty() {
        let p = detect_comment_prefix("  # ");
        assert!(p.is_some());
        assert!(p.unwrap().is_empty);
    }

    // -- Shared helpers --

    #[test]
    fn test_is_markup_language() {
        assert!(is_markup_language("html"));
        assert!(is_markup_language("xml"));
        assert!(!is_markup_language("rust"));
    }
}
