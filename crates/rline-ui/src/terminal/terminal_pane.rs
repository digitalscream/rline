//! TerminalPane — tabbed notebook of terminal emulators.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;

use super::terminal_tab::TerminalTab;

/// The terminal pane containing a notebook of terminal tabs.
#[derive(Debug, Clone)]
pub struct TerminalPane {
    container: gtk4::Box,
    notebook: gtk4::Notebook,
    default_dir: Rc<RefCell<Option<PathBuf>>>,
    tab_counter: Rc<RefCell<usize>>,
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
            font_size: Rc::new(RefCell::new(settings.terminal_font_size)),
        };

        // Wire "+" button
        let pane_clone = pane.clone();
        add_btn.connect_clicked(move |_| {
            pane_clone.add_terminal(None);
        });

        // Add one default terminal
        pane.add_terminal(None);

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

        let font_size = *self.font_size.borrow();
        let tab = TerminalTab::new(index, dir.as_deref(), font_size);

        let scrolled = gtk4::ScrolledWindow::builder()
            .child(tab.widget())
            .vexpand(true)
            .build();

        let page_num = self.notebook.append_page(&scrolled, Some(tab.tab_label()));
        self.notebook.set_current_page(Some(page_num));
    }

    /// Set the default working directory for new terminals.
    pub fn set_default_directory(&self, dir: &Path) {
        self.default_dir.replace(Some(dir.to_path_buf()));
    }

    /// The container widget.
    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }
}
