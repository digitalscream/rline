//! Settings dialog — theme, fonts, behavior, and AI completion configuration.

use std::rc::Rc;

use gtk4::prelude::*;

use rline_config::EditorSettings;

use rline_ai::agent::context::DEFAULT_SYSTEM_PROMPT;

/// A dialog for editing editor settings.
#[derive(Debug)]
pub struct SettingsDialog {
    window: gtk4::Window,
}

impl SettingsDialog {
    /// Create a new settings dialog.
    ///
    /// The `on_apply` callback is invoked when the user applies changes.
    pub fn new<F, G>(parent: &gtk4::Window, on_apply: F, on_open_file: G) -> Self
    where
        F: Fn(EditorSettings) + 'static,
        G: Fn(std::path::PathBuf) + 'static,
    {
        let settings = EditorSettings::load().unwrap_or_default();

        let window = gtk4::Window::builder()
            .title("Settings")
            .modal(true)
            .transient_for(parent)
            .default_width(450)
            .default_height(620)
            .build();

        // ── Notebook with three pages ──
        let notebook = gtk4::Notebook::new();
        notebook.set_vexpand(true);

        // ════════════════════════════════════════════════════════════
        //  Tab 1 — Editor
        // ════════════════════════════════════════════════════════════
        let editor_page = Self::build_editor_page(&settings, &window);

        // ════════════════════════════════════════════════════════════
        //  Tab 2 — Completion
        // ════════════════════════════════════════════════════════════
        let completion_page = Self::build_completion_page(&settings);

        // ════════════════════════════════════════════════════════════
        //  Tab 3 — Agent
        // ════════════════════════════════════════════════════════════
        let on_open_file = Rc::new(on_open_file);
        let agent_page = Self::build_agent_page(&settings, &window, &on_open_file);

        notebook.append_page(
            &editor_page.scrolled,
            Some(&gtk4::Label::new(Some("Editor"))),
        );
        notebook.append_page(
            &completion_page.scrolled,
            Some(&gtk4::Label::new(Some("Completion"))),
        );
        notebook.append_page(&agent_page.scrolled, Some(&gtk4::Label::new(Some("Agent"))));

        // ── Buttons ──
        let button_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        button_box.set_halign(gtk4::Align::End);
        button_box.set_margin_top(8);
        button_box.set_margin_bottom(8);
        button_box.set_margin_end(16);
        let cancel_btn = gtk4::Button::with_label("Cancel");
        let apply_btn = gtk4::Button::with_label("Apply");
        let ok_btn = gtk4::Button::with_label("OK");
        ok_btn.add_css_class("suggested-action");
        button_box.append(&cancel_btn);
        button_box.append(&apply_btn);
        button_box.append(&ok_btn);

        let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        outer.append(&notebook);
        outer.append(&button_box);
        window.set_child(Some(&outer));

        // Wire cancel
        cancel_btn.connect_clicked(glib::clone!(
            #[weak]
            window,
            move |_| {
                window.close();
            }
        ));

        // Shared apply logic
        let on_apply = Rc::new(on_apply);

        let do_apply = {
            let ep = editor_page.clone();
            let cp = completion_page.clone();
            let ap = agent_page.clone();
            let on_apply = on_apply.clone();
            Rc::new(move || {
                tracing::info!(
                    "do_apply: switch.is_active={}, switch.state={}",
                    cp.enabled_switch.is_active(),
                    cp.enabled_switch.state(),
                );
                let existing = EditorSettings::load().unwrap_or_default();
                let new_settings = EditorSettings {
                    // Editor page
                    theme: ep.read_theme(),
                    editor_font_family: ep.read_editor_font(),
                    font_size: ep.font_spin.value() as u32,
                    letter_spacing: ep.letter_spacing_spin.value(),
                    line_height: ep.line_height_spin.value(),
                    hint_style: ep.read_hint_style(),
                    tab_width: ep.tab_width_spin.value() as u32,
                    insert_spaces: ep.insert_spaces_switch.is_active(),
                    terminal_font_family: ep.read_terminal_font(),
                    terminal_font_size: ep.term_font_spin.value() as u32,
                    open_last_project: ep.last_project_switch.is_active(),
                    last_project_path: existing.last_project_path,
                    search_auto_expand_threshold: ep.expand_spin.value() as u32,
                    tab_cycle_depth: ep.cycle_spin.value() as u32,
                    use_treesitter: ep.treesitter_switch.is_active(),
                    // Completion page
                    ai_enabled: cp.enabled_switch.is_active(),
                    ai_endpoint_url: cp.endpoint_entry.text().to_string(),
                    ai_api_key: cp.api_key_entry.text().to_string(),
                    ai_model: cp.model_entry.text().to_string(),
                    ai_trigger_mode: cp.read_trigger_mode(),
                    ai_debounce_ms: cp.debounce_spin.value() as u32,
                    ai_max_tokens: cp.max_tokens_spin.value() as u32,
                    ai_context_lines_before: cp.context_before_spin.value() as u32,
                    ai_context_lines_after: cp.context_after_spin.value() as u32,
                    ai_max_lines: cp.max_lines_spin.value() as u32,
                    ai_temperature: cp.temperature_spin.value(),
                    // Agent page
                    agent_endpoint_url: ap.endpoint_entry.text().to_string(),
                    agent_api_key: ap.api_key_entry.text().to_string(),
                    agent_model: ap.model_entry.text().to_string(),
                    agent_max_tokens: ap.max_tokens_spin.value() as u32,
                    agent_temperature: ap.temperature_spin.value(),
                    agent_auto_approve_read: ap.auto_approve_read_switch.is_active(),
                    agent_auto_approve_edit: ap.auto_approve_edit_switch.is_active(),
                    agent_auto_approve_command: ap.auto_approve_command_switch.is_active(),
                    agent_yolo_mode: ap.yolo_mode_switch.is_active(),
                    agent_command_timeout_secs: ap.command_timeout_spin.value() as u32,
                    agent_context_length: ap.context_length_spin.value() as u32,
                    agent_max_turns: ap.max_turns_spin.value() as u32,
                    ..existing
                };

                if let Err(e) = new_settings.save() {
                    tracing::error!("failed to save settings: {e}");
                }
                on_apply(new_settings);
            })
        };

        let do_apply_for_apply = do_apply.clone();
        apply_btn.connect_clicked(move |_| {
            do_apply_for_apply();
        });

        ok_btn.connect_clicked(glib::clone!(
            #[weak]
            window,
            move |_| {
                do_apply();
                window.close();
            }
        ));

        Self { window }
    }

