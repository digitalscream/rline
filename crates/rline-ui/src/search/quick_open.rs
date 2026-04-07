//! QuickOpenDialog — Ctrl+P fuzzy file finder.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gtk4::prelude::*;

use super::search_worker;

/// GObject for file entries in the quick-open list.
mod file_entry_object {
    use std::cell::RefCell;

    use glib::prelude::*;
    use glib::subclass::prelude::*;
    use glib::Properties;

    mod imp {
        use super::*;

        #[derive(Debug, Default, Properties)]
        #[properties(wrapper_type = super::FileEntryObject)]
        pub struct FileEntryObject {
            #[property(get, set)]
            file_path: RefCell<String>,
            #[property(get, set)]
            display_name: RefCell<String>,
        }

        #[glib::object_subclass]
        impl ObjectSubclass for FileEntryObject {
            const NAME: &'static str = "RlineFileEntry";
            type Type = super::FileEntryObject;
            type ParentType = glib::Object;
        }

        #[glib::derived_properties]
        impl ObjectImpl for FileEntryObject {}
    }

    glib::wrapper! {
        pub struct FileEntryObject(ObjectSubclass<imp::FileEntryObject>);
    }

    impl FileEntryObject {
        pub fn new(path: &std::path::Path, root: &std::path::Path) -> Self {
            let display = path
                .strip_prefix(root)
                .unwrap_or(path)
                .display()
                .to_string();

            glib::Object::builder()
                .property("file-path", path.display().to_string())
                .property("display-name", &display)
                .build()
        }
    }
}

use file_entry_object::FileEntryObject;

/// A modal quick-open dialog for finding files by name.
pub struct QuickOpenDialog {
    window: gtk4::Window,
    // Callback type alias would obscure the signature for these one-off event handlers
    #[allow(clippy::type_complexity)]
    on_file_selected: Rc<RefCell<Option<Box<dyn Fn(&Path)>>>>,
}

impl std::fmt::Debug for QuickOpenDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuickOpenDialog").finish_non_exhaustive()
    }
}

impl QuickOpenDialog {
    /// Create a new quick-open dialog.
    pub fn new(parent: &gtk4::Window, project_root: &Path) -> Self {
        let window = gtk4::Window::builder()
            .title("Open File")
            .modal(true)
            .transient_for(parent)
            .default_width(500)
            .default_height(400)
            .build();

        let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        vbox.set_margin_top(8);
        vbox.set_margin_bottom(8);
        vbox.set_margin_start(8);
        vbox.set_margin_end(8);

        let search_entry = gtk4::SearchEntry::new();
        search_entry.set_placeholder_text(Some("Type to search files..."));
        vbox.append(&search_entry);

        let store = gio::ListStore::new::<FileEntryObject>();
        let selection = gtk4::SingleSelection::new(Some(store.clone()));

        let factory = gtk4::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            if let Some(list_item) = item.downcast_ref::<gtk4::ListItem>() {
                let label = gtk4::Label::new(None);
                label.set_halign(gtk4::Align::Start);
                label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
                list_item.set_child(Some(&label));
            }
        });
        factory.connect_bind(|_, item| {
            if let Some(list_item) = item.downcast_ref::<gtk4::ListItem>() {
                if let Some(obj) = list_item.item().and_downcast::<FileEntryObject>() {
                    if let Some(label) = list_item.child().and_downcast::<gtk4::Label>() {
                        label.set_text(&obj.display_name());
                    }
                }
            }
        });

        let list_view = gtk4::ListView::new(Some(selection), Some(factory));
        list_view.set_vexpand(true);

        let scrolled = gtk4::ScrolledWindow::builder()
            .child(&list_view)
            .vexpand(true)
            .build();
        vbox.append(&scrolled);

        window.set_child(Some(&vbox));

        // Collect file paths in background
        let root = project_root.to_path_buf();
        let all_files: Arc<Vec<PathBuf>> = Arc::new(search_worker::collect_file_paths(&root));
        let root_for_filter = root.clone();

        // Callback type alias would obscure the signature for these one-off event handlers
        #[allow(clippy::type_complexity)]
        let on_file_selected: Rc<RefCell<Option<Box<dyn Fn(&Path)>>>> = Rc::new(RefCell::new(None));

        // Filter as user types
        let files_ref = all_files.clone();
        let root_ref = root_for_filter.clone();
        search_entry.connect_search_changed(glib::clone!(
            #[weak(rename_to = store_ref)]
            store,
            move |entry| {
                let query = entry.text().to_string().to_lowercase();
                store_ref.remove_all();

                if query.is_empty() {
                    return;
                }

                let mut count = 0;
                for path in files_ref.iter() {
                    if count >= 50 {
                        break; // Limit results for performance
                    }
                    let name = path
                        .file_name()
                        .map(|f| f.to_string_lossy().to_lowercase())
                        .unwrap_or_default();

                    if subsequence_match(&name, &query) {
                        store_ref.append(&FileEntryObject::new(path, &root_ref));
                        count += 1;
                    }
                }
            }
        ));

        // Open on activate (Enter or click)
        let cb_ref = on_file_selected.clone();
        list_view.connect_activate(glib::clone!(
            #[weak]
            window,
            #[weak]
            store,
            move |_, position| {
                if let Some(item) = store.item(position) {
                    if let Some(obj) = item.downcast_ref::<FileEntryObject>() {
                        let path = PathBuf::from(obj.file_path());
                        if let Some(ref cb) = *cb_ref.borrow() {
                            cb(&path);
                        }
                        window.close();
                    }
                }
            }
        ));

        // Escape to close
        let esc_controller = gtk4::EventControllerKey::new();
        esc_controller.connect_key_pressed(glib::clone!(
            #[weak]
            window,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                if key == gtk4::gdk::Key::Escape {
                    window.close();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        window.add_controller(esc_controller);

        Self {
            window,
            on_file_selected,
        }
    }

    /// Set the callback invoked when a file is selected.
    pub fn set_on_file_selected<F: Fn(&Path) + 'static>(&self, f: F) {
        self.on_file_selected.replace(Some(Box::new(f)));
    }

    /// Present the dialog.
    pub fn present(&self) {
        self.window.present();
    }
}

/// Simple subsequence matching: every character in `query` appears in `name`
/// in order, but not necessarily contiguously.
fn subsequence_match(name: &str, query: &str) -> bool {
    let mut name_chars = name.chars();
    for qc in query.chars() {
        loop {
            match name_chars.next() {
                Some(nc) if nc == qc => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}
