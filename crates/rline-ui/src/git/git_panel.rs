//! GitPanel — left-pane panel showing staged and unstaged changes.
//!
//! Displays the current branch name, lists changed files grouped by
//! staged/unstaged sections, and provides stage/unstage/discard actions.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;

use super::git_status_row::GitStatusRow;
use super::git_worker::{self, GitStatusResult};

/// The git integration panel for the left sidebar.
#[derive(Clone)]
pub struct GitPanel {
    container: gtk4::Box,
    list_view: gtk4::ListView,
    status_store: gio::ListStore,
    project_root: Rc<RefCell<Option<PathBuf>>>,
    // Callback: user clicked a file to view its diff (path, is_staged).
    #[allow(clippy::type_complexity)]
    on_open_diff: Rc<RefCell<Option<Box<dyn Fn(&Path, bool)>>>>,
    branch_label: gtk4::Label,
    staged_collapsed: Rc<RefCell<bool>>,
    unstaged_collapsed: Rc<RefCell<bool>>,
    cached_status: Rc<RefCell<Option<GitStatusResult>>>,
    /// Callback fired after every status refresh with the total change count.
    #[allow(clippy::type_complexity)]
    on_status_refreshed: Rc<RefCell<Option<Box<dyn Fn(usize)>>>>,
}

impl std::fmt::Debug for GitPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitPanel").finish_non_exhaustive()
    }
}

impl Default for GitPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl GitPanel {
    /// Create a new git panel.
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.set_vexpand(true);

        // ── Header: branch label + refresh button ──
        let header_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        header_box.set_margin_top(4);
        header_box.set_margin_start(8);
        header_box.set_margin_end(4);
        header_box.set_margin_bottom(4);

        let branch_label = gtk4::Label::new(Some("No repository"));
        branch_label.set_halign(gtk4::Align::Start);
        branch_label.set_hexpand(true);
        branch_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        branch_label.add_css_class("heading");
        header_box.append(&branch_label);

        let refresh_button = gtk4::Button::from_icon_name("view-refresh-symbolic");
        refresh_button.set_tooltip_text(Some("Refresh"));
        refresh_button.add_css_class("flat");
        header_box.append(&refresh_button);

        container.append(&header_box);

        // ── Commit message area ──
        let commit_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        commit_box.set_margin_start(4);
        commit_box.set_margin_end(4);
        commit_box.set_margin_bottom(4);

        let commit_entry = gtk4::TextView::new();
        commit_entry.set_wrap_mode(gtk4::WrapMode::Word);
        commit_entry.set_height_request(60);
        commit_entry.set_top_margin(4);
        commit_entry.set_bottom_margin(4);
        commit_entry.set_left_margin(4);
        commit_entry.set_right_margin(4);
        commit_entry.add_css_class("commit-input");
        let placeholder_buffer = commit_entry.buffer();
        placeholder_buffer.set_text("Commit message...");

        // Clear placeholder on first focus.
        let placeholder_cleared = Rc::new(RefCell::new(false));
        let pc = placeholder_cleared.clone();
        let ce = commit_entry.clone();
        let focus_ctl = gtk4::EventControllerFocus::new();
        focus_ctl.connect_enter(move |_| {
            if !*pc.borrow() {
                ce.buffer().set_text("");
                *pc.borrow_mut() = true;
            }
        });
        commit_entry.add_controller(focus_ctl);

        let commit_scrolled = gtk4::ScrolledWindow::builder()
            .child(&commit_entry)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .height_request(60)
            .build();
        commit_box.append(&commit_scrolled);

        let commit_button = gtk4::Button::with_label("Commit");
        commit_button.set_tooltip_text(Some("Commit (Ctrl+Enter)"));
        commit_button.add_css_class("suggested-action");
        commit_box.append(&commit_button);

        container.append(&commit_box);

        // ── Status list ──
        let status_store = gio::ListStore::new::<GitStatusRow>();
        let selection = gtk4::SingleSelection::new(Some(status_store.clone()));

        let factory = gtk4::SignalListItemFactory::new();
        Self::setup_factory(&factory);

