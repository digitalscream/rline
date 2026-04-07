//! Settings dialog — theme, fonts, and behavior configuration.

use std::rc::Rc;

use gtk4::prelude::*;

use rline_config::EditorSettings;

/// A dialog for editing editor settings.
#[derive(Debug)]
pub struct SettingsDialog {
    window: gtk4::Window,
}

impl SettingsDialog {
    /// Create a new settings dialog.
    ///
    /// The `on_apply` callback is invoked when the user applies changes.
    pub fn new<F>(parent: &gtk4::Window, on_apply: F) -> Self
    where
        F: Fn(EditorSettings) + 'static,
    {
        let settings = EditorSettings::load().unwrap_or_default();

        let window = gtk4::Window::builder()
            .title("Settings")
            .modal(true)
            .transient_for(parent)
            .default_width(450)
            .default_height(480)
            .build();

        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        content.set_margin_top(16);
        content.set_margin_bottom(16);
        content.set_margin_start(16);
        content.set_margin_end(16);

        // ── Theme selector ──
        let theme_row = Self::make_row("Theme");
        let scheme_manager = sourceview5::StyleSchemeManager::default();
        let scheme_ids = scheme_manager.scheme_ids();
        let scheme_strings: Vec<&str> = scheme_ids.iter().map(|s| s.as_str()).collect();
        let theme_dropdown = gtk4::DropDown::from_strings(&scheme_strings);
        if let Some(pos) = scheme_ids.iter().position(|s| s == &settings.theme) {
            theme_dropdown.set_selected(pos as u32);
        }
        theme_row.append(&theme_dropdown);

        // ── Enumerate monospace fonts ──
        let mono_fonts = Self::list_monospace_fonts();
        let mono_strs: Vec<&str> = mono_fonts.iter().map(|s| s.as_str()).collect();

        // ── Editor font family ──
        let editor_font_row = Self::make_row("Editor Font");
        let editor_font_dropdown = gtk4::DropDown::from_strings(&mono_strs);
        if let Some(pos) = mono_fonts
            .iter()
            .position(|f| f == &settings.editor_font_family)
        {
            editor_font_dropdown.set_selected(pos as u32);
        }
        editor_font_row.append(&editor_font_dropdown);

        // ── Editor font size ──
        let font_row = Self::make_row("Editor Font Size");
        let font_spin = gtk4::SpinButton::with_range(5.0, 72.0, 1.0);
        font_spin.set_value(settings.font_size as f64);
        font_row.append(&font_spin);

        // ── Terminal font family ──
        let term_font_fam_row = Self::make_row("Terminal Font");
        let term_font_dropdown = gtk4::DropDown::from_strings(&mono_strs);
        if let Some(pos) = mono_fonts
            .iter()
            .position(|f| f == &settings.terminal_font_family)
        {
            term_font_dropdown.set_selected(pos as u32);
        }
        term_font_fam_row.append(&term_font_dropdown);

        // ── Terminal font size ──
        let term_font_row = Self::make_row("Terminal Font Size");
        let term_font_spin = gtk4::SpinButton::with_range(5.0, 72.0, 1.0);
        term_font_spin.set_value(settings.terminal_font_size as f64);
        term_font_row.append(&term_font_spin);

        // ── Open last project on startup ──
        let last_project_row = Self::make_row("Open Last Project on Startup");
        let last_project_switch = gtk4::Switch::new();
        last_project_switch.set_active(settings.open_last_project);
        last_project_switch.set_valign(gtk4::Align::Center);
        last_project_row.append(&last_project_switch);

        // ── Search auto-expand threshold ──
        let expand_row = Self::make_row("Auto-expand Search (max results)");
        let expand_spin = gtk4::SpinButton::with_range(0.0, 100.0, 1.0);
        expand_spin.set_value(settings.search_auto_expand_threshold as f64);
        expand_row.append(&expand_spin);

        // ── Buttons ──
        let button_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        button_box.set_halign(gtk4::Align::End);
        button_box.set_margin_top(16);
        let cancel_btn = gtk4::Button::with_label("Cancel");
        let apply_btn = gtk4::Button::with_label("Apply");
        let ok_btn = gtk4::Button::with_label("OK");
        ok_btn.add_css_class("suggested-action");
        button_box.append(&cancel_btn);
        button_box.append(&apply_btn);
        button_box.append(&ok_btn);

        content.append(&theme_row);
        content.append(&editor_font_row);
        content.append(&font_row);
        content.append(&term_font_fam_row);
        content.append(&term_font_row);
        content.append(&last_project_row);
        content.append(&expand_row);
        content.append(&button_box);

        window.set_child(Some(&content));

        // Wire cancel
        cancel_btn.connect_clicked(glib::clone!(
            #[weak]
            window,
            move |_| {
                window.close();
            }
        ));

        // Shared apply logic wrapped in Rc so both Apply and OK can call it
        let scheme_ids_owned: Vec<String> = scheme_ids.iter().map(|s| s.to_string()).collect();
        let mono_fonts_owned = mono_fonts.clone();
        let on_apply = Rc::new(on_apply);

        // Wire Apply (apply changes, keep dialog open)
        let do_apply = {
            let scheme_ids = scheme_ids_owned.clone();
            let mono_fonts = mono_fonts_owned.clone();
            let on_apply = on_apply.clone();
            Rc::new(
                move |theme_dropdown: &gtk4::DropDown,
                      editor_font_dropdown: &gtk4::DropDown,
                      font_spin: &gtk4::SpinButton,
                      term_font_dropdown: &gtk4::DropDown,
                      term_font_spin: &gtk4::SpinButton,
                      last_project_switch: &gtk4::Switch,
                      expand_spin: &gtk4::SpinButton| {
                    let selected = theme_dropdown.selected() as usize;
                    let theme = scheme_ids
                        .get(selected)
                        .cloned()
                        .unwrap_or_else(|| "Adwaita-dark".to_owned());

                    let editor_font_idx = editor_font_dropdown.selected() as usize;
                    let editor_font = mono_fonts
                        .get(editor_font_idx)
                        .cloned()
                        .unwrap_or_else(|| "Monospace".to_owned());

                    let term_font_idx = term_font_dropdown.selected() as usize;
                    let terminal_font = mono_fonts
                        .get(term_font_idx)
                        .cloned()
                        .unwrap_or_else(|| "Monospace".to_owned());

                    let existing = EditorSettings::load().unwrap_or_default();
                    let new_settings = EditorSettings {
                        theme,
                        editor_font_family: editor_font,
                        font_size: font_spin.value() as u32,
                        terminal_font_family: terminal_font,
                        terminal_font_size: term_font_spin.value() as u32,
                        open_last_project: last_project_switch.is_active(),
                        last_project_path: existing.last_project_path,
                        search_auto_expand_threshold: expand_spin.value() as u32,
                        ..existing
                    };

                    if let Err(e) = new_settings.save() {
                        tracing::error!("failed to save settings: {e}");
                    }

                    on_apply(new_settings);
                },
            )
        };

        let do_apply_for_apply = do_apply.clone();
        apply_btn.connect_clicked(glib::clone!(
            #[weak]
            theme_dropdown,
            #[weak]
            editor_font_dropdown,
            #[weak]
            font_spin,
            #[weak]
            term_font_dropdown,
            #[weak]
            term_font_spin,
            #[weak]
            last_project_switch,
            #[weak]
            expand_spin,
            move |_| {
                do_apply_for_apply(
                    &theme_dropdown,
                    &editor_font_dropdown,
                    &font_spin,
                    &term_font_dropdown,
                    &term_font_spin,
                    &last_project_switch,
                    &expand_spin,
                );
            }
        ));

        // Wire OK (apply changes + close dialog)
        ok_btn.connect_clicked(glib::clone!(
            #[weak]
            window,
            #[weak]
            theme_dropdown,
            #[weak]
            editor_font_dropdown,
            #[weak]
            font_spin,
            #[weak]
            term_font_dropdown,
            #[weak]
            term_font_spin,
            #[weak]
            last_project_switch,
            #[weak]
            expand_spin,
            move |_| {
                do_apply(
                    &theme_dropdown,
                    &editor_font_dropdown,
                    &font_spin,
                    &term_font_dropdown,
                    &term_font_spin,
                    &last_project_switch,
                    &expand_spin,
                );
                window.close();
            }
        ));

        Self { window }
    }

    /// Present the dialog.
    pub fn present(&self) {
        self.window.present();
    }

    /// Helper to create a settings row with a left-aligned label.
    fn make_row(label_text: &str) -> gtk4::Box {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let label = gtk4::Label::new(Some(label_text));
        label.set_hexpand(true);
        label.set_halign(gtk4::Align::Start);
        row.append(&label);
        row
    }

    /// Enumerate available monospace font families from Pango.
    fn list_monospace_fonts() -> Vec<String> {
        // Use a temporary label to access the system Pango font map
        let tmp = gtk4::Label::new(None);
        let pango_ctx = tmp.pango_context();
        let mut mono_names: Vec<String> = pango_ctx
            .list_families()
            .iter()
            .filter(|fam| fam.is_monospace())
            .map(|fam| fam.name().to_string())
            .collect();
        mono_names.sort_by_key(|a| a.to_lowercase());
        mono_names.dedup();

        // Ensure "Monospace" is always present as a fallback
        if !mono_names.iter().any(|n| n == "Monospace") {
            mono_names.insert(0, "Monospace".to_owned());
        }

        mono_names
    }
}
