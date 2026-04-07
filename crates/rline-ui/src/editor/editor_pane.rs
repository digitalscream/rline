//! EditorPane — tabbed notebook of editor views and diff views.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;

use rline_config::EditorSettings;
use rline_core::LineIndex;

use super::tab::EditorTab;
use crate::error::UiError;
use crate::git::git_worker::FileDiff;
use crate::git::DiffTab;

/// A tab in the editor pane — either a regular editor or a side-by-side diff.
#[derive(Debug, Clone)]
enum TabKind {
    /// A regular editable file tab.
    Editor(EditorTab),
    /// A read-only side-by-side diff view.
    Diff(DiffTab),
}

impl TabKind {
    /// A dedup key distinguishing editor tabs from diff tabs for the same file.
    fn dedup_key(&self) -> Option<PathBuf> {
        match self {
            Self::Editor(tab) => tab.file_path().map(|p| p.canonicalize().unwrap_or(p)),
            Self::Diff(tab) => {
                let mut key = PathBuf::from("diff:");
                key.push(tab.file_path());
                Some(key)
            }
        }
    }

    /// Apply settings to the tab.
    fn apply_settings(&self, settings: &EditorSettings) {
        match self {
            Self::Editor(tab) => tab.apply_settings(settings),
            Self::Diff(tab) => tab.apply_settings(settings),
        }
    }
}

/// Callback invoked after a tab is removed, receiving the remaining tab count.
type OnTabRemoved = Rc<RefCell<Option<Box<dyn Fn(usize)>>>>;

/// The editor pane containing a notebook of editor tabs.
#[derive(Clone)]
pub struct EditorPane {
    container: gtk4::Box,
    notebook: gtk4::Notebook,
    /// All tabs (editor and diff).
    tabs: Rc<RefCell<Vec<TabKind>>>,
    /// Map from dedup key to tab index.
    path_to_index: Rc<RefCell<HashMap<PathBuf, usize>>>,
    settings: Rc<RefCell<EditorSettings>>,
    /// Most-recently-used tab indices (front = most recent).
    mru: Rc<RefCell<Vec<u32>>>,
    /// Optional callback fired after a tab is removed.
    on_tab_removed: OnTabRemoved,
}

