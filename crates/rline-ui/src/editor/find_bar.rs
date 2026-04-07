//! FindBar — compact overlay find/replace bar for an editor tab.
//!
//! Uses `sourceview5::SearchContext` and `sourceview5::SearchSettings` to
//! highlight matches and navigate between them.  Floats in the top-right
//! corner of the editor view as a `gtk4::Overlay` child.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use sourceview5::prelude::*;

// ── Public widget ──────────────────────────────────────────────────

/// A compact, overlay find/replace bar for a single editor tab.
#[derive(Clone)]
pub struct FindBar {
    /// Outer frame that positions top-right inside an Overlay.
    container: gtk4::Box,
    find_entry: gtk4::Entry,
    replace_entry: gtk4::Entry,
    replace_row: gtk4::Box,
    match_label: gtk4::Label,
    context: Rc<RefCell<sourceview5::SearchContext>>,
    view: sourceview5::View,
}

impl std::fmt::Debug for FindBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FindBar").finish_non_exhaustive()
    }
}

impl FindBar {
    /// Create a new find bar bound to the given buffer and view (initially hidden).
    pub fn new(buffer: &sourceview5::Buffer, view: &sourceview5::View) -> Self {
        let settings = sourceview5::SearchSettings::new();
        settings.set_wrap_around(true);

        let context = sourceview5::SearchContext::new(buffer, Some(&settings));
        context.set_highlight(false);

        // ── Find row ───────────────────────────────────────────
        let find_entry = gtk4::Entry::builder()
            .placeholder_text("Find")
            .width_chars(20)
            .build();

        let prev_btn = gtk4::Button::from_icon_name("go-up-symbolic");
        prev_btn.add_css_class("flat");
        prev_btn.set_tooltip_text(Some("Previous match"));
        let next_btn = gtk4::Button::from_icon_name("go-down-symbolic");
        next_btn.add_css_class("flat");
        next_btn.set_tooltip_text(Some("Next match"));

        let match_label = gtk4::Label::new(None);
        match_label.add_css_class("dim-label");
        match_label.set_width_chars(8);

        let close_btn = gtk4::Button::from_icon_name("window-close-symbolic");
        close_btn.set_tooltip_text(Some("Close (Escape)"));
        close_btn.add_css_class("flat");

        let find_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
        find_row.append(&find_entry);
        find_row.append(&match_label);
        find_row.append(&prev_btn);
        find_row.append(&next_btn);
        find_row.append(&close_btn);

        // ── Replace row ────────────────────────────────────────
        let replace_entry = gtk4::Entry::builder()
            .placeholder_text("Replace")
            .width_chars(20)
            .build();

        let replace_btn = gtk4::Button::from_icon_name("edit-find-replace-symbolic");
        replace_btn.add_css_class("flat");
        replace_btn.set_tooltip_text(Some("Replace next (Enter)"));
        let replace_all_btn = gtk4::Button::with_label("All");
        replace_all_btn.add_css_class("flat");
        replace_all_btn.set_tooltip_text(Some("Replace all (Ctrl+Enter)"));

        let replace_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
        replace_row.append(&replace_entry);
        replace_row.append(&replace_btn);
        replace_row.append(&replace_all_btn);

        // ── Container — compact, overlay-friendly ──────────────
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.append(&find_row);
        container.append(&replace_row);
        container.set_halign(gtk4::Align::End);
        container.set_valign(gtk4::Align::Start);
        container.set_margin_end(20); // clear the scrollbar
        container.set_margin_top(2);
        container.add_css_class("find-bar");
        container.add_css_class("background");
        container.set_visible(false);

        let context = Rc::new(RefCell::new(context));

        let bar = Self {
            container,
            find_entry: find_entry.clone(),
            replace_entry: replace_entry.clone(),
            replace_row,
            match_label: match_label.clone(),
            context,
            view: view.clone(),
        };

        // ── Wire signals ───────────────────────────────────────

        // Close button
        let bar_close = bar.clone();
        close_btn.connect_clicked(move |_| bar_close.hide());

        // Escape on find entry
        let bar_esc = bar.clone();
        let key_ctl = gtk4::EventControllerKey::new();
        key_ctl.connect_key_pressed(move |_, key, _, _| {
            if key == gtk4::gdk::Key::Escape {
                bar_esc.hide();
                return gtk4::glib::Propagation::Stop;
            }
            gtk4::glib::Propagation::Proceed
        });
        find_entry.add_controller(key_ctl);

        // Escape on replace entry
        let bar_esc2 = bar.clone();
        let key_ctl2 = gtk4::EventControllerKey::new();
        key_ctl2.connect_key_pressed(move |_, key, _, _| {
            if key == gtk4::gdk::Key::Escape {
                bar_esc2.hide();
                return gtk4::glib::Propagation::Stop;
            }
            gtk4::glib::Propagation::Proceed
        });
        replace_entry.add_controller(key_ctl2);

        // Enter in find entry → find next
        let bar_activate = bar.clone();
        find_entry.connect_activate(move |_| {
            bar_activate.update_search_text();
            bar_activate.find_next();
        });

        // Live search as user types
        let bar_changed = bar.clone();
        find_entry.connect_changed(move |_| {
            bar_changed.update_search_text();
            bar_changed.find_next();
        });

        // Prev / Next buttons
        let bar_next = bar.clone();
        next_btn.connect_clicked(move |_| bar_next.find_next());
        let bar_prev = bar.clone();
        prev_btn.connect_clicked(move |_| bar_prev.find_prev());

        // Replace button
        let bar_rep = bar.clone();
        replace_btn.connect_clicked(move |_| bar_rep.replace_next());

        // Replace-all button
        let bar_rep_all = bar.clone();
        replace_all_btn.connect_clicked(move |_| bar_rep_all.replace_all());

        // Replace entry: Enter = replace next, Ctrl+Enter = replace all
        let bar_rep_key = bar.clone();
        let key_ctl3 = gtk4::EventControllerKey::new();
        key_ctl3.connect_key_pressed(move |_, key, _, modifiers| {
            if key == gtk4::gdk::Key::Return || key == gtk4::gdk::Key::KP_Enter {
                if modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
                    bar_rep_key.replace_all();
                } else {
                    bar_rep_key.replace_next();
                }
                return gtk4::glib::Propagation::Stop;
            }
            gtk4::glib::Propagation::Proceed
        });
        replace_entry.add_controller(key_ctl3);

