//! EditorPane — tabbed notebook of editor views.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;

use rline_config::EditorSettings;
use rline_core::LineIndex;

use super::tab::EditorTab;
use crate::error::UiError;

/// The editor pane containing a notebook of editor tabs.
#[derive(Debug, Clone)]
pub struct EditorPane {
    notebook: gtk4::Notebook,
    /// Maps file paths to notebook page indices.
    tabs: Rc<RefCell<Vec<EditorTab>>>,
    /// Map from canonical path to tab index for deduplication.
    path_to_index: Rc<RefCell<HashMap<PathBuf, usize>>>,
    settings: Rc<RefCell<EditorSettings>>,
}

impl Default for EditorPane {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorPane {
    /// Create a new empty editor pane.
    pub fn new() -> Self {
        let settings = EditorSettings::load().unwrap_or_default();
        let notebook = gtk4::Notebook::new();
        notebook.set_scrollable(true);
        notebook.set_vexpand(true);
        notebook.set_hexpand(true);

        Self {
            notebook,
            tabs: Rc::new(RefCell::new(Vec::new())),
            path_to_index: Rc::new(RefCell::new(HashMap::new())),
            settings: Rc::new(RefCell::new(settings)),
        }
    }

    /// Open a file in a new tab, or focus an existing tab if already open.
    pub fn open_file(&self, path: &Path) -> Result<(), UiError> {
        // Check if already open
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        {
            let index_map = self.path_to_index.borrow();
            if let Some(&idx) = index_map.get(&canonical) {
                self.notebook.set_current_page(Some(idx as u32));
                return Ok(());
            }
        }

        let settings = self.settings.borrow();
        let tab = EditorTab::new(&settings);
        tab.load_file(path)?;

        let page_idx = self
            .notebook
            .append_page(tab.widget(), Some(tab.tab_label()));
        self.notebook.set_tab_reorderable(tab.widget(), true);

        let idx = page_idx as usize;
        self.tabs.borrow_mut().push(tab);
        self.path_to_index.borrow_mut().insert(canonical, idx);

        self.notebook.set_current_page(Some(page_idx));
        Ok(())
    }

    /// Open a file and navigate to a specific line.
    pub fn open_file_at_line(&self, path: &Path, line: LineIndex) -> Result<(), UiError> {
        self.open_file(path)?;
        // After opening, the tab is focused — find it and goto line
        if let Some(current) = self.notebook.current_page() {
            let tabs = self.tabs.borrow();
            if let Some(tab) = tabs.get(current as usize) {
                tab.goto_line(line);
            }
        }
        Ok(())
    }

    /// Close the currently focused editor tab.
    pub fn close_current_tab(&self) {
        if let Some(page_num) = self.notebook.current_page() {
            let idx = page_num as usize;
            let tabs = self.tabs.borrow();
            if let Some(tab) = tabs.get(idx) {
                if tab.is_modified() {
                    // Show save confirmation dialog
                    let notebook = self.notebook.clone();
                    let tabs_rc = self.tabs.clone();
                    let path_map = self.path_to_index.clone();
                    let tab_clone = tab.clone();

                    let dialog = gtk4::AlertDialog::builder()
                        .message("Save changes?")
                        .detail(
                            "This file has unsaved changes. Do you want to save before closing?",
                        )
                        .buttons(["Save", "Discard", "Cancel"])
                        .default_button(0)
                        .cancel_button(2)
                        .modal(true)
                        .build();

                    // Get the window from the notebook
                    let window = notebook.root().and_downcast::<gtk4::Window>();
                    dialog.choose(
                        window.as_ref(),
                        gio::Cancellable::NONE,
                        glib::clone!(
                            #[strong]
                            notebook,
                            #[strong]
                            tabs_rc,
                            #[strong]
                            path_map,
                            #[strong]
                            tab_clone,
                            move |result| {
                                match result {
                                    Ok(0) => {
                                        // Save then close
                                        if let Err(e) = tab_clone.save() {
                                            tracing::error!("failed to save: {e}");
                                            return;
                                        }
                                        Self::remove_tab(&notebook, &tabs_rc, &path_map, page_num);
                                    }
                                    Ok(1) => {
                                        // Discard — just close
                                        Self::remove_tab(&notebook, &tabs_rc, &path_map, page_num);
                                    }
                                    _ => {
                                        // Cancel — do nothing
                                    }
                                }
                            }
                        ),
                    );
                    return;
                }
            }
            drop(tabs);
            Self::remove_tab(&self.notebook, &self.tabs, &self.path_to_index, page_num);
        }
    }

    /// Apply settings to all open tabs.
    pub fn apply_settings(&self, settings: &EditorSettings) {
        self.settings.replace(settings.clone());
        for tab in self.tabs.borrow().iter() {
            tab.apply_settings(settings);
        }
    }

    /// The notebook widget to embed in the layout.
    pub fn widget(&self) -> &gtk4::Notebook {
        &self.notebook
    }

    fn remove_tab(
        notebook: &gtk4::Notebook,
        tabs: &Rc<RefCell<Vec<EditorTab>>>,
        path_map: &Rc<RefCell<HashMap<PathBuf, usize>>>,
        page_num: u32,
    ) {
        let idx = page_num as usize;
        let mut tabs_vec = tabs.borrow_mut();
        if idx < tabs_vec.len() {
            let removed = tabs_vec.remove(idx);
            // Remove from path map
            if let Some(path) = removed.file_path() {
                let canonical = path.canonicalize().unwrap_or(path);
                path_map.borrow_mut().remove(&canonical);
            }
            // Rebuild index map
            let mut map = path_map.borrow_mut();
            map.clear();
            for (i, tab) in tabs_vec.iter().enumerate() {
                if let Some(p) = tab.file_path() {
                    let canonical = p.canonicalize().unwrap_or(p);
                    map.insert(canonical, i);
                }
            }
        }
        notebook.remove_page(Some(page_num));
    }
}
