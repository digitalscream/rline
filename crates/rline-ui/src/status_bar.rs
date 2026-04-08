//! StatusBar — bottom bar showing repository name, branch, and git blame info.
//!
//! The branch label watches `.git/HEAD` via `gio::FileMonitor` and updates
//! automatically when the user switches branches (e.g. from a terminal).
//! Clicking the branch label opens a popover listing recent local branches;
//! double-clicking a branch in the list checks it out.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;

use crate::git::git_worker;

/// Maximum number of branches shown in the switcher popover.
const BRANCH_LIST_LIMIT: usize = 20;

/// The bottom status bar showing repo info and git blame for the current line.
#[derive(Debug, Clone)]
pub struct StatusBar {
    /// The outer container widget.
    container: gtk4::Box,
    /// Label showing the repository name.
    repo_label: gtk4::Label,
    /// Label showing the current branch.
    branch_label: gtk4::Label,
    /// Label showing git blame for the current line.
    blame_label: gtk4::Label,
    /// Current project root.
    project_root: Rc<RefCell<Option<PathBuf>>>,
    /// Signal handler ID for the current buffer's cursor-position notify.
    cursor_handler: Rc<RefCell<Option<(glib::SignalHandlerId, sourceview5::Buffer)>>>,
    /// Pending blame timeout source ID (for debouncing).
    pending_blame: Rc<RefCell<Option<glib::SourceId>>>,
    /// Active file monitor for `.git/HEAD`.
    _head_monitor: Rc<RefCell<Option<gio::FileMonitor>>>,
    /// The popover for branch switching (kept alive so we can show/hide it).
    _branch_popover: gtk4::Popover,
    /// The list box inside the branch popover.
    _branch_list: gtk4::ListBox,
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusBar {
    /// Create a new status bar widget.
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        container.add_css_class("status-bar");
        container.set_margin_start(8);
        container.set_margin_end(8);

        // Repo name
        let repo_icon = gtk4::Image::from_icon_name("folder-symbolic");
        repo_icon.add_css_class("status-bar-icon");
        let repo_label = gtk4::Label::new(None);
        repo_label.add_css_class("status-bar-label");

        let repo_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        repo_box.append(&repo_icon);
        repo_box.append(&repo_label);

        // Branch name — clickable
        let branch_icon = gtk4::Image::from_icon_name("media-playlist-consecutive-symbolic");
        branch_icon.add_css_class("status-bar-icon");
        let branch_label = gtk4::Label::new(None);
        branch_label.add_css_class("status-bar-label");

        let branch_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        branch_box.append(&branch_icon);
        branch_box.append(&branch_label);

        // Build the branch popover
        let branch_list = gtk4::ListBox::new();
        branch_list.set_selection_mode(gtk4::SelectionMode::Single);
        branch_list.add_css_class("branch-list");

        let scroll = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .min_content_height(200)
            .max_content_height(800)
            .min_content_width(700)
            .child(&branch_list)
            .build();

        let branch_popover = gtk4::Popover::new();
        branch_popover.set_child(Some(&scroll));
        branch_popover.set_parent(&branch_box);
        branch_popover.set_autohide(true);
        branch_popover.add_css_class("branch-popover");

        // Click gesture on branch_box to open the popover
        let click = gtk4::GestureClick::new();
        click.set_button(1);
        let popover_for_click = branch_popover.clone();
        let project_root_for_click: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
        let list_for_click = branch_list.clone();
        let pr_click = project_root_for_click.clone();
        click.connect_released(move |_, _, _, _| {
            let root = match pr_click.borrow().clone() {
                Some(r) => r,
                None => return,
            };
            populate_branch_list(&list_for_click, &root);
            popover_for_click.popup();
        });
        branch_box.add_controller(click);

        // Make branch_box look clickable
        branch_box.set_cursor(gtk4::gdk::Cursor::from_name("pointer", None).as_ref());

        // Double-click on a row in the branch list → checkout
        let dbl_click = gtk4::GestureClick::new();
        dbl_click.set_button(1);
        let popover_for_dbl = branch_popover.clone();
        let branch_label_for_dbl = branch_label.clone();
        let list_for_dbl = branch_list.clone();
        let project_root_for_dbl = project_root_for_click.clone();
        dbl_click.connect_released(move |gesture, n_press, x, y| {
            if n_press < 2 {
                return;
            }
            let Some(widget) = gesture.widget() else {
                return;
            };
            let Some(row) = find_row_at_coords(&list_for_dbl, x, y, &widget) else {
                return;
            };

            let branch_name = match row.widget_name().as_str() {
                "" => return,
                name => name.to_string(),
            };

            let root = match project_root_for_dbl.borrow().clone() {
                Some(r) => r,
                None => return,
            };

            let branch_lbl = branch_label_for_dbl.clone();
            let popover = popover_for_dbl.clone();

            let (sender, receiver) = std::sync::mpsc::channel::<Result<(), String>>();
            let branch_for_thread = branch_name.clone();
            std::thread::spawn(move || {
                let result = git_worker::checkout_branch(&root, &branch_for_thread)
                    .map_err(|e| e.to_string());
                let _ = sender.send(result);
            });

            glib::idle_add_local(move || match receiver.try_recv() {
                Ok(Ok(())) => {
                    branch_lbl.set_text(&branch_name);
                    popover.popdown();
                    glib::ControlFlow::Break
                }
                Ok(Err(e)) => {
                    tracing::error!("branch checkout failed: {e}");
                    popover.popdown();
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            });
        });
        branch_list.add_controller(dbl_click);

        // Blame info (right-aligned)
        let blame_label = gtk4::Label::new(None);
        blame_label.add_css_class("status-bar-blame");
        blame_label.set_hexpand(true);
        blame_label.set_halign(gtk4::Align::End);
        blame_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

        container.append(&repo_box);
        container.append(&branch_box);
        container.append(&blame_label);

        Self {
            container,
            repo_label,
            branch_label,
            blame_label,
            project_root: project_root_for_click,
            cursor_handler: Rc::new(RefCell::new(None)),
            pending_blame: Rc::new(RefCell::new(None)),
            _head_monitor: Rc::new(RefCell::new(None)),
            _branch_popover: branch_popover,
            _branch_list: branch_list,
        }
    }

    /// The widget to embed in the window layout.
    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    /// Update the project root, refreshing repo name and branch.
    pub fn set_project_root(&self, root: &Path) {
        self.project_root.replace(Some(root.to_path_buf()));
        self.refresh_repo_info(root);
        self.watch_git_head(root);
    }

    /// Update which editor buffer is active. Connects to cursor-position
    /// changes for git blame updates.
    pub fn set_active_editor(
        &self,
        buffer: Option<sourceview5::Buffer>,
        file_path: Option<PathBuf>,
    ) {
        // Disconnect previous handler
        self.disconnect_cursor_handler();

        // Cancel any pending blame lookup
        if let Some(source_id) = self.pending_blame.borrow_mut().take() {
            source_id.remove();
        }

        let Some(buf) = buffer else {
            self.blame_label.set_text("");
            return;
        };

        let Some(path) = file_path else {
            self.blame_label.set_text("");
            return;
        };

        // Immediately update blame for the current cursor position
        self.update_blame_for_buffer(&buf, &path);

        // Connect to cursor-position changes
        let blame_label = self.blame_label.clone();
        let project_root = self.project_root.clone();
        let pending = self.pending_blame.clone();
        let path_for_signal = path.clone();

        let handler_id = buf.connect_notify_local(Some("cursor-position"), move |buf, _| {
            // Debounce: cancel previous pending blame, schedule new one
            if let Some(source_id) = pending.borrow_mut().take() {
                source_id.remove();
            }

            let blame_lbl = blame_label.clone();
            let root = project_root.clone();
            let file = path_for_signal.clone();
            let pending_clone = pending.clone();

            let iter = buf.iter_at_mark(&buf.get_insert());
            let line = (iter.line() + 1) as u32; // 1-based

            let source_id =
                glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                    // Clear pending marker
                    pending_clone.borrow_mut().take();

                    let root = match root.borrow().clone() {
                        Some(r) => r,
                        None => return,
                    };

                    // Run blame on a background thread
                    let (sender, receiver) =
                        std::sync::mpsc::channel::<Option<git_worker::BlameInfo>>();

                    let file_clone = file.clone();
                    std::thread::spawn(move || {
                        let result = git_worker::get_blame_for_line(&root, &file_clone, line).ok();
                        let _ = sender.send(result);
                    });

                    glib::idle_add_local(move || match receiver.try_recv() {
                        Ok(Some(info)) => {
                            blame_lbl.set_text(&format!(
                                "{}, {} \u{2014} {}",
                                info.author, info.date, info.summary
                            ));
                            glib::ControlFlow::Break
                        }
                        Ok(None) => {
                            blame_lbl.set_text("");
                            glib::ControlFlow::Break
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            blame_lbl.set_text("");
                            glib::ControlFlow::Break
                        }
                    });
                });

            pending.borrow_mut().replace(source_id);
        });