        bar
    }

    /// Show the find bar, optionally with the replace row.
    pub fn show(&self, with_replace: bool) {
        self.replace_row.set_visible(with_replace);
        self.container.set_visible(true);
        self.context.borrow().set_highlight(true);
        self.find_entry.grab_focus();
        if !self.find_entry.text().is_empty() {
            self.find_entry.select_region(0, -1);
        }
    }

    /// Hide the find bar and clear highlighting.
    pub fn hide(&self) {
        self.container.set_visible(false);
        self.context.borrow().set_highlight(false);
        self.view.grab_focus();
    }

    /// The widget to add as an overlay child.
    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    // ── Private helpers ────────────────────────────────────────

    fn update_search_text(&self) {
        let text = self.find_entry.text();
        let ctx = self.context.borrow();
        let settings = ctx.settings();
        if text.is_empty() {
            settings.set_search_text(None);
        } else {
            settings.set_search_text(Some(&text));
        }
    }

    fn find_next(&self) {
        let ctx = self.context.borrow();
        let buffer = self.view.buffer();
        let buffer = buffer.downcast_ref::<sourceview5::Buffer>().unwrap();

        let (_, start) = buffer.selection_bounds().unwrap_or_else(|| {
            let cursor = buffer.iter_at_mark(&buffer.get_insert());
            (cursor, cursor)
        });

        if let Some((mut match_start, match_end, _wrapped)) = ctx.forward(&start) {
            buffer.select_range(&match_start, &match_end);
            self.view
                .scroll_to_iter(&mut match_start, 0.2, false, 0.0, 0.5);
            self.update_match_label(&ctx, &match_start, &match_end);
        } else {
            self.match_label.set_text("No results");
        }
    }

    fn find_prev(&self) {
        let ctx = self.context.borrow();
        let buffer = self.view.buffer();
        let buffer = buffer.downcast_ref::<sourceview5::Buffer>().unwrap();

        let (start, _) = buffer.selection_bounds().unwrap_or_else(|| {
            let cursor = buffer.iter_at_mark(&buffer.get_insert());
            (cursor, cursor)
        });

        if let Some((mut match_start, match_end, _wrapped)) = ctx.backward(&start) {
            buffer.select_range(&match_start, &match_end);
            self.view
                .scroll_to_iter(&mut match_start, 0.2, false, 0.0, 0.5);
            self.update_match_label(&ctx, &match_start, &match_end);
        } else {
            self.match_label.set_text("No results");
        }
    }

    fn replace_next(&self) {
        {
            let ctx = self.context.borrow();
            let buffer = self.view.buffer();
            let buffer = buffer.downcast_ref::<sourceview5::Buffer>().unwrap();
            let replacement = self.replace_entry.text().to_string();

            if let Some((mut match_start, mut match_end)) = buffer.selection_bounds() {
                let _ = ctx.replace(&mut match_start, &mut match_end, &replacement);
            }
        }
        self.find_next();
    }

    fn replace_all(&self) {
        let ctx = self.context.borrow();
        let replacement = self.replace_entry.text().to_string();
        match ctx.replace_all(&replacement) {
            Ok(_) => self.match_label.set_text("Replaced all"),
            Err(e) => {
                tracing::error!("replace all failed: {e}");
                self.match_label.set_text("Replace failed");
            }
        }
    }

    fn update_match_label(
        &self,
        ctx: &sourceview5::SearchContext,
        match_start: &gtk4::TextIter,
        match_end: &gtk4::TextIter,
    ) {
        let total = ctx.occurrences_count();
        let pos = ctx.occurrence_position(match_start, match_end);
        if total >= 0 && pos >= 0 {
            self.match_label.set_text(&format!("{pos} of {total}"));
        } else {
            self.match_label.set_text("");
        }
    }
}
