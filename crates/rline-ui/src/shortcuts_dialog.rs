//! Keyboard shortcuts dialog with interactive rebinding.
//!
//! Each shortcut is shown as a row with its description and a button
//! displaying the current key combo. Clicking the button enters capture
//! mode — the next key combination pressed is recorded as the new
//! binding.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gdk;
use gtk4::prelude::*;

use rline_config::{EditorSettings, KeyBindings, SHORTCUT_DESCRIPTORS};

/// Shared mutable state for the dialog: the keybindings being edited
/// and a flag indicating which action (if any) is in capture mode.
struct DialogState {
    bindings: KeyBindings,
    /// The action name currently being rebound, or `None`.
    capturing: Option<String>,
}

/// Build and return the keyboard shortcuts dialog.
///
/// When the user closes the dialog, any changes are saved to settings
/// and the application accelerators are re-registered.
pub fn build_shortcuts_dialog(
    parent: &impl IsA<gtk4::Window>,
    settings: &EditorSettings,
    on_changed: impl Fn(&KeyBindings) + 'static,
) -> gtk4::Window {
    let state = Rc::new(RefCell::new(DialogState {
        bindings: settings.keybindings.clone(),
        capturing: None,
    }));

    let dialog = gtk4::Window::builder()
        .title("Keyboard Shortcuts")
        .modal(true)
        .transient_for(parent)
        .default_width(480)
        .default_height(520)
        .resizable(false)
        .build();

    let grid = gtk4::Grid::builder()
        .row_spacing(4)
        .column_spacing(16)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(24)
        .margin_end(24)
        .build();

    // Collect buttons so we can reference them in the key controller.
    let buttons: Rc<Vec<(String, gtk4::Button)>> = Rc::new(
        SHORTCUT_DESCRIPTORS
            .iter()
            .enumerate()
            .map(|(i, desc)| {
                let row = i as i32;

                let desc_label = gtk4::Label::builder()
                    .label(desc.label)
                    .halign(gtk4::Align::Start)
                    .hexpand(true)
                    .build();

                let accel = state
                    .borrow()
                    .bindings
                    .accel_for_action(desc.action)
                    .unwrap_or("")
                    .to_owned();
                let label_text = KeyBindings::accel_to_label(&accel);

                let btn = gtk4::Button::builder()
                    .label(&label_text)
                    .halign(gtk4::Align::End)
                    .width_request(180)
                    .build();
                btn.add_css_class("flat");
                btn.add_css_class("monospace");

                // Click → enter capture mode for this action.
                let action_name = desc.action.to_owned();
                let state_ref = Rc::clone(&state);
                let btn_ref = btn.clone();
                btn.connect_clicked(move |_| {
                    let mut st = state_ref.borrow_mut();
                    st.capturing = Some(action_name.clone());
                    btn_ref.set_label("Press a key…");
                    btn_ref.add_css_class("suggested-action");
                });

                grid.attach(&desc_label, 0, row, 1, 1);
                grid.attach(&btn, 1, row, 1, 1);

                (desc.action.to_owned(), btn)
            })
            .collect(),
    );

    // Add a "Reset to Defaults" button at the bottom.
    let reset_btn = gtk4::Button::builder()
        .label("Reset to Defaults")
        .halign(gtk4::Align::Center)
        .margin_top(12)
        .build();
    reset_btn.add_css_class("destructive-action");

    let bottom_box = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(0)
        .build();
    bottom_box.append(&grid);
    bottom_box.append(&reset_btn);

    let scrolled = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .child(&bottom_box)
        .vexpand(true)
        .build();

    dialog.set_child(Some(&scrolled));

    // Reset handler.
    {
        let state_ref = Rc::clone(&state);
        let buttons_ref = Rc::clone(&buttons);
        reset_btn.connect_clicked(move |_| {
            let defaults = KeyBindings::default();
            for (action, btn) in buttons_ref.iter() {
                if let Some(accel) = defaults.accel_for_action(action) {
                    let label = KeyBindings::accel_to_label(accel);
                    btn.set_label(&label);
                    btn.remove_css_class("suggested-action");
                }
            }
            let mut st = state_ref.borrow_mut();
            st.bindings = defaults;
            st.capturing = None;
        });
    }

    // Key controller in capture phase to intercept keypresses during rebind.
    let key_ctl = gtk4::EventControllerKey::new();
    key_ctl.set_propagation_phase(gtk4::PropagationPhase::Capture);

    {
        let state_ref = Rc::clone(&state);
        let buttons_ref = Rc::clone(&buttons);
        key_ctl.connect_key_pressed(move |_, keyval, _keycode, modifiers| {
            let mut st = state_ref.borrow_mut();

            // Escape cancels capture mode without changing the binding.
            if keyval == gdk::Key::Escape {
                if let Some(action) = st.capturing.take() {
                    // Restore the current label.
                    if let Some((_, btn)) = buttons_ref.iter().find(|(a, _)| *a == action) {
                        let accel = st.bindings.accel_for_action(&action).unwrap_or("");
                        btn.set_label(&KeyBindings::accel_to_label(accel));
                        btn.remove_css_class("suggested-action");
                    }
                }
                return gtk4::glib::Propagation::Stop;
            }

            let Some(ref action) = st.capturing else {
                return gtk4::glib::Propagation::Proceed;
            };
            let action = action.clone();

            // Ignore bare modifier keys (Shift, Ctrl, etc.) — wait for a
            // real key combined with modifiers.
            if is_modifier_key(keyval) {
                return gtk4::glib::Propagation::Stop;
            }

            // Build GTK accelerator string.
            let accel = build_accel_string(keyval, modifiers);
            st.bindings.set_accel_for_action(&action, &accel);

            // Update button label.
            if let Some((_, btn)) = buttons_ref.iter().find(|(a, _)| *a == action) {
                btn.set_label(&KeyBindings::accel_to_label(&accel));
                btn.remove_css_class("suggested-action");
            }

            st.capturing = None;
            gtk4::glib::Propagation::Stop
        });
    }

    dialog.add_controller(key_ctl);

    // On close, save the bindings and notify the caller.
    {
        let state_ref = Rc::clone(&state);
        let on_changed = Rc::new(on_changed);
        dialog.connect_close_request(move |_| {
            let st = state_ref.borrow();
            on_changed(&st.bindings);
            gtk4::glib::Propagation::Proceed
        });
    }

    dialog
}