        self.cursor_handler.replace(Some((handler_id, buf)));
    }

    /// Start watching `.git/HEAD` for changes so the branch label updates
    /// when the user switches branches from outside the editor.
    fn watch_git_head(&self, root: &Path) {
        // Drop any previous monitor.
        self._head_monitor.replace(None);

        let git_head = root.join(".git").join("HEAD");
        if !git_head.exists() {
            return;
        }

        let file = gio::File::for_path(&git_head);
        let monitor = match file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("failed to monitor .git/HEAD: {e}");
                return;
            }
        };

        let branch_label = self.branch_label.clone();
        let project_root = self.project_root.clone();

        monitor.connect_changed(move |_monitor, _file, _other, event| {
            if !matches!(
                event,
                gio::FileMonitorEvent::Changed | gio::FileMonitorEvent::Created
            ) {
                return;
            }

            let root = match project_root.borrow().clone() {
                Some(r) => r,
                None => return,
            };

            let label = branch_label.clone();
            let (sender, receiver) = std::sync::mpsc::channel::<Option<String>>();

            std::thread::spawn(move || {
                let branch = git_worker::get_repo_info(&root)
                    .ok()
                    .map(|info| info.branch);
                let _ = sender.send(branch);
            });

            glib::idle_add_local(move || match receiver.try_recv() {
                Ok(Some(branch)) => {
                    label.set_text(&branch);
                    glib::ControlFlow::Break
                }
                Ok(None) => glib::ControlFlow::Break,
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            });
        });

        self._head_monitor.replace(Some(monitor));
    }

    /// Fetch and display repo name and branch from a background thread.
    fn refresh_repo_info(&self, root: &Path) {
        let (sender, receiver) = std::sync::mpsc::channel::<Option<git_worker::RepoInfo>>();

        let root = root.to_path_buf();
        std::thread::spawn(move || {
            let result = git_worker::get_repo_info(&root).ok();
            let _ = sender.send(result);
        });

        let repo_label = self.repo_label.clone();
        let branch_label = self.branch_label.clone();

        glib::idle_add_local(move || match receiver.try_recv() {
            Ok(Some(info)) => {
                repo_label.set_text(&info.name);
                branch_label.set_text(&info.branch);
                glib::ControlFlow::Break
            }
            Ok(None) => {
                repo_label.set_text("");
                branch_label.set_text("");
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });
    }

    /// Run blame immediately for the current cursor position.
    fn update_blame_for_buffer(&self, buf: &sourceview5::Buffer, path: &Path) {
        let iter = buf.iter_at_mark(&buf.get_insert());
        let line = (iter.line() + 1) as u32;

        let root = match self.project_root.borrow().clone() {
            Some(r) => r,
            None => return,
        };

        let blame_label = self.blame_label.clone();
        let file = path.to_path_buf();

        let (sender, receiver) = std::sync::mpsc::channel::<Option<git_worker::BlameInfo>>();

        std::thread::spawn(move || {
            let result = git_worker::get_blame_for_line(&root, &file, line).ok();
            let _ = sender.send(result);
        });

        glib::idle_add_local(move || match receiver.try_recv() {
            Ok(Some(info)) => {
                blame_label.set_text(&format!(
                    "{}, {} \u{2014} {}",
                    info.author, info.date, info.summary
                ));
                glib::ControlFlow::Break
            }
            Ok(None) => {
                blame_label.set_text("");
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                blame_label.set_text("");
                glib::ControlFlow::Break
            }
        });
    }

    /// Disconnect the current cursor-position signal handler, if any.
    fn disconnect_cursor_handler(&self) {
        if let Some((handler_id, buf)) = self.cursor_handler.borrow_mut().take() {
            buf.disconnect(handler_id);
        }
    }
}