    /// Present the dialog.
    pub fn present(&self) {
        self.window.present();
    }

    // ── Editor page ───────────────────────────────────────────────

    fn build_editor_page(settings: &EditorSettings, window: &gtk4::Window) -> EditorPageWidgets {
        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        content.set_margin_top(16);
        content.set_margin_bottom(16);
        content.set_margin_start(16);
        content.set_margin_end(16);

        // Theme selector
        let theme_row = Self::make_row("Theme");
        let scheme_manager = sourceview5::StyleSchemeManager::default();
        let scheme_ids = scheme_manager.scheme_ids();
        let scheme_strings: Vec<&str> = scheme_ids.iter().map(|s| s.as_str()).collect();
        let theme_dropdown = gtk4::DropDown::from_strings(&scheme_strings);
        if let Some(pos) = scheme_ids.iter().position(|s| s == &settings.theme) {
            theme_dropdown.set_selected(pos as u32);
        }
        theme_row.append(&theme_dropdown);

        // Import theme buttons
        let import_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        import_row.set_halign(gtk4::Align::End);

        let import_vscode_btn = gtk4::Button::with_label("Import VS Code Theme...");
        import_row.append(&import_vscode_btn);

        let import_zed_btn = gtk4::Button::with_label("Import Zed Theme...");
        import_row.append(&import_zed_btn);

        import_vscode_btn.connect_clicked(glib::clone!(
            #[weak]
            window,
            #[weak]
            theme_dropdown,
            move |_| {
                Self::show_vscode_import_dialog(&window, &theme_dropdown);
            }
        ));

        import_zed_btn.connect_clicked(glib::clone!(
            #[weak]
            window,
            #[weak]
            theme_dropdown,
            move |_| {
                Self::show_zed_import_dialog(&window, &theme_dropdown);
            }
        ));

        // Monospace fonts
        let mono_fonts = Self::list_monospace_fonts();
        let mono_strs: Vec<&str> = mono_fonts.iter().map(|s| s.as_str()).collect();

        // Editor font family
        let editor_font_row = Self::make_row("Editor Font");
        let editor_font_dropdown = gtk4::DropDown::from_strings(&mono_strs);
        if let Some(pos) = mono_fonts
            .iter()
            .position(|f| f == &settings.editor_font_family)
        {
            editor_font_dropdown.set_selected(pos as u32);
        }
        editor_font_row.append(&editor_font_dropdown);

        // Editor font size
        let font_row = Self::make_row("Editor Font Size");
        let font_spin = gtk4::SpinButton::with_range(5.0, 72.0, 1.0);
        font_spin.set_value(settings.font_size as f64);
        font_row.append(&font_spin);

        // Tab width
        let tab_width_row = Self::make_row("Tab Width");
        let tab_width_spin = gtk4::SpinButton::with_range(1.0, 16.0, 1.0);
        tab_width_spin.set_value(settings.tab_width as f64);
        tab_width_row.append(&tab_width_spin);

        // Insert spaces
        let insert_spaces_row = Self::make_row("Insert Spaces");
        let insert_spaces_switch = gtk4::Switch::new();
        insert_spaces_switch.set_active(settings.insert_spaces);
        insert_spaces_switch.set_valign(gtk4::Align::Center);
        insert_spaces_row.append(&insert_spaces_switch);

        // Terminal font family
        let term_font_fam_row = Self::make_row("Terminal Font");
        let term_font_dropdown = gtk4::DropDown::from_strings(&mono_strs);
        if let Some(pos) = mono_fonts
            .iter()
            .position(|f| f == &settings.terminal_font_family)
        {
            term_font_dropdown.set_selected(pos as u32);
        }
        term_font_fam_row.append(&term_font_dropdown);

        // Terminal font size
        let term_font_row = Self::make_row("Terminal Font Size");
        let term_font_spin = gtk4::SpinButton::with_range(5.0, 72.0, 1.0);
        term_font_spin.set_value(settings.terminal_font_size as f64);
        term_font_row.append(&term_font_spin);

        // Open last project on startup
        let last_project_row = Self::make_row("Open Last Project on Startup");
        let last_project_switch = gtk4::Switch::new();
        last_project_switch.set_active(settings.open_last_project);
        last_project_switch.set_valign(gtk4::Align::Center);
        last_project_row.append(&last_project_switch);

        // Search auto-expand threshold
        let expand_row = Self::make_row("Auto-expand Search (max results)");
        let expand_spin = gtk4::SpinButton::with_range(0.0, 100.0, 1.0);
        expand_spin.set_value(settings.search_auto_expand_threshold as f64);
        expand_row.append(&expand_spin);

        // Tab cycle depth
        let cycle_row = Self::make_row("Ctrl+Tab Cycle Depth");
        let cycle_spin = gtk4::SpinButton::with_range(2.0, 50.0, 1.0);
        cycle_spin.set_value(settings.tab_cycle_depth as f64);
        cycle_row.append(&cycle_spin);

        // Letter spacing
        let letter_spacing_row = Self::make_row("Letter Spacing (px)");
        let letter_spacing_spin = gtk4::SpinButton::with_range(0.0, 5.0, 0.1);
        letter_spacing_spin.set_digits(1);
        letter_spacing_spin.set_value(settings.letter_spacing);
        letter_spacing_row.append(&letter_spacing_spin);

        // Line height
        let line_height_row = Self::make_row("Line Height");
        let line_height_spin = gtk4::SpinButton::with_range(1.0, 3.0, 0.1);
        line_height_spin.set_digits(1);
        line_height_spin.set_value(settings.line_height);
        line_height_row.append(&line_height_spin);

        // Hint level
        let hint_row = Self::make_row("Font Hinting");
        let hint_dropdown = gtk4::DropDown::from_strings(&["full", "slight"]);
        let hint_idx = if settings.hint_style == "slight" {
            1
        } else {
            0
        };
        hint_dropdown.set_selected(hint_idx);
        hint_row.append(&hint_dropdown);

        // Tree-sitter highlighting
        let treesitter_row = Self::make_row("Tree-sitter Highlighting");
        let treesitter_switch = gtk4::Switch::new();
        treesitter_switch.set_active(settings.use_treesitter);
        treesitter_switch.set_valign(gtk4::Align::Center);
        treesitter_row.append(&treesitter_switch);

        content.append(&theme_row);
        content.append(&import_row);
        content.append(&editor_font_row);
        content.append(&font_row);
        content.append(&letter_spacing_row);
        content.append(&line_height_row);
        content.append(&hint_row);
        content.append(&tab_width_row);
        content.append(&insert_spaces_row);
        content.append(&term_font_fam_row);
        content.append(&term_font_row);
        content.append(&last_project_row);
        content.append(&expand_row);
        content.append(&cycle_row);
        content.append(&treesitter_row);

        let scrolled = gtk4::ScrolledWindow::builder()
            .child(&content)
            .vexpand(true)
            .hexpand(true)
            .build();

        EditorPageWidgets {
            scrolled,
            theme_dropdown,
            editor_font_dropdown,
            mono_fonts,
            font_spin,
            letter_spacing_spin,
            line_height_spin,
            hint_dropdown,
            tab_width_spin,
            insert_spaces_switch,
            term_font_dropdown,
            term_font_spin,
            last_project_switch,
            expand_spin,
            cycle_spin,
            treesitter_switch,
        }
    }