        let list_view = gtk4::ListView::new(Some(selection), Some(factory));
        list_view.set_vexpand(true);
        list_view.add_css_class("compact-list");

        let scrolled = gtk4::ScrolledWindow::builder()
            .child(&list_view)
            .vexpand(true)
            .build();
        container.append(&scrolled);

        let panel = Self {
            container,
            list_view: list_view.clone(),
            status_store: status_store.clone(),
            project_root: Rc::new(RefCell::new(None)),
            on_open_diff: Rc::new(RefCell::new(None)),
            branch_label,
            staged_collapsed: Rc::new(RefCell::new(false)),
            unstaged_collapsed: Rc::new(RefCell::new(false)),
            cached_status: Rc::new(RefCell::new(None)),
            on_status_refreshed: Rc::new(RefCell::new(None)),
        };

        // Wire refresh button.
        let panel_for_refresh = panel.clone();
        refresh_button.connect_clicked(move |_| {
            panel_for_refresh.refresh();
        });

        // Shared commit logic used by both the button and Ctrl+Enter.
        let do_commit = {
            let panel_c = panel.clone();
            let ce_c = commit_entry.clone();
            let pc_c = placeholder_cleared.clone();
            Rc::new(move || {
                let buffer = ce_c.buffer();
                let (start, end) = buffer.bounds();
                let message = buffer.text(&start, &end, false).to_string();
                let message = message.trim().to_string();
                if message.is_empty() || !*pc_c.borrow() {
                    return;
                }
                panel_c.commit(&message);
                buffer.set_text("");
            })
        };

        // Wire commit button.
        let do_commit_btn = do_commit.clone();
        commit_button.connect_clicked(move |_| {
            do_commit_btn();
        });

