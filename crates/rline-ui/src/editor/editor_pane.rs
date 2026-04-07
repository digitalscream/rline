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
    container: gtk4::Box,
    notebook: gtk4::Notebook,
    /// Maps file paths to notebook page indices.
    tabs: Rc<RefCell<Vec<EditorTab>>>,
    /// Map from canonical path to tab index for deduplication.
    path_to_index: Rc<RefCell<HashMap<PathBuf, usize>>>,
    settings: Rc<RefCell<EditorSettings>>,
    /// Most-recently-used tab indices (front = most recent).
    mru: Rc<RefCell<Vec<u32>>>,
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

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.append(&notebook);
        container.set_vexpand(true);
        container.set_hexpand(true);

        let settings = Rc::new(RefCell::new(settings));

        let mru: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));
        // Whether a Ctrl+Tab cycle is in progress (suppresses normal MRU updates).
        let cycling = Rc::new(RefCell::new(false));
        // Current position in the MRU list while cycling.
        let cycle_index = Rc::new(RefCell::new(0usize));

        // Track tab switches in the MRU list (only when not cycling)
        let mru_for_switch = mru.clone();
        let cycling_for_switch = cycling.clone();
        let settings_for_switch = settings.clone();
        notebook.connect_switch_page(move |_, _, page_num| {
            if *cycling_for_switch.borrow() {
                return;
            }
            let limit = settings_for_switch.borrow().tab_cycle_depth as usize;
            let mut list = mru_for_switch.borrow_mut();
            list.retain(|&p| p != page_num);
            list.insert(0, page_num);
            list.truncate(limit);
        });

        // Ctrl+Tab cycles through MRU tabs; Ctrl release commits the choice
        let mru_for_key = mru.clone();
        let cycling_for_key = cycling.clone();
        let cycle_idx_for_key = cycle_index.clone();
        let nb_for_key = notebook.clone();
        let key_ctl = gtk4::EventControllerKey::new();
        key_ctl.set_propagation_phase(gtk4::PropagationPhase::Capture);
        key_ctl.connect_key_pressed(move |_, key, _, modifiers| {
            if (key == gtk4::gdk::Key::Tab || key == gtk4::gdk::Key::ISO_Left_Tab)
                && modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK)
            {
                let list = mru_for_key.borrow();
                if list.len() < 2 {
                    return gtk4::glib::Propagation::Stop;
                }

                // Start cycling or advance the position
                let mut idx = cycle_idx_for_key.borrow_mut();
                if !*cycling_for_key.borrow() {
                    *cycling_for_key.borrow_mut() = true;
                    *idx = 1; // first press goes to MRU[1]
                } else {
                    *idx = (*idx + 1) % list.len();
                }

                let next_page = list[*idx];
                nb_for_key.set_current_page(Some(next_page));

                return gtk4::glib::Propagation::Stop;
            }
            gtk4::glib::Propagation::Proceed
        });

        // When Ctrl is released, commit the cycled-to tab to MRU front
        let mru_for_release = mru.clone();
        let cycling_for_release = cycling.clone();
        let cycle_idx_for_release = cycle_index;
        let nb_for_release = notebook.clone();
        key_ctl.connect_key_released(move |_, key, _, _| {
            if !*cycling_for_release.borrow() {
                return;
            }
            // Ctrl_L or Ctrl_R released
            if key == gtk4::gdk::Key::Control_L || key == gtk4::gdk::Key::Control_R {
                *cycling_for_release.borrow_mut() = false;
                *cycle_idx_for_release.borrow_mut() = 0;

                // Move the currently displayed page to the front of MRU
                if let Some(current) = nb_for_release.current_page() {
                    let mut list = mru_for_release.borrow_mut();
                    list.retain(|&p| p != current);
                    list.insert(0, current);
                }
            }
        });
        container.add_controller(key_ctl);

        Self {
            container,
            notebook,
            tabs: Rc::new(RefCell::new(Vec::new())),
            path_to_index: Rc::new(RefCell::new(HashMap::new())),
            settings,
            mru,
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
                    let mru_rc = self.mru.clone();
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
                            mru_rc,
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
                                        Self::remove_tab(
                                            &notebook, &tabs_rc, &path_map, &mru_rc, page_num,
                                        );
                                    }
                                    Ok(1) => {
                                        // Discard — just close
                                        Self::remove_tab(
                                            &notebook, &tabs_rc, &path_map, &mru_rc, page_num,
                                        );
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
            Self::remove_tab(
                &self.notebook,
                &self.tabs,
                &self.path_to_index,
                &self.mru,
                page_num,
            );
        }
    }

    /// Apply settings to all open tabs.
    pub fn apply_settings(&self, settings: &EditorSettings) {
        self.settings.replace(settings.clone());
        for tab in self.tabs.borrow().iter() {
            tab.apply_settings(settings);
        }
    }

    /// Show the current tab's find bar overlay.
    ///
    /// If `with_replace` is true, the replace row is also shown.
    pub fn show_find_bar(&self, with_replace: bool) {
        if let Some(page) = self.notebook.current_page() {
            let tabs = self.tabs.borrow();
            if let Some(tab) = tabs.get(page as usize) {
                tab.show_find_bar(with_replace);
            }
        }
    }

    /// The container widget to embed in the layout.
    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    fn remove_tab(
        notebook: &gtk4::Notebook,
        tabs: &Rc<RefCell<Vec<EditorTab>>>,
        path_map: &Rc<RefCell<HashMap<PathBuf, usize>>>,
        mru: &Rc<RefCell<Vec<u32>>>,
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

        // Update MRU: remove the closed tab and adjust indices for tabs that shifted
        let mut mru_list = mru.borrow_mut();
        mru_list.retain(|&p| p != page_num);
        for p in mru_list.iter_mut() {
            if *p > page_num {
                *p -= 1;
            }
        }
    }
}