/// Returns `true` if the given key is a bare modifier (Shift, Ctrl, Alt, Super).
fn is_modifier_key(keyval: gdk::Key) -> bool {
    matches!(
        keyval,
        gdk::Key::Shift_L
            | gdk::Key::Shift_R
            | gdk::Key::Control_L
            | gdk::Key::Control_R
            | gdk::Key::Alt_L
            | gdk::Key::Alt_R
            | gdk::Key::Super_L
            | gdk::Key::Super_R
            | gdk::Key::Meta_L
            | gdk::Key::Meta_R
            | gdk::Key::Hyper_L
            | gdk::Key::Hyper_R
            | gdk::Key::ISO_Level3_Shift
            | gdk::Key::Caps_Lock
            | gdk::Key::Num_Lock
    )
}

/// Build a GTK accelerator string from a keyval and modifier mask.
///
/// For example, `(Key::f, CONTROL_MASK | SHIFT_MASK)` → `"<Ctrl><Shift>F"`.
fn build_accel_string(keyval: gdk::Key, modifiers: gdk::ModifierType) -> String {
    let mut accel = String::new();

    if modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
        accel.push_str("<Ctrl>");
    }
    if modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
        accel.push_str("<Shift>");
    }
    if modifiers.contains(gdk::ModifierType::ALT_MASK) {
        accel.push_str("<Alt>");
    }
    if modifiers.contains(gdk::ModifierType::SUPER_MASK) {
        accel.push_str("<Super>");
    }

    // Convert keyval to its canonical name.
    let key_name = keyval.name().map(|n| n.to_string()).unwrap_or_default();
    accel.push_str(&key_name);

    accel
}
