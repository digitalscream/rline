//! SplitContainer — manages one or two side-by-side `EditorPane` instances.
//!
//! Supports vertical splitting via `Ctrl+\`, cross-pane file deduplication,
//! and automatic collapse when a split pane becomes empty.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;

use rline_config::EditorSettings;
use rline_core::LineIndex;

use super::editor_pane::EditorPane;
use crate::error::UiError;
use crate::git::git_worker::FileDiff;

/// Identifies which pane in a split layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PaneId {
    Left,
    Right,
}

/// The internal layout state.
enum SplitState {
    /// A single editor pane occupying the full width.
    Single { pane: EditorPane },
    /// Two panes arranged side-by-side in a `gtk4::Paned`.
    Split {
        paned: gtk4::Paned,
        left: EditorPane,
        right: EditorPane,
    },
}

/// Callback invoked when the active editor tab changes, receiving the file path
/// (if any) of the newly focused tab.
type OnActiveFileChanged = Rc<RefCell<Option<Box<dyn Fn(Option<PathBuf>)>>>>;

/// Container that holds one or two `EditorPane` instances, supporting vertical
/// split via `Ctrl+\`. New files open in the last-focused pane, and closing
/// the last tab in a split pane collapses back to a single pane.
#[derive(Clone)]
pub struct SplitContainer {
    /// Stable outer widget that stays in the layout tree.
    outer: gtk4::Box,
    state: Rc<RefCell<SplitState>>,
    /// Cross-pane file deduplication: canonical path → which pane holds it.
    global_paths: Rc<RefCell<HashMap<PathBuf, PaneId>>>,
    /// Which pane last received focus.
    active_pane: Rc<RefCell<PaneId>>,
    settings: Rc<RefCell<EditorSettings>>,
    /// Callback fired when the active editor file changes (tab switch or pane focus).
    on_active_file_changed: OnActiveFileChanged,
}

impl std::fmt::Debug for SplitContainer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state_name = match &*self.state.borrow() {
            SplitState::Single { .. } => "Single",
            SplitState::Split { .. } => "Split",
        };
        f.debug_struct("SplitContainer")
            .field("state", &state_name)
            .field("active_pane", &self.active_pane)
            .finish_non_exhaustive()
    }
}

impl Default for SplitContainer {
    fn default() -> Self {
        Self::new()
    }
}

impl SplitContainer {
    /// Create a new split container with a single editor pane.
    pub fn new() -> Self {
        let settings = EditorSettings::load().unwrap_or_default();
        let pane = EditorPane::new();

        let outer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        outer.set_vexpand(true);
        outer.set_hexpand(true);
        outer.append(pane.widget());

        let settings = Rc::new(RefCell::new(settings));
        let on_active_file_changed: OnActiveFileChanged = Rc::new(RefCell::new(None));

        // Fire the file-changed callback whenever the initial pane switches tabs.
        // Use the signal's page_num parameter (the NEW page) because
        // `current_file_path()` still returns the OLD page at signal time.
        {
            let cb = on_active_file_changed.clone();
            let pane_clone = pane.clone();
            pane.notebook().connect_switch_page(move |_, _, page_num| {
                if let Some(ref f) = *cb.borrow() {
                    f(pane_clone.file_path_at(page_num));
                }
            });
        }

        let sc = Self {
            outer,
            state: Rc::new(RefCell::new(SplitState::Single { pane })),
            global_paths: Rc::new(RefCell::new(HashMap::new())),
            active_pane: Rc::new(RefCell::new(PaneId::Left)),
            settings,
            on_active_file_changed,
        };

        sc.wire_tab_removed_callback(PaneId::Left);
        sc
    }

    /// The stable container widget to embed in the layout.
    pub fn widget(&self) -> &gtk4::Box {
        &self.outer
    }

    /// Open a file in the active pane, or focus it if already open in any pane.
    pub fn open_file(&self, path: &Path) -> Result<(), UiError> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Cross-pane dedup: if already open, focus that pane and tab.
        if self.try_focus_existing(&canonical) {
            return Ok(());
        }

