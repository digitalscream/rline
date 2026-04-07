//! StatusBar — bottom bar showing repository name, branch, and git blame info.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;

use crate::git::git_worker;

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

        // Branch name
        let branch_icon = gtk4::Image::from_icon_name("media-playlist-consecutive-symbolic");
        branch_icon.add_css_class("status-bar-icon");
        let branch_label = gtk4::Label::new(None);
        branch_label.add_css_class("status-bar-label");

        let branch_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        branch_box.append(&branch_icon);
        branch_box.append(&branch_label);

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
            project_root: Rc::new(RefCell::new(None)),
            cursor_handler: Rc::new(RefCell::new(None)),
            pending_blame: Rc::new(RefCell::new(None)),
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