    // ── Completion page ───────────────────────────────────────────

    fn build_completion_page(settings: &EditorSettings) -> CompletionPageWidgets {
        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        content.set_margin_top(16);
        content.set_margin_bottom(16);
        content.set_margin_start(16);
        content.set_margin_end(16);

        // Enable AI Completion
        let enabled_row = Self::make_row("Enable AI Completion");
        let enabled_switch = gtk4::Switch::new();
        enabled_switch.set_active(settings.ai_enabled);
        enabled_switch.set_valign(gtk4::Align::Center);
        enabled_row.append(&enabled_switch);

        // Endpoint URL
        let endpoint_row = Self::make_row("Endpoint URL");
        let endpoint_entry = gtk4::Entry::builder()
            .text(&settings.ai_endpoint_url)
            .hexpand(true)
            .build();
        endpoint_row.append(&endpoint_entry);

        // API Key
        let api_key_row = Self::make_row("API Key");
        let api_key_entry = gtk4::PasswordEntry::builder()
            .show_peek_icon(true)
            .hexpand(true)
            .build();
        api_key_entry.set_text(&settings.ai_api_key);
        api_key_row.append(&api_key_entry);

        // Model
        let model_row = Self::make_row("Model");
        let model_entry = gtk4::Entry::builder()
            .text(&settings.ai_model)
            .placeholder_text("e.g. codellama, deepseek-coder")
            .hexpand(true)
            .build();
        model_row.append(&model_entry);

        // Trigger Mode
        let trigger_row = Self::make_row("Trigger Mode");
        let trigger_dropdown = gtk4::DropDown::from_strings(&["Automatic", "Manual", "Both"]);
        let trigger_idx = match settings.ai_trigger_mode.as_str() {
            "manual" => 1,
            "both" => 2,
            _ => 0, // "automatic"
        };
        trigger_dropdown.set_selected(trigger_idx);
        trigger_row.append(&trigger_dropdown);

        // Debounce (ms)
        let debounce_row = Self::make_row("Debounce (ms)");
        let debounce_spin = gtk4::SpinButton::with_range(50.0, 2000.0, 50.0);
        debounce_spin.set_value(settings.ai_debounce_ms as f64);
        debounce_row.append(&debounce_spin);

        // Max Tokens
        let max_tokens_row = Self::make_row("Max Tokens");
        let max_tokens_spin = gtk4::SpinButton::with_range(16.0, 1024.0, 16.0);
        max_tokens_spin.set_value(settings.ai_max_tokens as f64);
        max_tokens_row.append(&max_tokens_spin);

        // Context Lines Before
        let context_before_row = Self::make_row("Context Lines Before");
        let context_before_spin = gtk4::SpinButton::with_range(32.0, 512.0, 32.0);
        context_before_spin.set_value(settings.ai_context_lines_before as f64);
        context_before_row.append(&context_before_spin);

        // Context Lines After
        let context_after_row = Self::make_row("Context Lines After");
        let context_after_spin = gtk4::SpinButton::with_range(16.0, 256.0, 16.0);
        context_after_spin.set_value(settings.ai_context_lines_after as f64);
        context_after_row.append(&context_after_spin);

        // Max Lines (0 = unlimited)
        let max_lines_row = Self::make_row("Max Lines (0 = unlimited)");
        let max_lines_spin = gtk4::SpinButton::with_range(0.0, 100.0, 1.0);
        max_lines_spin.set_value(settings.ai_max_lines as f64);
        max_lines_row.append(&max_lines_spin);

        // Temperature
        let temperature_row = Self::make_row("Temperature");
        let temperature_spin = gtk4::SpinButton::with_range(0.0, 2.0, 0.1);
        temperature_spin.set_digits(1);
        temperature_spin.set_value(settings.ai_temperature);
        temperature_row.append(&temperature_spin);

        content.append(&enabled_row);
        content.append(&endpoint_row);
        content.append(&api_key_row);
        content.append(&model_row);
        content.append(&trigger_row);
        content.append(&debounce_row);
        content.append(&max_tokens_row);
        content.append(&context_before_row);
        content.append(&context_after_row);
        content.append(&max_lines_row);
        content.append(&temperature_row);

        let scrolled = gtk4::ScrolledWindow::builder()
            .child(&content)
            .vexpand(true)
            .hexpand(true)
            .build();

        CompletionPageWidgets {
            scrolled,
            enabled_switch,
            endpoint_entry,
            api_key_entry,
            model_entry,
            trigger_dropdown,
            debounce_spin,
            max_tokens_spin,
            context_before_spin,
            context_after_spin,
            max_lines_spin,
            temperature_spin,
        }
    }