/// Populate the branch list box with recent branches fetched from a background thread.
fn populate_branch_list(list: &gtk4::ListBox, root: &Path) {
    // Remove all existing rows.
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    // Add a temporary "Loading..." row.
    let loading = gtk4::Label::new(Some("Loading branches…"));
    loading.set_margin_top(8);
    loading.set_margin_bottom(8);
    list.append(&loading);

    let (sender, receiver) =
        std::sync::mpsc::channel::<Result<Vec<git_worker::BranchInfo>, String>>();

    let root = root.to_path_buf();
    std::thread::spawn(move || {
        let result = git_worker::list_branches(&root, BRANCH_LIST_LIMIT).map_err(|e| e.to_string());
        let _ = sender.send(result);
    });

    let list = list.clone();
    glib::idle_add_local(move || match receiver.try_recv() {
        Ok(Ok(branches)) => {
            // Clear loading row.
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }

            for info in &branches {
                let row = build_branch_row(info);
                list.append(&row);
            }

            if branches.is_empty() {
                let empty = gtk4::Label::new(Some("No local branches"));
                empty.set_margin_top(8);
                empty.set_margin_bottom(8);
                list.append(&empty);
            }

            glib::ControlFlow::Break
        }
        Ok(Err(e)) => {
            tracing::error!("failed to list branches: {e}");
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            let err_label = gtk4::Label::new(Some("Failed to load branches"));
            err_label.set_margin_top(8);
            err_label.set_margin_bottom(8);
            list.append(&err_label);
            glib::ControlFlow::Break
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
    });
}

