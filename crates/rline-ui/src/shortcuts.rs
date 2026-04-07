//! Keyboard shortcut registration for the application.

use gtk4::prelude::*;

/// Register all keyboard accelerators on the application.
pub fn register_accels(app: &gtk4::Application) {
    app.set_accels_for_action("win.open-file", &["<Ctrl>O"]);
    app.set_accels_for_action("win.save-file", &["<Ctrl>S"]);
    app.set_accels_for_action("win.close-tab", &["<Ctrl>W"]);
    app.set_accels_for_action("win.quick-open", &["<Ctrl>P"]);
    app.set_accels_for_action("win.project-search", &["<Ctrl><Shift>F"]);
    app.set_accels_for_action("win.quit-app", &["<Ctrl>Q"]);
    app.set_accels_for_action("win.show-git", &["<Ctrl><Shift>G"]);
    app.set_accels_for_action("win.show-files", &["<Ctrl><Shift>E"]);
    app.set_accels_for_action("win.focus-terminal", &["<Ctrl><Shift>W"]);
    app.set_accels_for_action("win.find", &["<Ctrl>F"]);
    app.set_accels_for_action("win.find-replace", &["<Ctrl>H"]);
}