    // ── Agent page ─────────────────────────────────────────────────

    fn build_agent_page(
        settings: &EditorSettings,
        dialog_window: &gtk4::Window,
        on_open_file: &Rc<impl Fn(std::path::PathBuf) + 'static>,
    ) -> AgentPageWidgets {
        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        content.set_margin_top(16);
        content.set_margin_bottom(16);
        content.set_margin_start(16);
        content.set_margin_end(16);

        // Endpoint URL
        let endpoint_row = Self::make_row("Endpoint URL");
        let endpoint_entry = gtk4::Entry::builder()
            .text(&settings.agent_endpoint_url)
            .hexpand(true)
            .build();
        endpoint_row.append(&endpoint_entry);

        // API Key
        let api_key_row = Self::make_row("API Key");
        let api_key_entry = gtk4::PasswordEntry::builder()
            .show_peek_icon(true)
            .hexpand(true)
            .build();
        api_key_entry.set_text(&settings.agent_api_key);
        api_key_row.append(&api_key_entry);

        // Model
        let model_row = Self::make_row("Model");
        let model_entry = gtk4::Entry::builder()
            .text(&settings.agent_model)
            .placeholder_text("e.g. qwen-2.5, deepseek-r1")
            .hexpand(true)
            .build();
        model_row.append(&model_entry);

        // Max Tokens
        let max_tokens_row = Self::make_row("Max Tokens");
        let max_tokens_spin = gtk4::SpinButton::with_range(256.0, 32768.0, 256.0);
        max_tokens_spin.set_value(settings.agent_max_tokens as f64);
        max_tokens_row.append(&max_tokens_spin);

        // Temperature
        let temperature_row = Self::make_row("Temperature");
        let temperature_spin = gtk4::SpinButton::with_range(0.0, 2.0, 0.1);
        temperature_spin.set_digits(1);
        temperature_spin.set_value(settings.agent_temperature);
        temperature_row.append(&temperature_spin);

        // Context Length
        let context_row = Self::make_row("Context Length (tokens)");
        let context_length_spin = gtk4::SpinButton::with_range(4096.0, 1_048_576.0, 4096.0);
        context_length_spin.set_value(settings.agent_context_length as f64);
        context_row.append(&context_length_spin);

        // Command Timeout
        let timeout_row = Self::make_row("Command Timeout (seconds)");
        let command_timeout_spin = gtk4::SpinButton::with_range(5.0, 600.0, 5.0);
        command_timeout_spin.set_value(settings.agent_command_timeout_secs as f64);
        timeout_row.append(&command_timeout_spin);

        // Max Turns
        let max_turns_row = Self::make_row("Max Tool-Use Turns");
        let max_turns_spin = gtk4::SpinButton::with_range(1.0, 500.0, 1.0);
        max_turns_spin.set_value(settings.agent_max_turns as f64);
        max_turns_row.append(&max_turns_spin);

        // ── System Prompt ──
        let prompt_row = Self::make_row("System Prompt");
        let edit_prompt_btn = gtk4::Button::with_label("Edit");
        edit_prompt_btn.set_tooltip_text(Some("Edit the custom agent system prompt"));
        {
            let on_open = on_open_file.clone();
            let win = dialog_window.clone();
            edit_prompt_btn.connect_clicked(move |_| {
                match rline_config::paths::system_prompt_path() {
                    Ok(path) => {
                        // Create the file with the default prompt if it doesn't exist.
                        if !path.exists() {
                            if let Some(parent) = path.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            let _ = std::fs::write(&path, DEFAULT_SYSTEM_PROMPT);
                        }
                        on_open(path);
                        win.close();
                    }
                    Err(e) => {
                        tracing::error!("failed to resolve system prompt path: {e}");
                    }
                }
            });
        }
        prompt_row.append(&edit_prompt_btn);

        // ── Auto-approve section ──
        let approve_header = gtk4::Label::new(None);
        approve_header.set_markup("<b>Auto-approve Permissions (Act mode only)</b>");
        approve_header.set_halign(gtk4::Align::Start);
        approve_header.set_margin_top(8);

        // Auto-approve read
        let approve_read_row = Self::make_row("Read files");
        let auto_approve_read_switch = gtk4::Switch::new();
        auto_approve_read_switch.set_active(settings.agent_auto_approve_read);
        auto_approve_read_switch.set_valign(gtk4::Align::Center);
        approve_read_row.append(&auto_approve_read_switch);

        // Auto-approve edit
        let approve_edit_row = Self::make_row("Edit files");
        let auto_approve_edit_switch = gtk4::Switch::new();
        auto_approve_edit_switch.set_active(settings.agent_auto_approve_edit);
        auto_approve_edit_switch.set_valign(gtk4::Align::Center);
        approve_edit_row.append(&auto_approve_edit_switch);

        // Auto-approve command
        let approve_command_row = Self::make_row("Execute safe commands");
        let auto_approve_command_switch = gtk4::Switch::new();
        auto_approve_command_switch.set_active(settings.agent_auto_approve_command);
        auto_approve_command_switch.set_valign(gtk4::Align::Center);
        approve_command_row.append(&auto_approve_command_switch);

        // YOLO mode
        let yolo_row = Self::make_row("YOLO mode (skip system command approval)");
        let yolo_mode_switch = gtk4::Switch::new();
        yolo_mode_switch.set_active(settings.agent_yolo_mode);
        yolo_mode_switch.set_valign(gtk4::Align::Center);
        yolo_mode_switch.set_tooltip_text(Some(
            "When enabled, commands that affect the system outside the project \
             (apt, sudo, global installs, etc.) will not require approval.",
        ));
        yolo_row.append(&yolo_mode_switch);

        content.append(&endpoint_row);
        content.append(&api_key_row);
        content.append(&model_row);
        content.append(&max_tokens_row);
        content.append(&temperature_row);
        content.append(&context_row);
        content.append(&timeout_row);
        content.append(&max_turns_row);
        content.append(&prompt_row);
        content.append(&approve_header);
        content.append(&approve_read_row);
        content.append(&approve_edit_row);
        content.append(&approve_command_row);
        content.append(&yolo_row);

        let scrolled = gtk4::ScrolledWindow::builder()
            .child(&content)
            .vexpand(true)
            .hexpand(true)
            .build();

        AgentPageWidgets {
            scrolled,
            endpoint_entry,
            api_key_entry,
            model_entry,
            max_tokens_spin,
            temperature_spin,
            context_length_spin,
            command_timeout_spin,
            max_turns_spin,
            auto_approve_read_switch,
            auto_approve_edit_switch,
            auto_approve_command_switch,
            yolo_mode_switch,
        }
    }

