//! QuickOpenDialog — Ctrl+P fuzzy file finder as a top-of-screen popup.
//!
//! Presents a search entry at the top of the window with a dropdown list of
//! matching files. Supports keyboard navigation (arrow keys + Enter) and
//! single-click selection.

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

/// A top-of-screen popup for finding files by name with keyboard navigation.
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
    /// Create a new quick-open popup anchored at the top of the parent window.
    pub fn new(parent: &gtk4::Window, project_root: &Path) -> Self {
        let window = gtk4::Window::builder()
            .modal(true)
            .transient_for(parent)
            .decorated(false)
            .default_width(600)
            .default_height(350)
            .build();

        // Position at top center of parent
        window.set_valign(gtk4::Align::Start);

        let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

        let search_entry = gtk4::SearchEntry::new();
        search_entry.set_placeholder_text(Some("Search files by name..."));
        search_entry.set_hexpand(true);
        search_entry.add_css_class("quick-open-entry");
        vbox.append(&search_entry);

        let store = gio::ListStore::new::<FileEntryObject>();
        let selection = gtk4::SingleSelection::new(Some(store.clone()));
        selection.set_autoselect(true);

        let factory = gtk4::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            if let Some(list_item) = item.downcast_ref::<gtk4::ListItem>() {
                let label = gtk4::Label::new(None);
                label.set_halign(gtk4::Align::Start);
                label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
                label.set_margin_top(4);
                label.set_margin_bottom(4);
                label.set_margin_start(8);
                label.set_margin_end(8);
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

        let list_view = gtk4::ListView::new(Some(selection.clone()), Some(factory));
        list_view.set_vexpand(true);

        let scrolled = gtk4::ScrolledWindow::builder()
            .child(&list_view)
            .vexpand(true)
            .build();
        vbox.append(&scrolled);

        window.set_child(Some(&vbox));

        // Collect file paths
        let root = project_root.to_path_buf();
        let all_files: Arc<Vec<PathBuf>> = Arc::new(search_worker::collect_file_paths(&root));

        // Callback type alias would obscure the signature for these one-off event handlers
        #[allow(clippy::type_complexity)]
        let on_file_selected: Rc<RefCell<Option<Box<dyn Fn(&Path)>>>> = Rc::new(RefCell::new(None));

        // ── Filter as user types ──
        let files_ref = all_files.clone();
        let root_ref = root.clone();
        search_entry.connect_search_changed(glib::clone!(
            #[weak(rename_to = store_ref)]
            store,
            move |entry| {
                let query = entry.text().to_string().to_lowercase();
                store_ref.remove_all();

                if query.is_empty() {
                    return;
                }

                // Score and collect matches
                let mut scored: Vec<(usize, &PathBuf)> = files_ref
                    .iter()
                    .filter_map(|path| {
                        let name = path
                            .file_name()
                            .map(|f| f.to_string_lossy().to_lowercase())
                            .unwrap_or_default();
                        if subsequence_match(&name, &query) {
                            Some((match_score(&name, &query), path))
                        } else {
                            None
                        }
                    })
                    .collect();

                // Sort by score (lower = better match)
                scored.sort_by_key(|(score, _)| *score);

                for (_, path) in scored.iter().take(50) {
                    store_ref.append(&FileEntryObject::new(path, &root_ref));
                }
            }
        ));

        // ── Single-click selects and closes ──
        let cb_click = on_file_selected.clone();
        let store_click = store.clone();
        let lv_click = list_view.clone();
        let click_gesture = gtk4::GestureClick::new();
        click_gesture.set_button(1);
        let win_click = window.clone();
        click_gesture.connect_released(move |_, _, _, _| {
            if let Some((path, _)) = get_selected_file(&lv_click, &store_click) {
                if let Some(ref cb) = *cb_click.borrow() {
                    cb(&path);
                }
                win_click.close();
            }
        });
        list_view.add_controller(click_gesture);

        // ── Keyboard navigation: arrows, Enter, Escape ──
        // Use capture phase so we intercept keys before the search entry consumes them
        let key_controller = gtk4::EventControllerKey::new();
        key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let cb_key = on_file_selected.clone();
        let selection_key = selection.clone();
        let store_key = store.clone();
        key_controller.connect_key_pressed(glib::clone!(
            #[weak]
            window,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                match key {
                    gtk4::gdk::Key::Escape => {
                        window.close();
                        glib::Propagation::Stop
                    }
                    gtk4::gdk::Key::Return | gtk4::gdk::Key::KP_Enter => {
                        let pos = selection_key.selected();
                        if let Some(item) = store_key.item(pos) {
                            if let Some(obj) = item.downcast_ref::<FileEntryObject>() {
                                let path = PathBuf::from(obj.file_path());
                                if let Some(ref cb) = *cb_key.borrow() {
                                    cb(&path);
                                }
                                window.close();
                            }
                        }
                        glib::Propagation::Stop
                    }
                    gtk4::gdk::Key::Down => {
                        let current = selection_key.selected();
                        let n_items = store_key.n_items();
                        if n_items > 0 && current + 1 < n_items {
                            selection_key.set_selected(current + 1);
                        }
                        glib::Propagation::Stop
                    }
                    gtk4::gdk::Key::Up => {
                        let current = selection_key.selected();
                        if current > 0 {
                            selection_key.set_selected(current - 1);
                        }
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            }
        ));
        window.add_controller(key_controller);

        Self {
            window,
            on_file_selected,
        }
    }

    /// Set the callback invoked when a file is selected.
    pub fn set_on_file_selected<F: Fn(&Path) + 'static>(&self, f: F) {
        self.on_file_selected.replace(Some(Box::new(f)));
    }

    /// Present the popup.
    pub fn present(&self) {
        self.window.present();
    }
}

/// Get the currently selected file from the list view.
fn get_selected_file(
    list_view: &gtk4::ListView,
    store: &gio::ListStore,
) -> Option<(PathBuf, String)> {
    let model = list_view.model()?;
    let selection = model.downcast_ref::<gtk4::SingleSelection>()?;
    let pos = selection.selected();
    let item = store.item(pos)?;
    let obj = item.downcast_ref::<FileEntryObject>()?;
    Some((PathBuf::from(obj.file_path()), obj.display_name()))
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

/// Score a match for relevance ranking. Lower score = better match.
///
/// Scoring heuristics:
/// - Exact filename match gets best score (0)
/// - Prefix match scores better than mid-string match
/// - Fewer gaps between matched characters scores better
/// - Shorter filenames score better (less noise)
fn match_score(name: &str, query: &str) -> usize {
    // Exact match
    if name == query {
        return 0;
    }

    // Prefix match bonus
    let prefix_bonus = if name.starts_with(query) { 0 } else { 100 };

    // Count gaps between matched characters
    let mut gaps = 0;
    let mut last_match_pos: Option<usize> = None;
    let mut name_iter = name.char_indices();

    for qc in query.chars() {
        for (pos, nc) in name_iter.by_ref() {
            if nc == qc {
                if let Some(last) = last_match_pos {
                    gaps += pos - last - 1;
                }
                last_match_pos = Some(pos);
                break;
            }
        }
    }

    prefix_bonus + gaps * 10 + name.len()
}
