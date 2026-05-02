//! Manual format and format-on-save flows for editor tabs.
//!
//! Both flows share the same hot path: capture buffer text + cursor → spawn
//! the formatter on a worker thread → atomically replace buffer text under
//! `begin_user_action` / `end_user_action`. The format-on-save variant
//! additionally calls back into the tab's save once the format result lands.

use std::cell::Cell;
use std::rc::Rc;

use gtk4::prelude::*;

use rline_lint::{LintError, LintRegistry};

use crate::editor::EditorTab;
use crate::lint::lint_worker;

thread_local! {
    /// Monotonic id used to single-flight format requests per tab. A new
    /// request increments the counter; results carrying an older id are
    /// dropped.
    static REQUEST_COUNTER: Cell<u64> = const { Cell::new(0) };
}

fn next_request_id() -> u64 {
    REQUEST_COUNTER.with(|c| {
        let next = c.get() + 1;
        c.set(next);
        next
    })
}

/// Reason the format flow ended without applying a formatted buffer.
#[derive(Debug)]
pub enum FormatOutcome {
    /// The buffer was successfully reformatted.
    Reformatted,
    /// Formatting completed but produced text identical to the input.
    NoChange,
    /// No formatter is configured for this buffer's language.
    NoFormatter,
    /// The buffer has no associated file path, so we can't pick a formatter.
    NoPath,
    /// The formatter failed; the buffer is untouched.
    Failed(LintError),
    /// A newer format request superseded this one before it completed.
    Superseded,
}

/// Run the format flow on `tab`. When `on_done` is supplied, it is invoked
/// on the GTK main thread once the result lands (regardless of outcome).
pub fn format_buffer<F>(tab: &EditorTab, registry: &LintRegistry, on_done: F)
where
    F: FnOnce(FormatOutcome) + 'static,
{
    let path = match tab.file_path() {
        Some(p) => p,
        None => {
            on_done(FormatOutcome::NoPath);
            return;
        }
    };

    let language = match rline_syntax::language_for_extension(
        path.extension().and_then(|e| e.to_str()).unwrap_or(""),
    ) {
        Some(lang) => lang,
        None => {
            on_done(FormatOutcome::NoFormatter);
            return;
        }
    };

    let entry = registry.entry(language);
    let formatter = match entry.formatter {
        Some(f) => f,
        None => {
            on_done(FormatOutcome::NoFormatter);
            return;
        }
    };

    let buffer = tab.buffer().clone();
    let (start, end) = buffer.bounds();
    let source = buffer.text(&start, &end, true).to_string();

    // Capture cursor position by line/column so we can do best-effort
    // restoration after replace. Line-based mapping survives reformatting
    // better than byte offsets.
    let cursor_iter = buffer.iter_at_mark(&buffer.get_insert());
    let cursor_line = cursor_iter.line();
    let cursor_offset = cursor_iter.line_offset();

    let request_id = next_request_id();
    let on_done = Rc::new(std::cell::RefCell::new(Some(on_done)));

    lint_worker::spawn_format(
        formatter,
        source.clone(),
        path,
        request_id,
        move |id, result| {
            // Pull out the callback once.
            let cb = on_done.borrow_mut().take();

            // Single-flight: a newer request has been issued. Drop quietly.
            let current = REQUEST_COUNTER.with(|c| c.get());
            if id != current {
                if let Some(cb) = cb {
                    cb(FormatOutcome::Superseded);
                }
                return;
            }

            match result {
                Ok(formatted) => {
                    if formatted == source {
                        if let Some(cb) = cb {
                            cb(FormatOutcome::NoChange);
                        }
                        return;
                    }
                    apply_formatted_text(&buffer, &formatted, cursor_line, cursor_offset);
                    if let Some(cb) = cb {
                        cb(FormatOutcome::Reformatted);
                    }
                }
                Err(e) => {
                    tracing::warn!("format failed: {e}");
                    if let Some(cb) = cb {
                        cb(FormatOutcome::Failed(e));
                    }
                }
            }
        },
    );
}

/// Save `tab`. If `format_on_save` is enabled for this buffer's language and
/// a formatter is registered, format first and then save; otherwise save
/// directly. The save itself never blocks on the format — if the formatter
/// fails or no formatter is configured, the buffer is saved as-is.
pub fn save_with_optional_format(
    tab: &EditorTab,
    registry: &LintRegistry,
    settings: &rline_config::EditorSettings,
) {
    let path = match tab.file_path() {
        Some(p) => p,
        None => {
            do_save(tab, "<unsaved>");
            return;
        }
    };

    let language_id = lint_worker::language_id_for_path(&path);
    let should_format = match language_id {
        Some(id) => settings.lint.should_format_on_save(id),
        None => false,
    };

    if !should_format {
        do_save(tab, &path.display().to_string());
        return;
    }

    let language = rline_syntax::language_for_extension(
        path.extension().and_then(|e| e.to_str()).unwrap_or(""),
    );
    let formatter = language
        .map(|l| registry.entry(l))
        .and_then(|e| e.formatter);
    if formatter.is_none() {
        do_save(tab, &path.display().to_string());
        return;
    }

    let tab_for_cb = tab.clone();
    let path_str = path.display().to_string();
    format_buffer(tab, registry, move |outcome| {
        match outcome {
            FormatOutcome::Reformatted | FormatOutcome::NoChange => {
                do_save(&tab_for_cb, &path_str);
            }
            FormatOutcome::Failed(e) => {
                tracing::warn!("format-on-save failed for {path_str}: {e} — saving unformatted");
                do_save(&tab_for_cb, &path_str);
            }
            FormatOutcome::NoFormatter | FormatOutcome::NoPath => {
                do_save(&tab_for_cb, &path_str);
            }
            FormatOutcome::Superseded => {
                // A newer format request is in flight. The eventual save it
                // triggers will write the latest formatted text.
            }
        }
    });
}

fn do_save(tab: &EditorTab, label: &str) {
    if let Err(e) = tab.save() {
        tracing::error!("save failed for {label}: {e}");
    }
}

/// Replace the entire buffer with `formatted` in a single undo step, then
/// best-effort restore the cursor to the same line/column it occupied before.
fn apply_formatted_text(
    buffer: &sourceview5::Buffer,
    formatted: &str,
    cursor_line: i32,
    cursor_offset: i32,
) {
    buffer.begin_user_action();
    let (mut start, mut end) = buffer.bounds();
    buffer.delete(&mut start, &mut end);
    buffer.insert(&mut start, formatted);

    let line_count = buffer.line_count();
    let target_line = cursor_line.min(line_count.saturating_sub(1).max(0));
    if let Some(mut iter) = buffer.iter_at_line(target_line) {
        let line_chars = {
            let mut e = iter;
            if !e.ends_line() {
                e.forward_to_line_end();
            }
            e.line_offset()
        };
        let target_offset = cursor_offset.min(line_chars);
        // iter is currently at column 0 of target_line; move forward.
        for _ in 0..target_offset {
            if !iter.forward_char() {
                break;
            }
        }
        buffer.place_cursor(&iter);
    }

    buffer.end_user_action();
    buffer.set_modified(true);
}
