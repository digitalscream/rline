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

        // ── Import VS Code theme ──
        let import_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        import_row.set_halign(gtk4::Align::End);
        let import_btn = gtk4::Button::with_label("Import VS Code Theme...");
        import_row.append(&import_btn);

        import_btn.connect_clicked(glib::clone!(
            #[weak]
            window,
            #[weak]
            theme_dropdown,
            move |_| {
                Self::show_vscode_import_dialog(&window, &theme_dropdown);
            }
        ));

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

        // ── Tab width ──
        let tab_width_row = Self::make_row("Tab Width");
        let tab_width_spin = gtk4::SpinButton::with_range(1.0, 16.0, 1.0);
        tab_width_spin.set_value(settings.tab_width as f64);
        tab_width_row.append(&tab_width_spin);

        // ── Insert spaces instead of tabs ──
        let insert_spaces_row = Self::make_row("Insert Spaces");
        let insert_spaces_switch = gtk4::Switch::new();
        insert_spaces_switch.set_active(settings.insert_spaces);
        insert_spaces_switch.set_valign(gtk4::Align::Center);
        insert_spaces_row.append(&insert_spaces_switch);

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

        // ── Tab cycle depth ──
        let cycle_row = Self::make_row("Ctrl+Tab Cycle Depth");
        let cycle_spin = gtk4::SpinButton::with_range(2.0, 50.0, 1.0);
        cycle_spin.set_value(settings.tab_cycle_depth as f64);
        cycle_row.append(&cycle_spin);

        // ── Tree-sitter highlighting ──
        let treesitter_row = Self::make_row("Tree-sitter Highlighting");
        let treesitter_switch = gtk4::Switch::new();
        treesitter_switch.set_active(settings.use_treesitter);
        treesitter_switch.set_valign(gtk4::Align::Center);
        treesitter_row.append(&treesitter_switch);

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
        content.append(&import_row);
        content.append(&editor_font_row);
        content.append(&font_row);
        content.append(&tab_width_row);
        content.append(&insert_spaces_row);
        content.append(&term_font_fam_row);
        content.append(&term_font_row);
        content.append(&last_project_row);
        content.append(&expand_row);
        content.append(&cycle_row);
        content.append(&treesitter_row);
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
        let mono_fonts_owned = mono_fonts.clone();
        let on_apply = Rc::new(on_apply);

        // Wire Apply (apply changes, keep dialog open)
        let do_apply = {
            let mono_fonts = mono_fonts_owned.clone();
            let on_apply = on_apply.clone();
            Rc::new(
                move |theme_dropdown: &gtk4::DropDown,
                      editor_font_dropdown: &gtk4::DropDown,
                      font_spin: &gtk4::SpinButton,
                      tab_width_spin: &gtk4::SpinButton,
                      insert_spaces_switch: &gtk4::Switch,
                      term_font_dropdown: &gtk4::DropDown,
                      term_font_spin: &gtk4::SpinButton,
                      last_project_switch: &gtk4::Switch,
                      expand_spin: &gtk4::SpinButton,
                      cycle_spin: &gtk4::SpinButton,
                      treesitter_switch: &gtk4::Switch| {
                    // Read selected theme from the dropdown's model (handles dynamically added items)
                    let theme = theme_dropdown
                        .selected_item()
                        .and_then(|obj| obj.downcast::<gtk4::StringObject>().ok())
                        .map(|so| so.string().to_string())
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
                        tab_width: tab_width_spin.value() as u32,
                        insert_spaces: insert_spaces_switch.is_active(),
                        terminal_font_family: terminal_font,
                        terminal_font_size: term_font_spin.value() as u32,
                        open_last_project: last_project_switch.is_active(),
                        last_project_path: existing.last_project_path,
                        search_auto_expand_threshold: expand_spin.value() as u32,
                        tab_cycle_depth: cycle_spin.value() as u32,
                        use_treesitter: treesitter_switch.is_active(),
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
            tab_width_spin,
            #[weak]
            insert_spaces_switch,
            #[weak]
            term_font_dropdown,
            #[weak]
            term_font_spin,
            #[weak]
            last_project_switch,
            #[weak]
            expand_spin,
            #[weak]
            cycle_spin,
            #[weak]
            treesitter_switch,
            move |_| {
                do_apply_for_apply(
                    &theme_dropdown,
                    &editor_font_dropdown,
                    &font_spin,
                    &tab_width_spin,
                    &insert_spaces_switch,
                    &term_font_dropdown,
                    &term_font_spin,
                    &last_project_switch,
                    &expand_spin,
                    &cycle_spin,
                    &treesitter_switch,
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
            tab_width_spin,
            #[weak]
            insert_spaces_switch,
            #[weak]
            term_font_dropdown,
            #[weak]
            term_font_spin,
            #[weak]
            last_project_switch,
            #[weak]
            expand_spin,
            #[weak]
            cycle_spin,
            #[weak]
            treesitter_switch,
            move |_| {
                do_apply(
                    &theme_dropdown,
                    &editor_font_dropdown,
                    &font_spin,
                    &tab_width_spin,
                    &insert_spaces_switch,
                    &term_font_dropdown,
                    &term_font_spin,
                    &last_project_switch,
                    &expand_spin,
                    &cycle_spin,
                    &treesitter_switch,
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

    /// Show a dialog to import a VS Code theme.
    fn show_vscode_import_dialog(parent: &gtk4::Window, theme_dropdown: &gtk4::DropDown) {
        use rline_config::vscode_import;

        let themes = vscode_import::discover_vscode_themes();
        if themes.is_empty() {
            let alert = gtk4::AlertDialog::builder()
                .message("No VS Code Themes Found")
                .detail("No VS Code installation with theme extensions was found on this system.\n\nChecked: ~/.vscode/extensions, ~/.vscode-insiders/extensions")
                .build();
            alert.show(Some(parent));
            return;
        }

        let dialog = gtk4::Window::builder()
            .title("Import VS Code Theme")
            .modal(true)
            .transient_for(parent)
            .default_width(450)
            .default_height(350)
            .build();

        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);

        let label = gtk4::Label::new(Some("Select a theme to import:"));
        label.set_halign(gtk4::Align::Start);
        content.append(&label);

        // Build a scrollable list of themes
        let scrolled = gtk4::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .min_content_height(200)
            .build();

        let list_box = gtk4::ListBox::new();
        list_box.set_selection_mode(gtk4::SelectionMode::Single);

        for theme in &themes {
            let row_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
            row_box.set_margin_top(4);
            row_box.set_margin_bottom(4);
            row_box.set_margin_start(8);
            row_box.set_margin_end(8);

            let name_label = gtk4::Label::new(Some(&theme.label));
            name_label.set_halign(gtk4::Align::Start);
            name_label.add_css_class("heading");

            let detail = format!(
                "{} ({} theme)",
                theme.extension_name,
                if theme.ui_theme.contains("dark") {
                    "dark"
                } else {
                    "light"
                }
            );
            let detail_label = gtk4::Label::new(Some(&detail));
            detail_label.set_halign(gtk4::Align::Start);
            detail_label.add_css_class("dim-label");

            row_box.append(&name_label);
            row_box.append(&detail_label);
            list_box.append(&row_box);
        }

        scrolled.set_child(Some(&list_box));
        content.append(&scrolled);

        // Buttons
        let button_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        button_box.set_halign(gtk4::Align::End);
        button_box.set_margin_top(8);

        let cancel_btn = gtk4::Button::with_label("Cancel");
        let import_btn = gtk4::Button::with_label("Import");
        import_btn.add_css_class("suggested-action");

        button_box.append(&cancel_btn);
        button_box.append(&import_btn);
        content.append(&button_box);

        dialog.set_child(Some(&content));

        cancel_btn.connect_clicked(glib::clone!(
            #[weak]
            dialog,
            move |_| {
                dialog.close();
            }
        ));

        let themes = Rc::new(themes);
        import_btn.connect_clicked(glib::clone!(
            #[weak]
            dialog,
            #[weak]
            list_box,
            #[weak]
            theme_dropdown,
            #[strong]
            themes,
            move |_| {
                let Some(row) = list_box.selected_row() else {
                    return;
                };
                let idx = row.index() as usize;
                let Some(entry) = themes.get(idx) else {
                    return;
                };

                match Self::import_vscode_theme(entry, &theme_dropdown) {
                    Ok(scheme_id) => {
                        tracing::info!("imported VS Code theme as: {scheme_id}");
                        dialog.close();
                    }
                    Err(e) => {
                        tracing::error!("failed to import theme: {e}");
                        let alert = gtk4::AlertDialog::builder()
                            .message("Import Failed")
                            .detail(format!("Could not import theme: {e}"))
                            .build();
                        alert.show(Some(&dialog));
                    }
                }
            }
        ));

        // Select the first row by default
        if let Some(first_row) = list_box.row_at_index(0) {
            list_box.select_row(Some(&first_row));
        }

        dialog.present();
    }

    /// Import a single VS Code theme: convert, install, and update the dropdown.
    fn import_vscode_theme(
        entry: &rline_config::vscode_import::VscodeThemeEntry,
        theme_dropdown: &gtk4::DropDown,
    ) -> Result<String, rline_config::ConfigError> {
        use rline_config::vscode_import;
        use sourceview5::prelude::*;

        let scheme_id = vscode_import::import_vscode_theme(entry)?;

        // Add the user styles directory to the search path and rescan
        let scheme_manager = sourceview5::StyleSchemeManager::default();
        if let Ok(styles_dir) = rline_config::paths::gtksourceview_styles_dir() {
            let styles_path = styles_dir.to_string_lossy().to_string();
            let current_paths = scheme_manager.search_path();
            if !current_paths.iter().any(|p| p.as_str() == styles_path) {
                scheme_manager.append_search_path(&styles_path);
            }
        }
        scheme_manager.force_rescan();

        // Add the new scheme ID to the dropdown model
        if let Some(model) = theme_dropdown.model() {
            if let Ok(string_list) = model.downcast::<gtk4::StringList>() {
                string_list.append(&scheme_id);
                // Select the newly imported theme
                let new_idx = string_list.n_items() - 1;
                theme_dropdown.set_selected(new_idx);
            }
        }

        Ok(scheme_id)
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
