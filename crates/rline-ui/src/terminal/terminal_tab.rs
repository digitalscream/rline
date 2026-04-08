//! TerminalTab — a single VTE terminal instance.

use std::path::Path;

use gtk4::prelude::*;
use vte4::prelude::*;

use crate::theming::TerminalColors;

/// A single terminal tab backed by a VTE terminal widget.
#[derive(Debug, Clone)]
pub struct TerminalTab {
    terminal: vte4::Terminal,
    tab_label: gtk4::Box,
    close_btn: gtk4::Button,
}

impl TerminalTab {
    /// Create a new terminal tab and spawn a shell.
    pub fn new(
        index: usize,
        working_dir: Option<&Path>,
        font_family: &str,
        font_size: u32,
    ) -> Self {
        let terminal = vte4::Terminal::new();
        terminal.set_vexpand(true);
        terminal.set_hexpand(true);
        terminal.set_cursor_shape(vte4::CursorShape::Block);
        terminal.set_cursor_blink_mode(vte4::CursorBlinkMode::Off);

        // Apply font
        Self::apply_font(&terminal, font_family, font_size);

        let name_label = gtk4::Label::new(Some(&format!("Terminal {index}")));
        let close_btn = gtk4::Button::from_icon_name("window-close-symbolic");
        close_btn.add_css_class("flat");
        close_btn.add_css_class("circular");
        close_btn.set_valign(gtk4::Align::Center);
        close_btn.set_has_frame(false);
        close_btn.set_margin_start(2);

        let tab_label = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        tab_label.append(&name_label);
        tab_label.append(&close_btn);

        // Determine working directory
        let cwd = working_dir
            .map(|p| p.to_string_lossy().to_string())
            .or_else(|| std::env::var("HOME").ok())
            .unwrap_or_else(|| "/".to_string());

        // Determine shell
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

        // Spawn the shell
        terminal.spawn_async(
            vte4::PtyFlags::DEFAULT,
            Some(cwd.as_str()),
            &[shell.as_str()],
            &[],
            glib::SpawnFlags::DEFAULT,
            || {}, // child setup
            -1,    // timeout (-1 = default)
            gio::Cancellable::NONE,
            |result| {
                // callback when spawn completes
                if let Err(e) = result {
                    tracing::error!("failed to spawn terminal shell: {e}");
                }
            },
        );

        Self {
            terminal,
            tab_label,
            close_btn,
        }
    }

    /// The terminal widget.
    pub fn widget(&self) -> &vte4::Terminal {
        &self.terminal
    }

    /// The tab label widget.
    pub fn tab_label(&self) -> &gtk4::Box {
        &self.tab_label
    }

    /// The close button in the tab label.
    pub fn close_btn(&self) -> &gtk4::Button {
        &self.close_btn
    }

    /// Apply theme colors (background, foreground, cursor, selection, bold) to the terminal.
    pub fn apply_theme(&self, colors: &TerminalColors) {
        Self::apply_theme_to_terminal(&self.terminal, colors);
    }

    /// Apply theme colors to a VTE terminal widget.
    pub fn apply_theme_to_terminal(terminal: &vte4::Terminal, colors: &TerminalColors) {
        terminal.set_color_background(&colors.background);
        terminal.set_color_foreground(&colors.foreground);
        // Reset cursor colors to None so VTE uses its default inverted fg/bg,
        // which guarantees the cursor is always visible.
        terminal.set_color_cursor(None::<&gtk4::gdk::RGBA>);
        terminal.set_color_cursor_foreground(None::<&gtk4::gdk::RGBA>);
        terminal.set_color_highlight(Some(&colors.highlight));
        terminal.set_color_highlight_foreground(Some(&colors.highlight_foreground));
        terminal.set_color_bold(Some(&colors.bold));
    }

    /// Apply font family and size to the terminal widget.
    fn apply_font(terminal: &vte4::Terminal, font_family: &str, font_size: u32) {
        let font_desc =
            gtk4::pango::FontDescription::from_string(&format!("{font_family} {font_size}"));
        terminal.set_font(Some(&font_desc));
    }
}
