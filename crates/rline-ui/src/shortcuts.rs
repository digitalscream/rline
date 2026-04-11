//! Keyboard shortcut registration for the application.

use gtk4::prelude::*;
use rline_config::{KeyBindings, SHORTCUT_DESCRIPTORS};

/// Register all keyboard accelerators on the application using the
/// given keybindings configuration.
pub fn register_accels(app: &gtk4::Application, bindings: &KeyBindings) {
    for desc in SHORTCUT_DESCRIPTORS {
        if let Some(accel) = bindings.accel_for_action(desc.action) {
            if !accel.is_empty() {
                app.set_accels_for_action(desc.action, &[accel]);
            } else {
                // Clear any previously registered accelerator.
                app.set_accels_for_action(desc.action, &[] as &[&str]);
            }
        }
    }
}