    // ── Helpers ───────────────────────────────────────────────────

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

    /// Show a dialog to import a Zed theme.
    fn show_zed_import_dialog(parent: &gtk4::Window, theme_dropdown: &gtk4::DropDown) {
        use rline_config::zed_import;

        let themes = zed_import::discover_zed_themes();
        if themes.is_empty() {
            let alert = gtk4::AlertDialog::builder()
                .message("No Zed Themes Found")
                .detail("No Zed installation with themes was found on this system.\n\nChecked: ~/.local/share/zed/extensions/installed, ~/.config/zed/themes")
                .build();
            alert.show(Some(parent));
            return;
        }

        let dialog = gtk4::Window::builder()
            .title("Import Zed Theme")
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

            let detail = format!("{} ({} theme)", theme.source, theme.appearance);
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

                match Self::import_zed_theme(entry, &theme_dropdown) {
                    Ok(scheme_id) => {
                        tracing::info!("imported Zed theme as: {scheme_id}");
                        dialog.close();
                    }
                    Err(e) => {
                        tracing::error!("failed to import Zed theme: {e}");
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

    /// Import a single Zed theme: convert, install, and update the dropdown.
    fn import_zed_theme(
        entry: &rline_config::zed_import::ZedThemeEntry,
        theme_dropdown: &gtk4::DropDown,
    ) -> Result<String, rline_config::ConfigError> {
        use rline_config::zed_import;
        use sourceview5::prelude::*;

        let scheme_id = zed_import::import_zed_theme(entry)?;

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

// ── Widget holders for each notebook page ─────────────────────────

/// Holds references to editor-page widgets for reading values at apply time.
#[derive(Clone)]
struct EditorPageWidgets {
    scrolled: gtk4::ScrolledWindow,
    theme_dropdown: gtk4::DropDown,
    editor_font_dropdown: gtk4::DropDown,
    mono_fonts: Vec<String>,
    font_spin: gtk4::SpinButton,
    letter_spacing_spin: gtk4::SpinButton,
    line_height_spin: gtk4::SpinButton,
    hint_dropdown: gtk4::DropDown,
    tab_width_spin: gtk4::SpinButton,
    insert_spaces_switch: gtk4::Switch,
    term_font_dropdown: gtk4::DropDown,
    term_font_spin: gtk4::SpinButton,
    last_project_switch: gtk4::Switch,
    expand_spin: gtk4::SpinButton,
    cycle_spin: gtk4::SpinButton,
    treesitter_switch: gtk4::Switch,
}

impl EditorPageWidgets {
    fn read_theme(&self) -> String {
        self.theme_dropdown
            .selected_item()
            .and_then(|obj| obj.downcast::<gtk4::StringObject>().ok())
            .map(|so| so.string().to_string())
            .unwrap_or_else(|| "Adwaita-dark".to_owned())
    }

    fn read_editor_font(&self) -> String {
        let idx = self.editor_font_dropdown.selected() as usize;
        self.mono_fonts
            .get(idx)
            .cloned()
            .unwrap_or_else(|| "Monospace".to_owned())
    }

    fn read_terminal_font(&self) -> String {
        let idx = self.term_font_dropdown.selected() as usize;
        self.mono_fonts
            .get(idx)
            .cloned()
            .unwrap_or_else(|| "Monospace".to_owned())
    }

    fn read_hint_style(&self) -> String {
        self.hint_dropdown
            .selected_item()
            .and_then(|obj| obj.downcast::<gtk4::StringObject>().ok())
            .map(|so| so.string().to_string())
            .unwrap_or_else(|| "full".to_owned())
    }
}

/// Holds references to completion-page widgets for reading values at apply time.
#[derive(Clone)]
struct CompletionPageWidgets {
    scrolled: gtk4::ScrolledWindow,
    enabled_switch: gtk4::Switch,
    endpoint_entry: gtk4::Entry,
    api_key_entry: gtk4::PasswordEntry,
    model_entry: gtk4::Entry,
    trigger_dropdown: gtk4::DropDown,
    debounce_spin: gtk4::SpinButton,
    max_tokens_spin: gtk4::SpinButton,
    context_before_spin: gtk4::SpinButton,
    context_after_spin: gtk4::SpinButton,
    max_lines_spin: gtk4::SpinButton,
    temperature_spin: gtk4::SpinButton,
}

impl CompletionPageWidgets {
    fn read_trigger_mode(&self) -> String {
        match self.trigger_dropdown.selected() {
            1 => "manual".to_owned(),
            2 => "both".to_owned(),
            _ => "automatic".to_owned(),
        }
    }
}

/// Holds references to agent-page widgets for reading values at apply time.
#[derive(Clone)]
struct AgentPageWidgets {
    scrolled: gtk4::ScrolledWindow,
    endpoint_entry: gtk4::Entry,
    api_key_entry: gtk4::PasswordEntry,
    model_entry: gtk4::Entry,
    max_tokens_spin: gtk4::SpinButton,
    temperature_spin: gtk4::SpinButton,
    context_length_spin: gtk4::SpinButton,
    command_timeout_spin: gtk4::SpinButton,
    max_turns_spin: gtk4::SpinButton,
    auto_approve_read_switch: gtk4::Switch,
    auto_approve_edit_switch: gtk4::Switch,
    auto_approve_command_switch: gtk4::Switch,
    yolo_mode_switch: gtk4::Switch,
}
