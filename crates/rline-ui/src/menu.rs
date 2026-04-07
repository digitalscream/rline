//! Hamburger menu construction for the main window header bar.

/// Build the application menu model with a "Settings" entry.
pub fn build_app_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("Keyboard Shortcuts"), Some("win.show-shortcuts"));
    menu.append(Some("Settings"), Some("win.show-settings"));
    menu
}
