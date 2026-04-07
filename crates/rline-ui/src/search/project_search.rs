//! ProjectSearchPanel — full-text search across project files with grouped results.
//!
//! Results are grouped by file. Each file row is expandable to show individual
//! line matches. Files with few matches are auto-expanded based on settings.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;

use rline_core::SearchResult;

use super::search_worker;

// ── GObject for a search result row (file header or line match) ─────────

mod search_row_object {
    use std::cell::RefCell;

    use glib::prelude::*;
    use glib::subclass::prelude::*;
    use glib::Properties;

    mod imp {
        use super::*;

        #[derive(Debug, Default, Properties)]
        #[properties(wrapper_type = super::SearchRowObject)]
        pub struct SearchRowObject {
            /// The absolute file path.
            #[property(get, set)]
            file_path: RefCell<String>,
            /// Display text shown in the row.
            #[property(get, set)]
            display_text: RefCell<String>,
            /// The line number (0-based), or u32::MAX for file header rows.
            #[property(get, set)]
            line_number: RefCell<u32>,
            /// Whether this row is a file header (true) or a line match (false).
            #[property(get, set, name = "is-header")]
            is_header: RefCell<bool>,
            /// Number of matches in this file (only meaningful for header rows).
            #[property(get, set)]
            match_count: RefCell<u32>,
        }

        #[glib::object_subclass]
        impl ObjectSubclass for SearchRowObject {
            const NAME: &'static str = "RlineSearchRow";
            type Type = super::SearchRowObject;
            type ParentType = glib::Object;
        }

        #[glib::derived_properties]
        impl ObjectImpl for SearchRowObject {}
    }

    glib::wrapper! {
        pub struct SearchRowObject(ObjectSubclass<imp::SearchRowObject>);
    }

    impl SearchRowObject {
        /// Create a file header row.
        pub fn new_header(path: &std::path::Path, match_count: u32) -> Self {
            let filename = path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            let suffix = if match_count == 1 { "" } else { "es" };
            let display = format!("{filename} ({match_count} match{suffix})");

            glib::Object::builder()
                .property("file-path", path.display().to_string())
                .property("display-text", &display)
                .property("line-number", u32::MAX)
                .property("is-header", true)
                .property("match-count", match_count)
                .build()
        }

        /// Create a line match row.
        pub fn new_match(result: &rline_core::SearchResult) -> Self {
            let display = format!(
                "  {}:  {}",
                result.line_number.0 + 1,
                result.line_text.trim()
            );

            glib::Object::builder()
                .property("file-path", result.path.display().to_string())
                .property("display-text", &display)
                .property("line-number", result.line_number.0 as u32)
                .property("is-header", false)
                .property("match-count", 0u32)
                .build()
        }
    }
}

use search_row_object::SearchRowObject;

/// The project search panel for the left sidebar.
#[derive(Clone)]
pub struct ProjectSearchPanel {
    container: gtk4::Box,
    search_entry: gtk4::SearchEntry,
    results_store: gio::ListStore,
    project_root: Rc<RefCell<Option<PathBuf>>>,
    // Callback type alias would obscure the signature for these one-off event handlers
    #[allow(clippy::type_complexity)]
    on_open_file_at_line: Rc<RefCell<Option<Box<dyn Fn(&Path, rline_core::LineIndex)>>>>,
    /// Tracks which files are currently expanded (by file path).
    expanded_files: Rc<RefCell<HashMap<String, bool>>>,
    /// Cached raw results from the last search, grouped by file.
    cached_results: Rc<RefCell<HashMap<String, Vec<SearchResult>>>>,
    /// The auto-expand threshold from settings.
    auto_expand_threshold: Rc<RefCell<u32>>,
}

impl std::fmt::Debug for ProjectSearchPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectSearchPanel").finish_non_exhaustive()
    }
}

