//! Automatic bracket and quote pair completion.
//!
//! Inserts matching closing characters when the user types an opening
//! bracket or quote. Also handles skip-over of closing characters,
//! backspace deletion of empty pairs, and wrapping selected text.

use std::cell::Cell;
use std::rc::Rc;

use glib::Propagation;
use gtk4::prelude::*;

/// Character pairs: (opener, closer).
const PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')];

/// Returns the closing character for a given opener, if any.
fn closer_for(ch: char) -> Option<char> {
    PAIRS.iter().find(|(o, _)| *o == ch).map(|(_, c)| *c)
}

/// Returns true if `ch` is an opening bracket or quote.
fn is_opener(ch: char) -> bool {
    PAIRS.iter().any(|(o, _)| *o == ch)
}

/// Returns true if `ch` is a closing bracket (not a quote, since quotes
/// are both openers and closers).
fn is_close_bracket(ch: char) -> bool {
    matches!(ch, ')' | ']' | '}')
}

/// Returns true if `ch` is a quote character that acts as both opener and
/// closer.
fn is_quote(ch: char) -> bool {
    matches!(ch, '"' | '\'')
}

/// Returns true if the cursor sits between a matched empty pair, e.g. `(|)`.
fn is_between_pair(buffer: &sourceview5::Buffer) -> bool {
    let cursor = buffer.iter_at_mark(&buffer.get_insert());

    let mut before = cursor;
    if !before.backward_char() {
        return false;
    }
    let before_ch = before.char();

    let after_ch = cursor.char();

    closer_for(before_ch) == Some(after_ch)
}

/// Heuristic: should we auto-close this quote character?
///
/// Returns `true` when the character before the cursor is whitespace,
/// punctuation, an opening bracket, or the start of the buffer — i.e. the
/// user is likely starting a new string literal rather than typing a
/// contraction like `don't`.
fn should_auto_close_quote(buffer: &sourceview5::Buffer) -> bool {
    let cursor = buffer.iter_at_mark(&buffer.get_insert());

    // Character before cursor (start-of-buffer counts as "ok").
    let before_ok = {
        let mut prev = cursor;
        if prev.backward_char() {
            let ch = prev.char();
            ch.is_whitespace() || ch.is_ascii_punctuation() || is_opener(ch)
        } else {
            true
        }
    };

    // Character after cursor (end-of-buffer counts as "ok").
    let after_ok = if cursor.is_end() {
        true
    } else {
        let ch = cursor.char();
        ch.is_whitespace() || ch.is_ascii_punctuation() || is_close_bracket(ch)
    };

    before_ok && after_ok
}

/// Manages automatic bracket/quote completion for a single editor buffer.
#[derive(Debug, Clone)]
pub struct BracketCompletion {
    /// The sourceview5 buffer we operate on.
    buffer: sourceview5::Buffer,
    /// The sourceview5 view we attach the key controller to.
    view: sourceview5::View,
    /// Re-entrancy guard — set while we programmatically insert text so our
    /// own handler does not react to those insertions.
    suppressing: Rc<Cell<bool>>,
}

impl BracketCompletion {
    /// Create and wire up bracket completion for the given view and buffer.
    ///
    /// The `EventControllerKey` is added in the Capture phase so it runs
    /// before the default sourceview key handling. It must be added *after*
    /// any `InlineCompletion` controller so that ghost-text dismissal takes
    /// priority.
    pub fn new(view: &sourceview5::View, buffer: &sourceview5::Buffer) -> Self {
        let bc = Self {
            buffer: buffer.clone(),
            view: view.clone(),
            suppressing: Rc::new(Cell::new(false)),
        };

        bc.setup_key_controller();
        bc
    }

    /// Attach the key-press controller to the view.
    fn setup_key_controller(&self) {
        let key_ctrl = gtk4::EventControllerKey::new();
        key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);

        let bc = self.clone();
        key_ctrl.connect_key_pressed(move |_ctrl, key, _code, mods| {
            if bc.suppressing.get() {
                return Propagation::Proceed;
            }

            // Ignore modifier combos other than bare Shift (needed for `{`, `"`, etc.).
            let dominated = mods.difference(gtk4::gdk::ModifierType::SHIFT_MASK);
            if !dominated.is_empty() {
                return Propagation::Proceed;
            }

            // Handle Backspace between an empty pair.
            if key == gtk4::gdk::Key::BackSpace {
                return bc.handle_backspace();
            }

            let Some(ch) = key.to_unicode() else {
                return Propagation::Proceed;
            };

            let has_selection = bc.buffer.has_selection();

            // Selection wrapping: opener with text selected wraps the selection.
            if has_selection && is_opener(ch) {
                return bc.wrap_selection(ch);
            }

            // Skip-over: typing a closing bracket that already exists after cursor.
            if is_close_bracket(ch) {
                let cursor = bc.buffer.iter_at_mark(&bc.buffer.get_insert());
                if !cursor.is_end() && cursor.char() == ch {
                    let mut next = cursor;
                    next.forward_char();
                    bc.buffer.place_cursor(&next);
                    return Propagation::Stop;
                }
            }

            // Quote typed when the same quote is after cursor → skip over.
            if is_quote(ch) && !has_selection {
                let cursor = bc.buffer.iter_at_mark(&bc.buffer.get_insert());
                if !cursor.is_end() && cursor.char() == ch {
                    let mut next = cursor;
                    next.forward_char();
                    bc.buffer.place_cursor(&next);
                    return Propagation::Stop;
                }
            }

            // Auto-close: insert opener + closer, cursor between them.
            if let Some(closer) = closer_for(ch) {
                // Quote heuristic — skip auto-close mid-word.
                if is_quote(ch) && !should_auto_close_quote(&bc.buffer) {
                    return Propagation::Proceed;
                }

                bc.suppressing.set(true);

                let mut cursor = bc.buffer.iter_at_mark(&bc.buffer.get_insert());
                let offset = cursor.offset();

                bc.buffer.begin_user_action();
                let pair = format!("{ch}{closer}");
                bc.buffer.insert(&mut cursor, &pair);
                // Place cursor between the pair.
                let between = bc.buffer.iter_at_offset(offset + 1);
                bc.buffer.place_cursor(&between);
                bc.buffer.end_user_action();

                bc.suppressing.set(false);
                return Propagation::Stop;
            }

            Propagation::Proceed
        });

