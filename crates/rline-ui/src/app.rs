//! RlineApplication — GTK Application subclass and entry point.

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;

use crate::shortcuts;
use crate::window::RlineWindow;

// ── Implementation ──────────────────────────────────────────────

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct RlineApplication;

    #[glib::object_subclass]
    impl ObjectSubclass for RlineApplication {
        const NAME: &'static str = "RlineApplication";
        type Type = super::RlineApplication;
        type ParentType = gtk4::Application;
    }

    impl ObjectImpl for RlineApplication {}

    impl ApplicationImpl for RlineApplication {
        fn activate(&self) {
            let app = self.obj();
            shortcuts::register_accels(app.upcast_ref());

            let window = RlineWindow::new(app.upcast_ref());
            window.present();
        }
    }

    impl GtkApplicationImpl for RlineApplication {}
}

// ── Public type ─────────────────────────────────────────────────

glib::wrapper! {
    /// The top-level GTK application for rline.
    pub struct RlineApplication(ObjectSubclass<imp::RlineApplication>)
        @extends gio::Application, gtk4::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl RlineApplication {
    /// Create a new rline application instance.
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", "dev.rline.editor")
            .build()
    }
}

impl Default for RlineApplication {
    fn default() -> Self {
        Self::new()
    }
}