impl std::fmt::Debug for EditorPane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorPane")
            .field("tab_count", &self.tabs.borrow().len())
            .finish_non_exhaustive()
    }
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
            on_tab_removed: Rc::new(RefCell::new(None)),
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
        self.wire_tab_close_btn(tab.close_btn(), page_idx);
        self.wire_tab_context_menu(tab.tab_label(), page_idx);

        let idx = page_idx as usize;
        self.tabs.borrow_mut().push(TabKind::Editor(tab));
        self.path_to_index.borrow_mut().insert(canonical, idx);

        self.notebook.set_current_page(Some(page_idx));
        Ok(())
    }

    /// Open a side-by-side diff view for a file, or focus an existing one.
    pub fn open_diff(&self, path: &Path, diff: &FileDiff) -> Result<(), UiError> {
        // Dedup key for diff tabs.
        let mut dedup_key = PathBuf::from("diff:");
        dedup_key.push(path);

        {
            let index_map = self.path_to_index.borrow();
            if let Some(&idx) = index_map.get(&dedup_key) {
                self.notebook.set_current_page(Some(idx as u32));
                return Ok(());
            }
        }

        let settings = self.settings.borrow();
        let tab = DiffTab::load_diff(path, diff, &settings);

        let page_idx = self
            .notebook
            .append_page(tab.widget(), Some(tab.tab_label()));
        self.notebook.set_tab_reorderable(tab.widget(), true);
        self.wire_tab_close_btn(tab.close_btn(), page_idx);
        self.wire_tab_context_menu(tab.tab_label(), page_idx);

        let idx = page_idx as usize;
        self.tabs.borrow_mut().push(TabKind::Diff(tab));
        self.path_to_index.borrow_mut().insert(dedup_key, idx);

        self.notebook.set_current_page(Some(page_idx));
        Ok(())
    }

    /// Open a file and navigate to a specific line.
    pub fn open_file_at_line(&self, path: &Path, line: LineIndex) -> Result<(), UiError> {
        self.open_file(path)?;
        // After opening, the tab is focused — find it and goto line
        if let Some(current) = self.notebook.current_page() {
            let tabs = self.tabs.borrow();
            if let Some(TabKind::Editor(tab)) = tabs.get(current as usize) {
                tab.goto_line(line);
            }
        }
        Ok(())
    }

    /// The file path of the currently focused editor tab, if any.
    pub fn current_file_path(&self) -> Option<PathBuf> {
        let page_num = self.notebook.current_page()?;
        self.file_path_at(page_num)
    }

    /// The file path of the editor tab at the given page index, if any.
    pub fn file_path_at(&self, page_num: u32) -> Option<PathBuf> {
        let tabs = self.tabs.borrow();
        if let Some(TabKind::Editor(tab)) = tabs.get(page_num as usize) {
            tab.file_path()
        } else {
            None
        }
    }

    /// Save the currently focused editor tab.
    pub fn save_current_tab(&self) {
        if let Some(page_num) = self.notebook.current_page() {
            let tabs = self.tabs.borrow();
            if let Some(TabKind::Editor(tab)) = tabs.get(page_num as usize) {
                if let Err(e) = tab.save() {
                    tracing::error!("failed to save: {e}");
                }
            }
        }
    }

    /// Close the currently focused editor tab.
    pub fn close_current_tab(&self) {
        if let Some(page_num) = self.notebook.current_page() {
            self.close_tab_at(page_num);
        }
    }

    /// Close the tab at the given notebook page index, prompting to save if
    /// the tab contains unsaved changes.
    pub fn close_tab_at(&self, page_num: u32) {
        let idx = page_num as usize;
        let tabs = self.tabs.borrow();
        if let Some(TabKind::Editor(tab)) = tabs.get(idx) {
            if tab.is_modified() {
                let notebook = self.notebook.clone();
                let tabs_rc = self.tabs.clone();
                let path_map = self.path_to_index.clone();
                let mru_rc = self.mru.clone();
                let tab_clone = tab.clone();
                let on_removed = self.on_tab_removed.clone();

                let dialog = gtk4::AlertDialog::builder()
                    .message("Save changes?")
                    .detail("This file has unsaved changes. Do you want to save before closing?")
                    .buttons(["Save", "Discard", "Cancel"])
                    .default_button(0)
                    .cancel_button(2)
                    .modal(true)
                    .build();

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
                        #[strong]
                        on_removed,
                        move |result| {
                            match result {
                                Ok(0) => {
                                    if let Err(e) = tab_clone.save() {
                                        tracing::error!("failed to save: {e}");
                                        return;
                                    }
                                    Self::remove_tab(
                                        &notebook,
                                        &tabs_rc,
                                        &path_map,
                                        &mru_rc,
                                        &on_removed,
                                        page_num,
                                    );
                                }
                                Ok(1) => {
                                    Self::remove_tab(
                                        &notebook,
                                        &tabs_rc,
                                        &path_map,
                                        &mru_rc,
                                        &on_removed,
                                        page_num,
                                    );
                                }
                                _ => {}
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
            &self.on_tab_removed,
            page_num,
        );
    }

    /// Close all tabs to the right of the given page index.
    pub fn close_tabs_right_of(&self, page_num: u32) {
        let total = self.notebook.n_pages();
        // Close from rightmost to avoid index shifting issues; note that each
        // close_tab_at for modified files is async (save dialog), so only
        // unmodified/diff tabs close synchronously.  We iterate in reverse to
        // keep indices stable for the synchronous removals.
        for i in (page_num + 1..total).rev() {
            self.close_tab_at(i);
        }
    }

    /// Close all tabs to the left of the given page index.
    pub fn close_tabs_left_of(&self, page_num: u32) {
        for _ in 0..page_num {
            // Always close index 0 because each removal shifts tabs left.
            self.close_tab_at(0);
        }
    }

    /// Close all tabs except the one at the given page index.
    pub fn close_tabs_except(&self, page_num: u32) {
        // Close right first (indices remain stable), then left.
        self.close_tabs_right_of(page_num);
        // After right tabs are closed, the target tab's index may have
        // changed — but since we only closed tabs to the right, it hasn't.
        self.close_tabs_left_of(page_num);
    }

    /// Close all tabs.
    pub fn close_all_tabs(&self) {
        let total = self.notebook.n_pages();
        for _ in 0..total {
            self.close_tab_at(0);
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
            if let Some(TabKind::Editor(tab)) = tabs.get(page as usize) {
                tab.show_find_bar(with_replace);
            }
        }
    }

    /// The number of open tabs in this pane.
    pub fn tab_count(&self) -> usize {
        self.tabs.borrow().len()
    }

    /// Check whether a file (by canonical path) is open, returning the notebook
    /// page index if found.
    pub fn has_file(&self, canonical: &Path) -> Option<usize> {
        self.path_to_index.borrow().get(canonical).copied()
    }

    /// Focus the tab at the given notebook page index.
    pub fn focus_tab(&self, idx: usize) {
        self.notebook.set_current_page(Some(idx as u32));
    }

    /// Register a callback invoked after a tab is removed. The callback
    /// receives the number of remaining tabs in this pane.
    pub fn set_on_tab_removed(&self, cb: impl Fn(usize) + 'static) {
        self.on_tab_removed.replace(Some(Box::new(cb)));
    }

    /// The underlying notebook widget.
    pub fn notebook(&self) -> &gtk4::Notebook {
        &self.notebook
    }

    /// All dedup keys for currently open tabs, used by `SplitContainer` to
    /// rebuild its cross-pane path index.
    pub fn dedup_keys(&self) -> Vec<PathBuf> {
        self.tabs
            .borrow()
            .iter()
            .filter_map(|t| t.dedup_key())
            .collect()
    }

    /// The currently active editor tab, if any (not diff tabs).
    pub fn current_editor_tab(&self) -> Option<EditorTab> {
        let page = self.notebook.current_page()?;
        let tabs = self.tabs.borrow();
        match tabs.get(page as usize) {
            Some(TabKind::Editor(tab)) => Some(tab.clone()),
            _ => None,
        }
    }

    /// The container widget to embed in the layout.
    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    /// Wire a close button to close its containing tab. Because tabs can be
    /// reordered, we look up the page number dynamically from the widget.
    fn wire_tab_close_btn(&self, btn: &gtk4::Button, initial_page: u32) {
        let pane = self.clone();
        let notebook = self.notebook.clone();
        let page_widget = notebook.nth_page(Some(initial_page));

        btn.connect_clicked(move |_| {
            // Resolve current page index from widget (handles reorder).
            if let Some(pn) = page_widget.as_ref().and_then(|w| notebook.page_num(w)) {
                pane.close_tab_at(pn);
            }
        });
    }

    /// Attach a right-click context menu to a tab label with Close All, Close
    /// Others, Close All Left, and Close All Right actions.
    fn wire_tab_context_menu(&self, tab_label: &gtk4::Box, initial_page: u32) {
        let pane = self.clone();
        let notebook = self.notebook.clone();
        let page_widget = notebook.nth_page(Some(initial_page));

        let gesture = gtk4::GestureClick::builder()
            .button(3) // right-click
            .build();

        gesture.connect_pressed(move |gesture, _, x, y| {
            let Some(pn) = page_widget.as_ref().and_then(|w| notebook.page_num(w)) else {
                return;
            };
            let Some(widget) = gesture.widget() else {
                return;
            };

            let menu = gio::Menu::new();
            menu.append(Some("Close"), Some("tab.close"));
            menu.append(Some("Close All"), Some("tab.close-all"));
            menu.append(Some("Close Others"), Some("tab.close-others"));
            menu.append(Some("Close All Left"), Some("tab.close-left"));
            menu.append(Some("Close All Right"), Some("tab.close-right"));

            let action_group = gio::SimpleActionGroup::new();

            let p = pane.clone();
            let close = gio::SimpleAction::new("close", None);
            close.connect_activate(move |_, _| p.close_tab_at(pn));
            action_group.add_action(&close);

            let p = pane.clone();
            let close_all = gio::SimpleAction::new("close-all", None);
            close_all.connect_activate(move |_, _| p.close_all_tabs());
            action_group.add_action(&close_all);

            let p = pane.clone();
            let close_others = gio::SimpleAction::new("close-others", None);
            close_others.connect_activate(move |_, _| p.close_tabs_except(pn));
            action_group.add_action(&close_others);

            let p = pane.clone();
            let close_left = gio::SimpleAction::new("close-left", None);
            close_left.connect_activate(move |_, _| p.close_tabs_left_of(pn));
            action_group.add_action(&close_left);

            let p = pane.clone();
            let close_right = gio::SimpleAction::new("close-right", None);
            close_right.connect_activate(move |_, _| p.close_tabs_right_of(pn));
            action_group.add_action(&close_right);

            widget.insert_action_group("tab", Some(&action_group));

            let popover = gtk4::PopoverMenu::from_model(Some(&menu));
            popover.set_parent(&widget);
            popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });

        tab_label.add_controller(gesture);
    }

    fn remove_tab(
        notebook: &gtk4::Notebook,
        tabs: &Rc<RefCell<Vec<TabKind>>>,
        path_map: &Rc<RefCell<HashMap<PathBuf, usize>>>,
        mru: &Rc<RefCell<Vec<u32>>>,
        on_removed: &OnTabRemoved,
        page_num: u32,
    ) {
        let idx = page_num as usize;
        let remaining;
        {
            let mut tabs_vec = tabs.borrow_mut();
            if idx < tabs_vec.len() {
                let removed = tabs_vec.remove(idx);
                // Remove from path map using the dedup key.
                if let Some(key) = removed.dedup_key() {
                    path_map.borrow_mut().remove(&key);
                }
                // Rebuild index map.
                let mut map = path_map.borrow_mut();
                map.clear();
                for (i, tab) in tabs_vec.iter().enumerate() {
                    if let Some(key) = tab.dedup_key() {
                        map.insert(key, i);
                    }
                }
            }
            remaining = tabs_vec.len();
        }
        notebook.remove_page(Some(page_num));

        // Update MRU: remove the closed tab and adjust indices for tabs that shifted.
        let mut mru_list = mru.borrow_mut();
        mru_list.retain(|&p| p != page_num);
        for p in mru_list.iter_mut() {
            if *p > page_num {
                *p -= 1;
            }
        }
        drop(mru_list);

        // Notify the container about the removal.
        if let Some(ref cb) = *on_removed.borrow() {
            cb(remaining);
        }
    }
}