        // Wire Ctrl+Enter in the commit text view.
        let do_commit_key = do_commit.clone();
        let key_ctl = gtk4::EventControllerKey::new();
        key_ctl.connect_key_pressed(move |_, key, _, modifiers| {
            if (key == gtk4::gdk::Key::Return || key == gtk4::gdk::Key::KP_Enter)
                && modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK)
            {
                do_commit_key();
                return gtk4::glib::Propagation::Stop;
            }
            gtk4::glib::Propagation::Proceed
        });
        commit_entry.add_controller(key_ctl);

        // Wire single-click on list items.
        let panel_for_click = panel.clone();
        let store_for_click = status_store;
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
                if let Some(obj) = item.downcast_ref::<GitStatusRow>() {
                    if obj.is_header() {
                        panel_for_click.toggle_section(obj.is_staged());
                    } else {
                        panel_for_click.handle_file_click(obj);
                    }
                }
            }
        });
        list_view.add_controller(click_gesture);

        panel
    }

    /// Set the factory for rendering list items.
    fn setup_factory(factory: &gtk4::SignalListItemFactory) {
        factory.connect_setup(|_, item| {
            if let Some(list_item) = item.downcast_ref::<gtk4::ListItem>() {
                let row_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
                row_box.set_margin_start(4);
                row_box.set_margin_end(4);

                // Status badge (e.g. "M", "A", "D")
                let status_label = gtk4::Label::new(None);
                status_label.set_width_chars(2);
                status_label.set_halign(gtk4::Align::Center);
                status_label.set_widget_name("status-badge");

                // Display name
                let name_label = gtk4::Label::new(None);
                name_label.set_halign(gtk4::Align::Start);
                name_label.set_hexpand(true);
                name_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                name_label.set_widget_name("name-label");

                // Action buttons container
                let actions_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
                actions_box.set_widget_name("actions-box");

                row_box.append(&status_label);
                row_box.append(&name_label);
                row_box.append(&actions_box);

                list_item.set_child(Some(&row_box));
            }
        });

        factory.connect_bind(|_, item| {
            let list_item = match item.downcast_ref::<gtk4::ListItem>() {
                Some(li) => li,
                None => return,
            };
            let obj = match list_item.item().and_downcast::<GitStatusRow>() {
                Some(o) => o,
                None => return,
            };
            let row_box = match list_item.child().and_downcast::<gtk4::Box>() {
                Some(b) => b,
                None => return,
            };

            let status_label = find_child_by_name::<gtk4::Label>(&row_box, "status-badge");
            let name_label = find_child_by_name::<gtk4::Label>(&row_box, "name-label");
            let actions_box = find_child_by_name::<gtk4::Box>(&row_box, "actions-box");

            if obj.is_header() {
                // Section header: arrow + title
                let collapsed_marker = if obj.item_count() == 0 { "" } else { "▼" };
                if let Some(ref sl) = status_label {
                    sl.set_text(collapsed_marker);
                    sl.remove_css_class("git-status-m");
                    sl.remove_css_class("git-status-a");
                    sl.remove_css_class("git-status-d");
                }
                if let Some(ref nl) = name_label {
                    let count = obj.item_count();
                    let title = if obj.is_staged() {
                        format!("Staged Changes ({count})")
                    } else {
                        format!("Changes ({count})")
                    };
                    nl.set_text(&title);
                    nl.add_css_class("heading");
                }
                // Stage All / Unstage All button on section headers.
                if let Some(ref ab) = actions_box {
                    clear_box(ab);
                    if obj.item_count() > 0 {
                        if obj.is_staged() {
                            let btn = gtk4::Button::from_icon_name("list-remove-symbolic");
                            btn.set_tooltip_text(Some("Unstage All"));
                            btn.add_css_class("flat");
                            btn.add_css_class("circular");
                            btn.set_widget_name("__unstage_all__");
                            ab.append(&btn);
                        } else {
                            let btn = gtk4::Button::from_icon_name("list-add-symbolic");
                            btn.set_tooltip_text(Some("Stage All"));
                            btn.add_css_class("flat");
                            btn.add_css_class("circular");
                            btn.set_widget_name("__stage_all__");
                            ab.append(&btn);
                        }
                    }
                }
            } else {
                // File entry: status badge + filename + action buttons
                let status_str = obj.status();
                if let Some(ref sl) = status_label {
                    sl.set_text(&status_str);
                    // Apply color class based on status.
                    sl.remove_css_class("git-status-m");
                    sl.remove_css_class("git-status-a");
                    sl.remove_css_class("git-status-d");
                    sl.remove_css_class("git-status-r");
                    sl.remove_css_class("git-status-c");
                    match status_str.as_str() {
                        "M" => sl.add_css_class("git-status-m"),
                        "A" => sl.add_css_class("git-status-a"),
                        "D" => sl.add_css_class("git-status-d"),
                        "R" => sl.add_css_class("git-status-r"),
                        "C" => sl.add_css_class("git-status-c"),
                        _ => {}
                    }
                }
                if let Some(ref nl) = name_label {
                    nl.set_text(&obj.display_name());
                    nl.remove_css_class("heading");
                }
                // Build action buttons.
                if let Some(ref ab) = actions_box {
                    clear_box(ab);

                    if obj.is_staged() {
                        // Unstage button (minus)
                        let unstage_btn = gtk4::Button::from_icon_name("list-remove-symbolic");
                        unstage_btn.set_tooltip_text(Some("Unstage"));
                        unstage_btn.add_css_class("flat");
                        unstage_btn.add_css_class("circular");
                        let file_path = obj.file_path();
                        unstage_btn.set_widget_name(&file_path);
                        ab.append(&unstage_btn);
                    } else {
                        // Stage button (plus)
                        let stage_btn = gtk4::Button::from_icon_name("list-add-symbolic");
                        stage_btn.set_tooltip_text(Some("Stage"));
                        stage_btn.add_css_class("flat");
                        stage_btn.add_css_class("circular");
                        stage_btn.set_widget_name(&obj.file_path());
                        ab.append(&stage_btn);

                        // Discard button (revert)
                        let discard_btn = gtk4::Button::from_icon_name("edit-undo-symbolic");
                        discard_btn.set_tooltip_text(Some("Discard Changes"));
                        discard_btn.add_css_class("flat");
                        discard_btn.add_css_class("circular");
                        discard_btn.set_widget_name(&obj.file_path());
                        ab.append(&discard_btn);
                    }
                }
            }
        });
    }

    /// Handle a click on a file row — invoke the diff callback.
    fn handle_file_click(&self, row: &GitStatusRow) {
        let root = match self.project_root.borrow().clone() {
            Some(r) => r,
            None => return,
        };
        let relative_path = row.file_path();
        let abs_path = root.join(&relative_path);
        let is_staged = row.is_staged();

        if let Some(ref cb) = *self.on_open_diff.borrow() {
            cb(&abs_path, is_staged);
        }
    }

    /// Toggle a section's collapsed state.
    fn toggle_section(&self, is_staged: bool) {
        if is_staged {
            let mut collapsed = self.staged_collapsed.borrow_mut();
            *collapsed = !*collapsed;
        } else {
            let mut collapsed = self.unstaged_collapsed.borrow_mut();
            *collapsed = !*collapsed;
        }
        self.rebuild_store();
    }

    /// Refresh git status from the repository.
    pub fn refresh(&self) {
        let root = match self.project_root.borrow().clone() {
            Some(r) => r,
            None => return,
        };

        let (sender, receiver) = std::sync::mpsc::channel::<Result<GitStatusResult, String>>();

        std::thread::spawn(move || {
            let result = git_worker::get_status(&root).map_err(|e| e.to_string());
            let _ = sender.send(result);
        });

        let cached = self.cached_status.clone();
        let branch_label = self.branch_label.clone();
        let panel = self.clone();

        glib::idle_add_local(move || match receiver.try_recv() {
            Ok(Ok(status)) => {
                let branch = status.branch_name.as_deref().unwrap_or("detached HEAD");
                branch_label.set_text(&format!("\u{e0a0} {branch}"));
                let count = status.staged.len() + status.unstaged.len();
                cached.replace(Some(status));
                panel.rebuild_store();
                if let Some(ref cb) = *panel.on_status_refreshed.borrow() {
                    cb(count);
                }
                glib::ControlFlow::Break
            }
            Ok(Err(e)) => {
                tracing::error!("git status failed: {e}");
                branch_label.set_text("Git error");
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });
    }

    /// Rebuild the flat store from the cached status.
    fn rebuild_store(&self) {
        self.status_store.remove_all();

        let cached = self.cached_status.borrow();
        let status = match cached.as_ref() {
            Some(s) => s,
            None => return,
        };

        let staged_collapsed = *self.staged_collapsed.borrow();
        let unstaged_collapsed = *self.unstaged_collapsed.borrow();

        // Staged section
        if !status.staged.is_empty() {
            let header =
                GitStatusRow::new_header("Staged Changes", status.staged.len() as u32, true);
            if staged_collapsed {
                header.set_display_name("Staged Changes");
            }
            self.status_store.append(&header);

            if !staged_collapsed {
                for file in &status.staged {
                    let display = file.path.display().to_string();
                    let row = GitStatusRow::new_file(&display, &display, file.status.label(), true);
                    self.status_store.append(&row);
                }
            }
        }

        // Unstaged section
        if !status.unstaged.is_empty() {
            let header = GitStatusRow::new_header("Changes", status.unstaged.len() as u32, false);
            if unstaged_collapsed {
                header.set_display_name("Changes");
            }
            self.status_store.append(&header);

            if !unstaged_collapsed {
                for file in &status.unstaged {
                    let display = file.path.display().to_string();
                    let row =
                        GitStatusRow::new_file(&display, &display, file.status.label(), false);
                    self.status_store.append(&row);
                }
            }
        }

        // Show message when repo is clean.
        if status.staged.is_empty() && status.unstaged.is_empty() {
            let row = GitStatusRow::new_header("No changes", 0, false);
            self.status_store.append(&row);
        }
    }

    /// Stage a file and refresh.
    pub fn stage_file(&self, relative_path: &str) {
        let root = match self.project_root.borrow().clone() {
            Some(r) => r,
            None => return,
        };

        let abs_path = root.join(relative_path);
        let (sender, receiver) = std::sync::mpsc::channel::<Result<(), String>>();

        std::thread::spawn(move || {
            let result = git_worker::stage_file(&root, &abs_path).map_err(|e| e.to_string());
            let _ = sender.send(result);
        });

        let panel = self.clone();
        glib::idle_add_local(move || match receiver.try_recv() {
            Ok(Ok(())) => {
                panel.refresh();
                glib::ControlFlow::Break
            }
            Ok(Err(e)) => {
                tracing::error!("git stage failed: {e}");
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });
    }

    /// Unstage a file and refresh.
    pub fn unstage_file(&self, relative_path: &str) {
        let root = match self.project_root.borrow().clone() {
            Some(r) => r,
            None => return,
        };

        let abs_path = root.join(relative_path);
        let (sender, receiver) = std::sync::mpsc::channel::<Result<(), String>>();

        std::thread::spawn(move || {
            let result = git_worker::unstage_file(&root, &abs_path).map_err(|e| e.to_string());
            let _ = sender.send(result);
        });

        let panel = self.clone();
        glib::idle_add_local(move || match receiver.try_recv() {
            Ok(Ok(())) => {
                panel.refresh();
                glib::ControlFlow::Break
            }
            Ok(Err(e)) => {
                tracing::error!("git unstage failed: {e}");
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });
    }

    /// Discard working-tree changes to a file (with confirmation) and refresh.
    pub fn discard_file(&self, relative_path: &str, parent_window: Option<&gtk4::Window>) {
        let root = match self.project_root.borrow().clone() {
            Some(r) => r,
            None => return,
        };

        let abs_path = root.join(relative_path);
        let panel = self.clone();
        let path_display = relative_path.to_string();

        // Show confirmation dialog.
        let dialog = gtk4::AlertDialog::builder()
            .message("Discard Changes")
            .detail(format!(
                "Are you sure you want to discard changes to \"{path_display}\"?\n\nThis cannot be undone."
            ))
            .buttons(["Cancel", "Discard"])
            .cancel_button(0)
            .default_button(1)
            .modal(true)
            .build();

        let window = parent_window.cloned();
        dialog.choose(window.as_ref(), gio::Cancellable::NONE, move |result| {
            // Button index 1 = "Discard"
            if result == Ok(1) {
                let (sender, receiver) = std::sync::mpsc::channel::<Result<(), String>>();

                let abs = abs_path.clone();
                let r = root.clone();
                std::thread::spawn(move || {
                    let result = git_worker::discard_file(&r, &abs).map_err(|e| e.to_string());
                    let _ = sender.send(result);
                });

                let p = panel.clone();
                glib::idle_add_local(move || match receiver.try_recv() {
                    Ok(Ok(())) => {
                        p.refresh();
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        tracing::error!("git discard failed: {e}");
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
                });
            }
        });
    }

    /// Stage all unstaged changes and refresh.
    pub fn stage_all(&self) {
        let root = match self.project_root.borrow().clone() {
            Some(r) => r,
            None => return,
        };

        let (sender, receiver) = std::sync::mpsc::channel::<Result<(), String>>();

        std::thread::spawn(move || {
            let result = git_worker::stage_all(&root).map_err(|e| e.to_string());
            let _ = sender.send(result);
        });

        let panel = self.clone();
        glib::idle_add_local(move || match receiver.try_recv() {
            Ok(Ok(())) => {
                panel.refresh();
                glib::ControlFlow::Break
            }
            Ok(Err(e)) => {
                tracing::error!("git stage all failed: {e}");
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });
    }

    /// Unstage all staged changes and refresh.
    pub fn unstage_all(&self) {
        let root = match self.project_root.borrow().clone() {
            Some(r) => r,
            None => return,
        };

        let (sender, receiver) = std::sync::mpsc::channel::<Result<(), String>>();

        std::thread::spawn(move || {
            let result = git_worker::unstage_all(&root).map_err(|e| e.to_string());
            let _ = sender.send(result);
        });

        let panel = self.clone();
        glib::idle_add_local(move || match receiver.try_recv() {
            Ok(Ok(())) => {
                panel.refresh();
                glib::ControlFlow::Break
            }
            Ok(Err(e)) => {
                tracing::error!("git unstage all failed: {e}");
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });
    }

    /// Commit staged changes with the given message and refresh.
    pub fn commit(&self, message: &str) {
        let root = match self.project_root.borrow().clone() {
            Some(r) => r,
            None => return,
        };

        let msg = message.to_string();
        let (sender, receiver) = std::sync::mpsc::channel::<Result<(), String>>();

        std::thread::spawn(move || {
            let result = git_worker::commit(&root, &msg).map_err(|e| e.to_string());
            let _ = sender.send(result);
        });

        let panel = self.clone();
        glib::idle_add_local(move || match receiver.try_recv() {
            Ok(Ok(())) => {
                panel.refresh();
                glib::ControlFlow::Break
            }
            Ok(Err(e)) => {
                tracing::error!("git commit failed: {e}");
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });
    }

    /// Set the project root and trigger an initial refresh.
    pub fn set_project_root(&self, root: &Path) {
        self.project_root.replace(Some(root.to_path_buf()));
        self.refresh();
    }

    /// Get the current project root.
    pub fn project_root(&self) -> Option<PathBuf> {
        self.project_root.borrow().clone()
    }

    /// Set the callback for opening a diff view.
    pub fn set_on_open_diff<F: Fn(&Path, bool) + 'static>(&self, f: F) {
        self.on_open_diff.replace(Some(Box::new(f)));
    }

    /// Set a callback that fires after every status refresh with the total
    /// number of changed files (staged + unstaged).
    pub fn set_on_status_refreshed<F: Fn(usize) + 'static>(&self, f: F) {
        self.on_status_refreshed.replace(Some(Box::new(f)));
    }

    /// The container widget.
    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    /// Wire the action buttons in list items to the panel's stage/unstage/discard
    /// methods. This must be called after the panel is fully constructed because
    /// it needs a reference to the parent window for the discard confirmation dialog.
    pub fn wire_action_buttons(&self, window: &gtk4::ApplicationWindow) {
        let panel = self.clone();
        let win = window.clone();

        // We handle button clicks via an event controller on the list view that
        // checks if the click target is one of our action buttons.
        let controller = gtk4::GestureClick::new();
        controller.set_button(1);
        controller.set_propagation_phase(gtk4::PropagationPhase::Capture);

        controller.connect_pressed(move |gesture, _, x, y| {
            let widget = match gesture.widget() {
                Some(w) => w,
                None => return,
            };

            // Walk up from the click coordinates to find if we hit a button.
            if let Some(target) = widget.pick(x, y, gtk4::PickFlags::DEFAULT) {
                let mut current = Some(target);
                while let Some(ref w) = current {
                    if let Some(btn) = w.downcast_ref::<gtk4::Button>() {
                        let file_path = btn.widget_name().to_string();
                        if file_path.is_empty() {
                            return;
                        }

                        let tooltip = btn.tooltip_text().unwrap_or_default();
                        match tooltip.as_str() {
                            "Stage" => {
                                gesture.set_state(gtk4::EventSequenceState::Claimed);
                                panel.stage_file(&file_path);
                                return;
                            }
                            "Unstage" => {
                                gesture.set_state(gtk4::EventSequenceState::Claimed);
                                panel.unstage_file(&file_path);
                                return;
                            }
                            "Discard Changes" => {
                                gesture.set_state(gtk4::EventSequenceState::Claimed);
                                panel.discard_file(
                                    &file_path,
                                    Some(win.upcast_ref::<gtk4::Window>()),
                                );
                                return;
                            }
                            "Stage All" => {
                                gesture.set_state(gtk4::EventSequenceState::Claimed);
                                panel.stage_all();
                                return;
                            }
                            "Unstage All" => {
                                gesture.set_state(gtk4::EventSequenceState::Claimed);
                                panel.unstage_all();
                                return;
                            }
                            _ => {}
                        }
                    }
                    current = w.parent();
                }
            }
        });

        self.list_view.add_controller(controller);
    }
}

/// Find a child widget by its widget name.
fn find_child_by_name<T: IsA<gtk4::Widget>>(parent: &gtk4::Box, name: &str) -> Option<T> {
    let mut child = parent.first_child();
    while let Some(ref w) = child {
        if w.widget_name() == name {
            return w.clone().downcast::<T>().ok();
        }
        child = w.next_sibling();
    }
    None
}

/// Remove all children from a box.
fn clear_box(container: &gtk4::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
