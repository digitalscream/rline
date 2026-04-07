//! TerminalPane — tabbed notebook of terminal emulators.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;
use vte4::prelude::*;

use super::terminal_tab::TerminalTab;

/// The terminal pane containing a notebook of terminal tabs.
#[derive(Debug, Clone)]
pub struct TerminalPane {
    container: gtk4::Box,
    notebook: gtk4::Notebook,
    default_dir: Rc<RefCell<Option<PathBuf>>>,
    tab_counter: Rc<RefCell<usize>>,
    font_family: Rc<RefCell<String>>,
    font_size: Rc<RefCell<u32>>,
}

impl Default for TerminalPane {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalPane {
    /// Create a new terminal pane with one default terminal.
    pub fn new() -> Self {
        let settings = rline_config::EditorSettings::load().unwrap_or_default();
        let notebook = gtk4::Notebook::new();
        notebook.set_scrollable(true);
        notebook.set_vexpand(true);
        notebook.set_hexpand(true);

        // "+" button to add terminals
        let add_btn = gtk4::Button::from_icon_name("list-add-symbolic");
        add_btn.set_tooltip_text(Some("New terminal"));
        notebook.set_action_widget(&add_btn, gtk4::PackType::End);

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.append(&notebook);
        container.set_vexpand(true);

        let pane = Self {
            container,
            notebook,
            default_dir: Rc::new(RefCell::new(None)),
            tab_counter: Rc::new(RefCell::new(0)),
            font_family: Rc::new(RefCell::new(settings.terminal_font_family)),
            font_size: Rc::new(RefCell::new(settings.terminal_font_size)),
        };

        // Wire "+" button
        let pane_clone = pane.clone();
        add_btn.connect_clicked(move |_| {
            pane_clone.add_terminal(None);
        });

        pane
    }

    /// Add a new terminal tab.
    pub fn add_terminal(&self, working_dir: Option<&Path>) {
        let mut counter = self.tab_counter.borrow_mut();
        *counter += 1;
        let index = *counter;

        let dir = working_dir
            .map(|p| p.to_path_buf())
            .or_else(|| self.default_dir.borrow().clone());

        let font_family = self.font_family.borrow().clone();
        let font_size = *self.font_size.borrow();
        let tab = TerminalTab::new(index, dir.as_deref(), &font_family, font_size);

        let scrolled = gtk4::ScrolledWindow::builder()
            .child(tab.widget())
            .vexpand(true)
            .build();

        let page_num = self.notebook.append_page(&scrolled, Some(tab.tab_label()));
        self.wire_tab_close_btn(tab.close_btn(), page_num);
        self.wire_tab_context_menu(tab.tab_label(), page_num);
        self.notebook.set_current_page(Some(page_num));
    }

    /// Close the terminal tab at the given page index.
    pub fn close_tab_at(&self, page_num: u32) {
        self.notebook.remove_page(Some(page_num));
    }

    /// Close all terminal tabs.
    pub fn close_all_tabs(&self) {
        let total = self.notebook.n_pages();
        for _ in 0..total {
            self.notebook.remove_page(Some(0));
        }
    }

    /// Close all tabs except the one at the given page index.
    pub fn close_tabs_except(&self, page_num: u32) {
        // Close right first (stable indices), then left.
        let total = self.notebook.n_pages();
        for i in (page_num + 1..total).rev() {
            self.notebook.remove_page(Some(i));
        }
        for _ in 0..page_num {
            self.notebook.remove_page(Some(0));
        }
    }

    /// Close all tabs to the left of the given page index.
    pub fn close_tabs_left_of(&self, page_num: u32) {
        for _ in 0..page_num {
            self.notebook.remove_page(Some(0));
        }
    }

    /// Close all tabs to the right of the given page index.
    pub fn close_tabs_right_of(&self, page_num: u32) {
        let total = self.notebook.n_pages();
        for i in (page_num + 1..total).rev() {
            self.notebook.remove_page(Some(i));
        }
    }

    /// Set the default working directory for new terminals.
    pub fn set_default_directory(&self, dir: &Path) {
        self.default_dir.replace(Some(dir.to_path_buf()));
    }

    /// Apply font settings to all existing terminal tabs and update defaults for new ones.
    pub fn apply_settings(&self, settings: &rline_config::EditorSettings) {
        self.font_family
            .replace(settings.terminal_font_family.clone());
        self.font_size.replace(settings.terminal_font_size);

        // Apply to all existing terminal tabs
        let font_desc = gtk4::pango::FontDescription::from_string(&format!(
            "{} {}",
            settings.terminal_font_family, settings.terminal_font_size
        ));
        let n_pages = self.notebook.n_pages();
        for i in 0..n_pages {
            if let Some(page) = self.notebook.nth_page(Some(i)) {
                // Each page is a ScrolledWindow containing a vte4::Terminal
                if let Some(scrolled) = page.downcast_ref::<gtk4::ScrolledWindow>() {
                    if let Some(terminal) = scrolled.child().and_downcast::<vte4::Terminal>() {
                        terminal.set_font(Some(&font_desc));
                    }
                }
            }
        }
    }

    /// Focus the currently active terminal tab.
    pub fn focus_current(&self) {
        if let Some(page_num) = self.notebook.current_page() {
            if let Some(page) = self.notebook.nth_page(Some(page_num)) {
                if let Some(scrolled) = page.downcast_ref::<gtk4::ScrolledWindow>() {
                    if let Some(terminal) = scrolled.child() {
                        terminal.grab_focus();
                    }
                }
            }
        }
    }

    /// The container widget.
    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    /// Wire a close button to close its containing terminal tab.
    fn wire_tab_close_btn(&self, btn: &gtk4::Button, initial_page: u32) {
        let notebook = self.notebook.clone();
        let page_widget = notebook.nth_page(Some(initial_page));

        btn.connect_clicked(move |_| {
            if let Some(pn) = page_widget.as_ref().and_then(|w| notebook.page_num(w)) {
                notebook.remove_page(Some(pn));
            }
        });
    }

    /// Attach a right-click context menu to a terminal tab label.
    fn wire_tab_context_menu(&self, tab_label: &gtk4::Box, initial_page: u32) {
        let pane = self.clone();
        let notebook = self.notebook.clone();
        let page_widget = notebook.nth_page(Some(initial_page));

        let gesture = gtk4::GestureClick::builder().button(3).build();

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
}