impl Default for ProjectSearchPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectSearchPanel {
    /// Create a new project search panel.
    pub fn new() -> Self {
        let settings = rline_config::EditorSettings::load().unwrap_or_default();

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        container.set_margin_top(4);
        container.set_margin_start(4);
        container.set_margin_end(4);

        let search_entry = gtk4::SearchEntry::new();
        search_entry.set_placeholder_text(Some("Search in files..."));
        container.append(&search_entry);

        let results_store = gio::ListStore::new::<SearchRowObject>();
        let selection = gtk4::SingleSelection::new(Some(results_store.clone()));

        let factory = gtk4::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            if let Some(list_item) = item.downcast_ref::<gtk4::ListItem>() {
                let label = gtk4::Label::new(None);
                label.set_halign(gtk4::Align::Start);
                label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                list_item.set_child(Some(&label));
            }
        });
        factory.connect_bind(|_, item| {
            if let Some(list_item) = item.downcast_ref::<gtk4::ListItem>() {
                if let Some(obj) = list_item.item().and_downcast::<SearchRowObject>() {
                    if let Some(label) = list_item.child().and_downcast::<gtk4::Label>() {
                        label.set_text(&obj.display_text());
                        if obj.is_header() {
                            label.add_css_class("heading");
                        } else {
                            label.remove_css_class("heading");
                        }
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
        container.append(&scrolled);

        let panel = Self {
            container,
            search_entry: search_entry.clone(),
            results_store: results_store.clone(),
            project_root: Rc::new(RefCell::new(None)),
            on_open_file_at_line: Rc::new(RefCell::new(None)),
            expanded_files: Rc::new(RefCell::new(HashMap::new())),
            cached_results: Rc::new(RefCell::new(HashMap::new())),
            auto_expand_threshold: Rc::new(RefCell::new(settings.search_auto_expand_threshold)),
        };

        // Wire search on activate (Enter)
        let panel_clone = panel.clone();
        search_entry.connect_activate(move |entry| {
            let query = entry.text().to_string();
            if !query.is_empty() {
                panel_clone.run_search(&query);
            }
        });

        // Wire single-click on result — file headers toggle expand, line matches open file
        let panel_for_click = panel.clone();
        let store_for_click = results_store.clone();
        let lv_for_click = list_view.clone();
        let click_gesture = gtk4::GestureClick::new();
        click_gesture.set_button(1);
        click_gesture.connect_released(move |_, _, _, _| {
            let model = match lv_for_click.model() {
                Some(m) => m,
                None => return,
            };
            let selection = match model.downcast_ref::<gtk4::SingleSelection>() {
                Some(s) => s,
                None => return,
            };
            let position = selection.selected();
            if let Some(item) = store_for_click.item(position) {
                if let Some(obj) = item.downcast_ref::<SearchRowObject>() {
                    if obj.is_header() {
                        panel_for_click.toggle_file(&obj.file_path());
                    } else {
                        let path = PathBuf::from(obj.file_path());
                        let line = rline_core::LineIndex(obj.line_number() as usize);
                        if let Some(ref cb) = *panel_for_click.on_open_file_at_line.borrow() {
                            cb(&path, line);
                        }
                    }
                }
            }
        });
        list_view.add_controller(click_gesture);

        panel
    }

    /// Run a search in the background and populate grouped results.
    fn run_search(&self, query: &str) {
        self.results_store.remove_all();
        self.expanded_files.borrow_mut().clear();
        self.cached_results.borrow_mut().clear();

        let root = match self.project_root.borrow().clone() {
            Some(r) => r,
            None => return,
        };

        let (sender, receiver) = std::sync::mpsc::channel::<SearchResult>();
        let query_owned = query.to_string();
        let root_owned = root.clone();

        // Run search in a background thread
        std::thread::spawn(move || {
            search_worker::search_files(&root_owned, &query_owned, &sender);
        });

        // Collect results and rebuild the grouped view
        let cached = self.cached_results.clone();
        let expanded = self.expanded_files.clone();
        let store = self.results_store.clone();
        let threshold = *self.auto_expand_threshold.borrow();

        glib::idle_add_local(move || {
            // Drain all available results
            let mut got_any = false;
            loop {
                match receiver.try_recv() {
                    Ok(result) => {
                        got_any = true;
                        let key = result.path.display().to_string();
                        cached.borrow_mut().entry(key).or_default().push(result);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        if got_any {
                            rebuild_store(&store, &cached.borrow(), &expanded.borrow());
                        }
                        return glib::ControlFlow::Continue;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        // Search complete — auto-expand files with few results
                        let results = cached.borrow();
                        let mut exp = expanded.borrow_mut();
                        for (path, matches) in results.iter() {
                            if matches.len() as u32 <= threshold {
                                exp.insert(path.clone(), true);
                            }
                        }
                        drop(exp);
                        rebuild_store(&store, &results, &expanded.borrow());
                        return glib::ControlFlow::Break;
                    }
                }
            }
        });
    }

    /// Toggle expansion of a file's results.
    fn toggle_file(&self, file_path: &str) {
        let mut expanded = self.expanded_files.borrow_mut();
        let is_expanded = expanded.get(file_path).copied().unwrap_or(false);
        expanded.insert(file_path.to_owned(), !is_expanded);
        drop(expanded);

        rebuild_store(
            &self.results_store,
            &self.cached_results.borrow(),
            &self.expanded_files.borrow(),
        );
    }

    /// Set the project root for searching.
    pub fn set_project_root(&self, root: &Path) {
        self.project_root.replace(Some(root.to_path_buf()));
    }

    /// Set the callback for opening a file at a specific line.
    pub fn set_on_open_file_at_line<F: Fn(&Path, rline_core::LineIndex) + 'static>(&self, f: F) {
        self.on_open_file_at_line.replace(Some(Box::new(f)));
    }

    /// Focus the search entry.
    pub fn focus_entry(&self) {
        self.search_entry.grab_focus();
    }

    /// The container widget.
    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }
}

/// Rebuild the flat store from the grouped results and expansion state.
fn rebuild_store(
    store: &gio::ListStore,
    results: &HashMap<String, Vec<SearchResult>>,
    expanded: &HashMap<String, bool>,
) {
    store.remove_all();

    // Sort file paths for stable ordering
    let mut paths: Vec<&String> = results.keys().collect();
    paths.sort();

    for file_path in paths {
        let matches = &results[file_path];
        let path = PathBuf::from(file_path);
        let is_expanded = expanded.get(file_path).copied().unwrap_or(false);

        // File header row with expand/collapse arrow
        let arrow = if is_expanded { "▼" } else { "▶" };
        let header = SearchRowObject::new_header(&path, matches.len() as u32);
        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        let count = matches.len();
        let suffix = if count == 1 { "" } else { "es" };
        header.set_display_text(format!("{arrow} {filename} ({count} match{suffix})"));
        store.append(&header);

        // Line match rows (only if expanded)
        if is_expanded {
            for result in matches {
                store.append(&SearchRowObject::new_match(result));
            }
        }
    }
}