        let active_id = *self.active_pane.borrow();
        let pane = self.get_active_pane();
        pane.open_file(path)?;
        self.global_paths.borrow_mut().insert(canonical, active_id);
        Ok(())
    }

    /// Open a file and navigate to a specific line.
    pub fn open_file_at_line(&self, path: &Path, line: LineIndex) -> Result<(), UiError> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        if self.try_focus_existing(&canonical) {
            // File is focused — now just navigate to the line.
            let pane = self.get_active_pane();
            return pane.open_file_at_line(path, line);
        }

        let active_id = *self.active_pane.borrow();
        let pane = self.get_active_pane();
        pane.open_file_at_line(path, line)?;
        self.global_paths.borrow_mut().insert(canonical, active_id);
        Ok(())
    }

    /// Open a side-by-side diff view in the active pane.
    pub fn open_diff(&self, path: &Path, diff: &FileDiff) -> Result<(), UiError> {
        let mut dedup_key = PathBuf::from("diff:");
        dedup_key.push(path);

        if self.try_focus_existing(&dedup_key) {
            return Ok(());
        }

        let active_id = *self.active_pane.borrow();
        let pane = self.get_active_pane();
        pane.open_diff(path, diff)?;
        self.global_paths.borrow_mut().insert(dedup_key, active_id);
        Ok(())
    }

    /// Save the current tab in the active pane.
    pub fn save_current_tab(&self) {
        self.get_active_pane().save_current_tab();
    }

    /// Close the current tab in the active pane. If this empties a split pane,
    /// the pane is removed and the remaining pane fills the space.
    pub fn close_current_tab(&self) {
        self.get_active_pane().close_current_tab();
    }

    /// Show the find bar on the active pane's current tab.
    pub fn show_find_bar(&self, with_replace: bool) {
        self.get_active_pane().show_find_bar(with_replace);
    }

    /// Apply settings to all panes.
    pub fn apply_settings(&self, settings: &EditorSettings) {
        self.settings.replace(settings.clone());
        let state = self.state.borrow();
        match &*state {
            SplitState::Single { pane } => pane.apply_settings(settings),
            SplitState::Split { left, right, .. } => {
                left.apply_settings(settings);
                right.apply_settings(settings);
            }
        }
    }

    /// Register a callback invoked whenever the active editor file changes
    /// (tab switch or pane focus change in split mode).
    pub fn set_on_active_file_changed<F: Fn(Option<PathBuf>) + 'static>(&self, f: F) {
        self.on_active_file_changed.replace(Some(Box::new(f)));
    }

    /// Split the editor area vertically into two panes. The currently active
    /// file is duplicated into the new (right) pane. No-op if already split.
    pub fn split_vertical(&self) {
        if matches!(&*self.state.borrow(), SplitState::Split { .. }) {
            return;
        }

        // Remember which file is active so we can open it in the new pane.
        let active_file = {
            let state = self.state.borrow();
            match &*state {
                SplitState::Single { pane } => pane.current_file_path(),
                SplitState::Split { .. } => None,
            }
        };

        // Take out the current single pane.
        let old_state = self.state.replace(SplitState::Single {
            pane: EditorPane::new(),
        });
        let existing = match old_state {
            SplitState::Single { pane } => pane,
            SplitState::Split { .. } => unreachable!(),
        };

        // Unparent existing pane from outer.
        self.outer.remove(existing.widget());

        // Create the new right pane with current settings.
        let new_pane = EditorPane::new();
        {
            let settings = self.settings.borrow();
            new_pane.apply_settings(&settings);
        }

        // Create the horizontal Paned.
        let paned = gtk4::Paned::new(gtk4::Orientation::Horizontal);
        paned.set_start_child(Some(existing.widget()));
        paned.set_end_child(Some(new_pane.widget()));
        paned.set_resize_start_child(true);
        paned.set_resize_end_child(true);
        paned.set_shrink_start_child(false);
        paned.set_shrink_end_child(false);

        // Split in half once the widget is mapped and has a real width.
        let paned_for_map = paned.clone();
        paned.connect_map(move |_| {
            let width = paned_for_map.width();
            if width > 0 {
                paned_for_map.set_position(width / 2);
            }
        });

        self.outer.append(&paned);

        // Attach focus controllers to both panes.
        self.attach_focus_tracking(&existing, PaneId::Left);
        self.attach_focus_tracking(&new_pane, PaneId::Right);

        // Duplicate the active file into the new pane (bypasses cross-pane
        // dedup since we intentionally want it in both panes).
        if let Some(ref path) = active_file {
            if let Err(e) = new_pane.open_file(path) {
                tracing::warn!("failed to duplicate file into split pane: {e}");
            }
            // Register in global_paths under the right pane. The left pane's
            // copy is already tracked — we update it to point to Left so that
            // cross-pane dedup still works for *new* open_file calls.
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            self.global_paths
                .borrow_mut()
                .insert(canonical, PaneId::Left);
        }

        // Update state and wire callbacks.
        self.state.replace(SplitState::Split {
            paned,
            left: existing,
            right: new_pane,
        });
        self.wire_tab_removed_callback(PaneId::Left);
        self.wire_tab_removed_callback(PaneId::Right);

        // Focus the new (right) pane.
        *self.active_pane.borrow_mut() = PaneId::Right;
        if let Some(p) = self.get_pane(PaneId::Right) {
            p.widget().grab_focus();
        }
    }

    // ── Private helpers ────────────────────────────────────────────

    /// Try to focus an already-open file by its dedup key. Returns `true` if
    /// the file was found and focused.
    fn try_focus_existing(&self, dedup_key: &Path) -> bool {
        let pane_id = match self.global_paths.borrow().get(dedup_key) {
            Some(&id) => id,
            None => return false,
        };
        let target = match self.get_pane(pane_id) {
            Some(p) => p,
            None => return false,
        };
        match target.has_file(dedup_key) {
            Some(idx) => {
                *self.active_pane.borrow_mut() = pane_id;
                target.focus_tab(idx);
                target.widget().grab_focus();
                true
            }
            None => false,
        }
    }

    /// Get a clone of the currently active pane.
    fn get_active_pane(&self) -> EditorPane {
        let state = self.state.borrow();
        let active = *self.active_pane.borrow();
        match &*state {
            SplitState::Single { pane } => pane.clone(),
            SplitState::Split { left, right, .. } => match active {
                PaneId::Left => left.clone(),
                PaneId::Right => right.clone(),
            },
        }
    }

    /// Get the pane for a given `PaneId`, if it exists in the current state.
    fn get_pane(&self, id: PaneId) -> Option<EditorPane> {
        let state = self.state.borrow();
        match &*state {
            SplitState::Single { pane } if id == PaneId::Left => Some(pane.clone()),
            SplitState::Single { .. } => None,
            SplitState::Split { left, right, .. } => match id {
                PaneId::Left => Some(left.clone()),
                PaneId::Right => Some(right.clone()),
            },
        }
    }

    /// Attach focus-tracking to a pane so that any interaction (tab switch,
    /// click into the editor area) marks it as active and notifies the
    /// file-changed callback.
    fn attach_focus_tracking(&self, pane: &EditorPane, id: PaneId) {
        // Track notebook tab switches — fires when user clicks a tab.
        // Use the signal's page_num (the NEW page) because current_page()
        // still returns the old page at signal time.
        let active_for_nb = self.active_pane.clone();
        let cb_for_nb = self.on_active_file_changed.clone();
        let pane_for_nb = pane.clone();
        pane.notebook().connect_switch_page(move |_, _, page_num| {
            *active_for_nb.borrow_mut() = id;
            if let Some(ref f) = *cb_for_nb.borrow() {
                f(pane_for_nb.file_path_at(page_num));
            }
        });

        // Track any click inside the pane container — catches clicks on the
        // sourceview body that don't trigger a tab switch.
        let active_for_click = self.active_pane.clone();
        let cb_for_click = self.on_active_file_changed.clone();
        let pane_for_click = pane.clone();
        let click_ctl = gtk4::GestureClick::new();
        click_ctl.set_propagation_phase(gtk4::PropagationPhase::Capture);
        click_ctl.connect_pressed(move |_, _, _, _| {
            *active_for_click.borrow_mut() = id;
            if let Some(ref f) = *cb_for_click.borrow() {
                f(pane_for_click.current_file_path());
            }
        });
        pane.widget().add_controller(click_ctl);

        // Also track keyboard focus entering the pane (e.g. Tab navigation).
        let active_for_focus = self.active_pane.clone();
        let cb_for_focus = self.on_active_file_changed.clone();
        let pane_for_focus = pane.clone();
        let focus_ctl = gtk4::EventControllerFocus::new();
        focus_ctl.set_propagation_phase(gtk4::PropagationPhase::Capture);
        focus_ctl.connect_enter(move |_| {
            *active_for_focus.borrow_mut() = id;
            if let Some(ref f) = *cb_for_focus.borrow() {
                f(pane_for_focus.current_file_path());
            }
        });
        pane.widget().add_controller(focus_ctl);
    }

    /// Wire the `on_tab_removed` callback for the given pane so that an empty
    /// split pane triggers collapse back to single-pane mode.
    fn wire_tab_removed_callback(&self, pane_id: PaneId) {
        let state = self.state.clone();
        let global_paths = self.global_paths.clone();
        let outer = self.outer.clone();
        let active_pane = self.active_pane.clone();

        if let Some(pane) = self.get_pane(pane_id) {
            pane.set_on_tab_removed(move |remaining| {
                if remaining > 0 {
                    return;
                }
                // Only collapse if we're in split state.
                if !matches!(&*state.borrow(), SplitState::Split { .. }) {
                    return;
                }

                let keep = match pane_id {
                    PaneId::Left => PaneId::Right,
                    PaneId::Right => PaneId::Left,
                };

                // Perform the unsplit inline since we can't call &self methods
                // from inside the closure.
                let old_state = state.replace(SplitState::Single {
                    pane: EditorPane::new(),
                });
                if let SplitState::Split {
                    paned, left, right, ..
                } = old_state
                {
                    // Unparent both children from the Paned.
                    paned.set_start_child(None::<&gtk4::Widget>);
                    paned.set_end_child(None::<&gtk4::Widget>);

                    let surviving = match keep {
                        PaneId::Left => left,
                        PaneId::Right => right,
                    };

                    outer.remove(&paned);
                    outer.append(surviving.widget());

                    // Rebuild global paths from the surviving pane.
                    {
                        let mut gp = global_paths.borrow_mut();
                        gp.clear();
                        for key in surviving.dedup_keys() {
                            gp.insert(key, PaneId::Left);
                        }
                    }

                    state.replace(SplitState::Single {
                        pane: surviving.clone(),
                    });
                    *active_pane.borrow_mut() = PaneId::Left;

                    // Set a no-op callback for single-pane mode (nothing to
                    // collapse when there's only one pane).
                    surviving.set_on_tab_removed(|_| {});
                    surviving.widget().grab_focus();
                }
            });
        }
    }
}