        self.view.add_controller(key_ctrl);
    }

    /// If the cursor is between a matched empty pair, delete both characters.
    fn handle_backspace(&self) -> Propagation {
        if self.buffer.has_selection() {
            return Propagation::Proceed;
        }

        if is_between_pair(&self.buffer) {
            self.suppressing.set(true);

            let cursor = self.buffer.iter_at_mark(&self.buffer.get_insert());
            let mut before = cursor;
            before.backward_char();
            let mut after = cursor;
            after.forward_char();

            self.buffer.begin_user_action();
            self.buffer.delete(&mut before, &mut after);
            self.buffer.end_user_action();

            self.suppressing.set(false);
            return Propagation::Stop;
        }

        Propagation::Proceed
    }

    /// Wrap the current selection with opener and closer, preserving the
    /// selection on the wrapped content.
    fn wrap_selection(&self, opener: char) -> Propagation {
        let Some(closer) = closer_for(opener) else {
            return Propagation::Proceed;
        };

        let Some((start, end)) = self.buffer.selection_bounds() else {
            return Propagation::Proceed;
        };

        let start_offset = start.offset();
        let end_offset = end.offset();

        self.suppressing.set(true);
        self.buffer.begin_user_action();

        // Insert closer at end first so start offset stays valid.
        let mut end_iter = self.buffer.iter_at_offset(end_offset);
        self.buffer.insert(&mut end_iter, &closer.to_string());

        // Insert opener at start.
        let mut start_iter = self.buffer.iter_at_offset(start_offset);
        self.buffer.insert(&mut start_iter, &opener.to_string());

        // Re-select the wrapped content (between opener and closer).
        let new_start = self.buffer.iter_at_offset(start_offset + 1);
        let new_end = self.buffer.iter_at_offset(end_offset + 1);
        self.buffer.select_range(&new_start, &new_end);

        self.buffer.end_user_action();
        self.suppressing.set(false);

        Propagation::Stop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_closer_for_parenthesis() {
        assert_eq!(closer_for('('), Some(')'));
    }

    #[test]
    fn test_closer_for_bracket() {
        assert_eq!(closer_for('['), Some(']'));
    }

    #[test]
    fn test_closer_for_brace() {
        assert_eq!(closer_for('{'), Some('}'));
    }

    #[test]
    fn test_closer_for_double_quote() {
        assert_eq!(closer_for('"'), Some('"'));
    }

    #[test]
    fn test_closer_for_single_quote() {
        assert_eq!(closer_for('\''), Some('\''));
    }

    #[test]
    fn test_closer_for_non_pair_returns_none() {
        assert_eq!(closer_for('a'), None);
        assert_eq!(closer_for('1'), None);
        assert_eq!(closer_for(' '), None);
    }

    #[test]
    fn test_is_opener_true() {
        for ch in ['(', '[', '{', '"', '\''] {
            assert!(is_opener(ch), "{ch:?} should be an opener");
        }
    }

    #[test]
    fn test_is_opener_false() {
        for ch in [')', ']', '}', 'a', '1', ' '] {
            assert!(!is_opener(ch), "{ch:?} should not be an opener");
        }
    }

    #[test]
    fn test_is_close_bracket_true() {
        for ch in [')', ']', '}'] {
            assert!(is_close_bracket(ch), "{ch:?} should be a close bracket");
        }
    }

    #[test]
    fn test_is_close_bracket_false() {
        for ch in ['(', '[', '{', '"', '\'', 'a'] {
            assert!(
                !is_close_bracket(ch),
                "{ch:?} should not be a close bracket"
            );
        }
    }

    #[test]
    fn test_is_quote_true() {
        assert!(is_quote('"'));
        assert!(is_quote('\''));
    }

    #[test]
    fn test_is_quote_false() {
        assert!(!is_quote('('));
        assert!(!is_quote(')'));
        assert!(!is_quote('`'));
    }
}
