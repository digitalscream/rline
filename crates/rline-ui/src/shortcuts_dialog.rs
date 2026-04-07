//! Dialog listing all available keyboard shortcuts.

use gtk4::prelude::*;

/// Shortcut entry: human-readable key combo and description.
struct ShortcutEntry {
    keys: &'static str,
    description: &'static str,
}

/// All registered keyboard shortcuts.
const SHORTCUTS: &[ShortcutEntry] = &[
    ShortcutEntry {
        keys: "Ctrl+O",
        description: "Open file",
    },
    ShortcutEntry {
        keys: "Ctrl+S",
        description: "Save file",
    },
    ShortcutEntry {
        keys: "Ctrl+W",
        description: "Close current editor tab",
    },
    ShortcutEntry {
        keys: "Ctrl+P",
        description: "Quick open (fuzzy file finder)",
    },
    ShortcutEntry {
        keys: "Ctrl+F",
        description: "Find in current file",
    },
    ShortcutEntry {
        keys: "Ctrl+H",
        description: "Find and replace in current file",
    },
    ShortcutEntry {
        keys: "Ctrl+\\",
        description: "Split editor vertically",
    },
    ShortcutEntry {
        keys: "Ctrl+Tab",
        description: "Cycle MRU tabs",
    },
    ShortcutEntry {
        keys: "Ctrl+Shift+F",
        description: "Focus project search",
    },
    ShortcutEntry {
        keys: "Ctrl+Shift+G",
        description: "Show git panel",
    },
    ShortcutEntry {
        keys: "Ctrl+Shift+E",
        description: "Show files panel",
    },
    ShortcutEntry {
        keys: "Ctrl+Shift+W",
        description: "Focus terminal",
    },
    ShortcutEntry {
        keys: "Ctrl+Q",
        description: "Quit application",
    },
];

/// Build and return the keyboard shortcuts dialog.
pub fn build_shortcuts_dialog(parent: &impl IsA<gtk4::Window>) -> gtk4::Window {
    let dialog = gtk4::Window::builder()
        .title("Keyboard Shortcuts")
        .modal(true)
        .transient_for(parent)
        .default_width(400)
        .default_height(450)
        .resizable(false)
        .build();

    let grid = gtk4::Grid::builder()
        .row_spacing(6)
        .column_spacing(24)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(24)
        .margin_end(24)
        .build();

    for (i, entry) in SHORTCUTS.iter().enumerate() {
        let row = i as i32;

        let desc_label = gtk4::Label::builder()
            .label(entry.description)
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .build();

        let keys_label = gtk4::Label::builder()
            .label(entry.keys)
            .halign(gtk4::Align::End)
            .build();
        keys_label.add_css_class("dim-label");
        keys_label.add_css_class("monospace");

        grid.attach(&desc_label, 0, row, 1, 1);
        grid.attach(&keys_label, 1, row, 1, 1);
    }

    let scrolled = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .child(&grid)
        .vexpand(true)
        .build();

    dialog.set_child(Some(&scrolled));
    dialog
}
