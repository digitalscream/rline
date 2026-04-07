//! TerminalTab — a single VTE terminal instance.

use std::path::Path;

use gtk4::prelude::*;
use vte4::prelude::*;

/// A single terminal tab backed by a VTE terminal widget.
#[derive(Debug, Clone)]
pub struct TerminalTab {
    terminal: vte4::Terminal,
    label: gtk4::Label,
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

        // Apply font
        Self::apply_font(&terminal, font_family, font_size);

        let label = gtk4::Label::new(Some(&format!("Terminal {index}")));

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

        Self { terminal, label }
    }

    /// The terminal widget.
    pub fn widget(&self) -> &vte4::Terminal {
        &self.terminal
    }

    /// The tab label widget.
    pub fn tab_label(&self) -> &gtk4::Label {
        &self.label
    }

    /// Apply font family and size to the terminal widget.
    fn apply_font(terminal: &vte4::Terminal, font_family: &str, font_size: u32) {
        let font_desc =
            gtk4::pango::FontDescription::from_string(&format!("{font_family} {font_size}"));
        terminal.set_font(Some(&font_desc));
    }
}
