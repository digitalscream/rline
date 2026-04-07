//! Settings dialog — theme, font sizes, and behavior configuration.

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
            .default_height(400)
            .build();

        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        content.set_margin_top(16);
        content.set_margin_bottom(16);
        content.set_margin_start(16);
        content.set_margin_end(16);

        // ── Theme selector ──
        let theme_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let theme_label = gtk4::Label::new(Some("Theme"));
        theme_label.set_hexpand(true);
        theme_label.set_halign(gtk4::Align::Start);

        let scheme_manager = sourceview5::StyleSchemeManager::default();
        let scheme_ids = scheme_manager.scheme_ids();
        let scheme_strings: Vec<&str> = scheme_ids.iter().map(|s| s.as_str()).collect();
        let theme_dropdown = gtk4::DropDown::from_strings(&scheme_strings);

        if let Some(pos) = scheme_ids.iter().position(|s| s == &settings.theme) {
            theme_dropdown.set_selected(pos as u32);
        }

        theme_row.append(&theme_label);
        theme_row.append(&theme_dropdown);

        // ── Editor font size ──
        let font_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let font_label = gtk4::Label::new(Some("Editor Font Size"));
        font_label.set_hexpand(true);
        font_label.set_halign(gtk4::Align::Start);
        let font_spin = gtk4::SpinButton::with_range(5.0, 72.0, 1.0);
        font_spin.set_value(settings.font_size as f64);
        font_row.append(&font_label);
        font_row.append(&font_spin);

        // ── Terminal font size ──
        let term_font_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let term_font_label = gtk4::Label::new(Some("Terminal Font Size"));
        term_font_label.set_hexpand(true);
        term_font_label.set_halign(gtk4::Align::Start);
        let term_font_spin = gtk4::SpinButton::with_range(5.0, 72.0, 1.0);
        term_font_spin.set_value(settings.terminal_font_size as f64);
        term_font_row.append(&term_font_label);
        term_font_row.append(&term_font_spin);

        // ── Open last project on startup ──
        let last_project_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let last_project_label = gtk4::Label::new(Some("Open Last Project on Startup"));
        last_project_label.set_hexpand(true);
        last_project_label.set_halign(gtk4::Align::Start);
        let last_project_switch = gtk4::Switch::new();
        last_project_switch.set_active(settings.open_last_project);
        last_project_switch.set_valign(gtk4::Align::Center);
        last_project_row.append(&last_project_label);
        last_project_row.append(&last_project_switch);

        // ── Search auto-expand threshold ──
        let expand_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let expand_label = gtk4::Label::new(Some("Auto-expand Search (max results)"));
        expand_label.set_hexpand(true);
        expand_label.set_halign(gtk4::Align::Start);
        let expand_spin = gtk4::SpinButton::with_range(0.0, 100.0, 1.0);
        expand_spin.set_value(settings.search_auto_expand_threshold as f64);
        expand_row.append(&expand_label);
        expand_row.append(&expand_spin);

        // ── Buttons ──
        let button_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        button_box.set_halign(gtk4::Align::End);
        button_box.set_margin_top(16);

        let cancel_btn = gtk4::Button::with_label("Cancel");
        let apply_btn = gtk4::Button::with_label("Apply");
        apply_btn.add_css_class("suggested-action");

        button_box.append(&cancel_btn);
        button_box.append(&apply_btn);

        content.append(&theme_row);
        content.append(&font_row);
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

        // Wire apply
        let scheme_ids_owned: Vec<String> = scheme_ids.iter().map(|s| s.to_string()).collect();
        apply_btn.connect_clicked(glib::clone!(
            #[weak]
            window,
            #[weak]
            theme_dropdown,
            #[weak]
            font_spin,
            #[weak]
            term_font_spin,
            #[weak]
            last_project_switch,
            #[weak]
            expand_spin,
            move |_| {
                let selected = theme_dropdown.selected() as usize;
                let theme = scheme_ids_owned
                    .get(selected)
                    .cloned()
                    .unwrap_or_else(|| "Adwaita-dark".to_owned());

                // Preserve last_project_path from existing settings
                let existing = EditorSettings::load().unwrap_or_default();
                let new_settings = EditorSettings {
                    theme,
                    font_size: font_spin.value() as u32,
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
                window.close();
            }
        ));

        Self { window }
    }

    /// Present the dialog.
    pub fn present(&self) {
        self.window.present();
    }
}