/// Build a single row widget for a branch entry.
fn build_branch_row(info: &git_worker::BranchInfo) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::new();
    row.set_widget_name(&info.name);

    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    hbox.set_margin_top(4);
    hbox.set_margin_bottom(4);
    hbox.set_margin_start(8);
    hbox.set_margin_end(8);

    // Current branch indicator
    let indicator = if info.is_current {
        gtk4::Label::new(Some("●"))
    } else {
        gtk4::Label::new(Some(" "))
    };
    indicator.set_width_chars(2);

    let name_label = gtk4::Label::new(Some(&info.name));
    name_label.set_xalign(0.0);
    name_label.add_css_class("branch-name");
    if info.is_current {
        name_label.add_css_class("branch-current");
    }

    let summary_label = gtk4::Label::new(Some(&info.last_commit_summary));
    summary_label.set_xalign(0.0);
    summary_label.set_hexpand(true);
    summary_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    summary_label.add_css_class("dim-label");

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    vbox.append(&name_label);
    vbox.append(&summary_label);
    vbox.set_hexpand(true);

    hbox.append(&indicator);
    hbox.append(&vbox);

    row.set_child(Some(&hbox));
    row
}

/// Find the `ListBoxRow` at the given coordinates, translating from the
/// gesture widget's coordinate space to the list box.
fn find_row_at_coords(
    list: &gtk4::ListBox,
    x: f64,
    y: f64,
    gesture_widget: &gtk4::Widget,
) -> Option<gtk4::ListBoxRow> {
    #[allow(deprecated)] // compute_point requires graphene dependency
    let (_, list_y) = gesture_widget.translate_coordinates(list, x, y)?;
    list.row_at_y(list_y as i32)
}
