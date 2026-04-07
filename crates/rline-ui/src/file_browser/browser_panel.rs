//! FileBrowserPanel — directory browser with tree view and context menu.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;

use super::file_node::FileNode;
use super::file_tree;

/// The file browser panel with a browse button and directory tree.
#[derive(Clone)]
pub struct FileBrowserPanel {
    container: gtk4::Box,
    list_view: gtk4::ListView,
    // Callback type alias would obscure the signature for these one-off event handlers
    #[allow(clippy::type_complexity)]
    on_open_file: Rc<RefCell<Option<Box<dyn Fn(&Path)>>>>,
    // Callback type alias would obscure the signature for these one-off event handlers
    #[allow(clippy::type_complexity)]
    on_project_root_changed: Rc<RefCell<Option<Box<dyn Fn(&Path)>>>>,
    project_root: Rc<RefCell<Option<PathBuf>>>,
}

impl std::fmt::Debug for FileBrowserPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileBrowserPanel").finish_non_exhaustive()
    }
}

impl Default for FileBrowserPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl FileBrowserPanel {
    /// Create a new file browser panel.
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

        // Browse button
        let browse_btn = gtk4::Button::with_label("Browse");
        browse_btn.set_margin_top(4);
        browse_btn.set_margin_bottom(4);
        browse_btn.set_margin_start(4);
        browse_btn.set_margin_end(4);
        container.append(&browse_btn);

        // List view (starts empty)
        let selection = gtk4::SingleSelection::new(None::<gtk4::TreeListModel>);
        let list_view =
            gtk4::ListView::new(Some(selection.clone()), None::<gtk4::SignalListItemFactory>);
        list_view.set_vexpand(true);

        let scrolled = gtk4::ScrolledWindow::builder()
            .child(&list_view)
            .vexpand(true)
            .build();
        container.append(&scrolled);

        let panel = Self {
            container,
            list_view: list_view.clone(),
            on_open_file: Rc::new(RefCell::new(None)),
            on_project_root_changed: Rc::new(RefCell::new(None)),
            project_root: Rc::new(RefCell::new(None)),
        };

        // Setup the factory for rendering tree items
        panel.setup_factory();

        // Wire browse button
        let panel_clone = panel.clone();
        browse_btn.connect_clicked(move |btn| {
            let dialog = gtk4::FileDialog::builder()
                .title("Select Project Directory")
                .modal(true)
                .build();

            let window = btn.root().and_downcast::<gtk4::Window>();
            let pc = panel_clone.clone();
            dialog.select_folder(window.as_ref(), gio::Cancellable::NONE, move |result| {
                if let Ok(folder) = result {
                    if let Some(path) = folder.path() {
                        pc.set_root(&path);
                    }
                }
            });
        });

        // Wire single-click to open files via selection change
        let open_cb = panel.on_open_file.clone();
        let lv_for_click = list_view.clone();
        let click_gesture = gtk4::GestureClick::new();
        click_gesture.set_button(1); // Left click
        click_gesture.connect_released(move |_, _, _, _| {
            if let Some(node) = get_selected_node(&lv_for_click) {
                if !node.is_directory() {
                    let path = PathBuf::from(node.path());
                    if let Some(ref cb) = *open_cb.borrow() {
                        cb(&path);
                    }
                }
            }
        });
        list_view.add_controller(click_gesture);

        // Wire right-click context menu
        panel.setup_context_menu();

        panel
    }

    /// Set the root directory and populate the tree.
    pub fn set_root(&self, path: &Path) {
        self.project_root.replace(Some(path.to_path_buf()));

        let tree_model = file_tree::build_tree_list_model(path);
        let selection = gtk4::SingleSelection::new(Some(tree_model));
        self.list_view.set_model(Some(&selection));

        if let Some(ref cb) = *self.on_project_root_changed.borrow() {
            cb(path);
        }
    }

    /// Set the callback invoked when a file is opened.
    pub fn set_on_open_file<F: Fn(&Path) + 'static>(&self, f: F) {
        self.on_open_file.replace(Some(Box::new(f)));
    }

    /// Set the callback invoked when the project root changes.
    pub fn set_on_project_root_changed<F: Fn(&Path) + 'static>(&self, f: F) {
        self.on_project_root_changed.replace(Some(Box::new(f)));
    }

    /// The container widget.
    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    /// Expand the tree to reveal the given file and select it.
    ///
    /// If the file is outside the project root or cannot be found in the tree,
    /// this method does nothing.
    pub fn reveal_file(&self, path: &Path) {
        // Early return if this file is already selected.
        if let Some(node) = get_selected_node(&self.list_view) {
            if PathBuf::from(node.path()) == path {
                return;
            }
        }

        let project_root = self.project_root.borrow().clone();
        let Some(root) = project_root else { return };

        let canon_root = root.canonicalize().unwrap_or(root);
        let canon_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        let relative = match canon_path.strip_prefix(&canon_root) {
            Ok(r) => r,
            Err(_) => return,
        };

        let components: Vec<String> = relative
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => s.to_str().map(|s| s.to_string()),
                _ => None,
            })
            .collect();

        if components.is_empty() {
            return;
        }

        let Some(model) = self.list_view.model() else {
            return;
        };
        let Some(selection) = model.downcast_ref::<gtk4::SingleSelection>() else {
            return;
        };
        let Some(list_model) = selection.model() else {
            return;
        };
        let Some(tree_model) = list_model.downcast_ref::<gtk4::TreeListModel>() else {
            return;
        };

        let mut search_start = 0u32;
        let mut target_depth = 0u32;

        for (i, component) in components.iter().enumerate() {
            let is_last = i == components.len() - 1;
            let n_items = tree_model.n_items();
            let mut found = false;
            let start = search_start;

            for j in start..n_items {
                let Some(tree_row) = tree_model.row(j) else {
                    continue;
                };
                let Some(node) = tree_row.item().and_downcast::<FileNode>() else {
                    continue;
                };

                let depth = tree_row.depth();
                if depth < target_depth && j > start {
                    break;
                }
                if depth != target_depth {
                    continue;
                }

                if node.name() == *component {
                    if is_last {
                        selection.set_selected(j);
                        scroll_to_item(&self.list_view, j);
                        return;
                    }
                    tree_row.set_expanded(true);
                    search_start = j + 1;
                    target_depth += 1;
                    found = true;
                    break;
                }
            }

            if !found {
                return;
            }
        }
    }

    fn setup_factory(&self) {
        let factory = gtk4::SignalListItemFactory::new();

        factory.connect_setup(|_, item| {
            let list_item = match item.downcast_ref::<gtk4::ListItem>() {
                Some(li) => li,
                None => return,
            };

            let expander = gtk4::TreeExpander::new();
            let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
            let icon = gtk4::Image::new();
            let label = gtk4::Label::new(None);
            label.set_halign(gtk4::Align::Start);
            hbox.append(&icon);
            hbox.append(&label);
            expander.set_child(Some(&hbox));

            list_item.set_child(Some(&expander));
        });

        factory.connect_bind(|_, item| {
            let list_item = match item.downcast_ref::<gtk4::ListItem>() {
                Some(li) => li,
                None => return,
            };

            let expander = match list_item.child().and_downcast::<gtk4::TreeExpander>() {
                Some(e) => e,
                None => return,
            };

            let tree_row = match list_item.item().and_downcast::<gtk4::TreeListRow>() {
                Some(r) => r,
                None => return,
            };

            expander.set_list_row(Some(&tree_row));

            let node = match tree_row.item().and_downcast::<FileNode>() {
                Some(n) => n,
                None => return,
            };

            let hbox = match expander.child().and_downcast::<gtk4::Box>() {
                Some(b) => b,
                None => return,
            };

            if let Some(icon) = hbox.first_child().and_downcast::<gtk4::Image>() {
                if node.is_directory() {
                    icon.set_icon_name(Some("folder-symbolic"));
                } else {
                    icon.set_icon_name(Some("text-x-generic-symbolic"));
                }
            }

            if let Some(label) = hbox.last_child().and_downcast::<gtk4::Label>() {
                label.set_text(&node.name());
            }
        });

        self.list_view.set_factory(Some(&factory));
    }

    fn setup_context_menu(&self) {
        let menu_model = gio::Menu::new();
        menu_model.append(Some("Open"), Some("filebrowser.open"));
        menu_model.append(Some("New File"), Some("filebrowser.new-file"));
        menu_model.append(Some("New Folder"), Some("filebrowser.new-folder"));
        menu_model.append(Some("Rename"), Some("filebrowser.rename"));
        menu_model.append(Some("Delete"), Some("filebrowser.delete"));

        let popover = gtk4::PopoverMenu::from_model(Some(&menu_model));
        popover.set_parent(&self.list_view);
        popover.set_has_arrow(false);

        // The node that was right-clicked (None when clicking empty space).
        let right_clicked_node: Rc<RefCell<Option<FileNode>>> = Rc::new(RefCell::new(None));

        // Action group
        let action_group = gio::SimpleActionGroup::new();

        let open_cb = self.on_open_file.clone();
        let rc_open = right_clicked_node.clone();
        let open_action = gio::SimpleAction::new("open", None);
        open_action.connect_activate(move |_, _| {
            if let Some(ref node) = *rc_open.borrow() {
                if !node.is_directory() {
                    let path = PathBuf::from(node.path());
                    if let Some(ref cb) = *open_cb.borrow() {
                        cb(&path);
                    }
                }
            }
        });
        action_group.add_action(&open_action);

        let root_new_file = self.project_root.clone();
        let open_cb_new = self.on_open_file.clone();
        let rc_new_file = right_clicked_node.clone();
        let new_file_action = gio::SimpleAction::new("new-file", None);
        new_file_action.connect_activate(glib::clone!(
            #[weak(rename_to = lv)]
            self.list_view,
            move |_, _| {
                let parent_dir = context_directory_from_node(&rc_new_file.borrow(), &root_new_file);
                if let Some(parent) = parent_dir {
                    let open_cb_for_dialog = open_cb_new.clone();
                    let root_for_refresh = root_new_file.clone();
                    let lv_for_refresh = lv.clone();
                    show_new_entry_dialog(&lv, "New File", &parent, false, move |created_path| {
                        refresh_tree(&root_for_refresh, &lv_for_refresh);
                        if let Some(ref cb) = *open_cb_for_dialog.borrow() {
                            cb(&created_path);
                        }
                    });
                }
            }
        ));
        action_group.add_action(&new_file_action);

        let root_new_folder = self.project_root.clone();
        let rc_new_folder = right_clicked_node.clone();
        let new_folder_action = gio::SimpleAction::new("new-folder", None);
        new_folder_action.connect_activate(glib::clone!(
            #[weak(rename_to = lv)]
            self.list_view,
            move |_, _| {
                let parent_dir =
                    context_directory_from_node(&rc_new_folder.borrow(), &root_new_folder);
                if let Some(parent) = parent_dir {
                    let root_for_refresh = root_new_folder.clone();
                    let lv_for_refresh = lv.clone();
                    show_new_entry_dialog(&lv, "New Folder", &parent, true, move |_| {
                        refresh_tree(&root_for_refresh, &lv_for_refresh);
                    });
                }
            }
        ));
        action_group.add_action(&new_folder_action);

        let root_ref = self.project_root.clone();
        let rc_rename = right_clicked_node.clone();
        let rename_action = gio::SimpleAction::new("rename", None);
        rename_action.connect_activate(glib::clone!(
            #[weak(rename_to = lv_rename)]
            self.list_view,
            move |_, _| {
                if let Some(ref node) = *rc_rename.borrow() {
                    let old_path = PathBuf::from(node.path());
                    let old_name = node.name();

                    let dialog = gtk4::AlertDialog::builder()
                        .message("Rename")
                        .detail(format!("Enter new name for '{old_name}':"))
                        .buttons(["Cancel", "Rename"])
                        .default_button(1)
                        .cancel_button(0)
                        .build();

                    let window = lv_rename.root().and_downcast::<gtk4::Window>();
                    let root_for_refresh = root_ref.clone();
                    let lv_for_refresh = lv_rename.clone();
                    dialog.choose(window.as_ref(), gio::Cancellable::NONE, move |result| {
                        if let Ok(1) = result {
                            show_rename_dialog(
                                &old_path,
                                &old_name,
                                &root_for_refresh,
                                &lv_for_refresh,
                            );
                        }
                    });
                }
            }
        ));
        action_group.add_action(&rename_action);

        let root_del = self.project_root.clone();
        let rc_delete = right_clicked_node.clone();
        let delete_action = gio::SimpleAction::new("delete", None);
        delete_action.connect_activate(glib::clone!(
            #[weak(rename_to = lv_delete)]
            self.list_view,
            move |_, _| {
                if let Some(ref node) = *rc_delete.borrow() {
                    let path = PathBuf::from(node.path());
                    let name = node.name();
                    let is_dir = node.is_directory();

                    let dialog = gtk4::AlertDialog::builder()
                        .message("Delete")
                        .detail(format!("Are you sure you want to delete '{name}'?"))
                        .buttons(["Cancel", "Delete"])
                        .default_button(0)
                        .cancel_button(0)
                        .build();

                    let window = lv_delete.root().and_downcast::<gtk4::Window>();
                    let lv_for_refresh = lv_delete.clone();
                    let root_for_refresh = root_del.clone();
                    dialog.choose(window.as_ref(), gio::Cancellable::NONE, move |result| {
                        if let Ok(1) = result {
                            let remove_result = if is_dir {
                                std::fs::remove_dir_all(&path)
                            } else {
                                std::fs::remove_file(&path)
                            };

                            if let Err(e) = remove_result {
                                tracing::error!("failed to delete {}: {e}", path.display());
                            } else {
                                refresh_tree(&root_for_refresh, &lv_for_refresh);
                            }
                        }
                    });
                }
            }
        ));
        action_group.add_action(&delete_action);

        self.list_view
            .insert_action_group("filebrowser", Some(&action_group));

        // Right-click gesture — identify the node under the cursor before showing
        // the context menu so that actions operate on the right-clicked item.
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3); // Right click
        gesture.connect_pressed(glib::clone!(
            #[weak]
            popover,
            #[weak(rename_to = lv)]
            self.list_view,
            #[strong]
            right_clicked_node,
            move |gesture, _, x, y| {
                right_clicked_node.replace(find_node_at_position(&lv, x, y));

                let point = gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1);
                popover.set_pointing_to(Some(&point));
                popover.popup();
                gesture.set_state(gtk4::EventSequenceState::Claimed);
            }
        ));
        self.list_view.add_controller(gesture);
    }
}

/// Scroll the list view so that the item at `position` is visible.
///
/// Uses the scroll adjustment to estimate row positions. Deferred to an idle
/// callback so the layout has settled after any tree expansion.
fn scroll_to_item(list_view: &gtk4::ListView, position: u32) {
    let lv = list_view.clone();
    glib::idle_add_local_once(move || {
        let Some(scrolled) = lv.parent().and_downcast::<gtk4::ScrolledWindow>() else {
            return;
        };
        let Some(model) = lv.model() else { return };
        let adj = scrolled.vadjustment();
        let total = model.n_items() as f64;
        if total <= 0.0 {
            return;
        }
        let upper = adj.upper();
        let page_size = adj.page_size();
        if upper <= page_size {
            return;
        }
        let row_height = upper / total;
        let target_y = position as f64 * row_height;
        let current = adj.value();
        // Only scroll if the item is outside the visible range.
        if target_y < current || target_y > current + page_size - row_height {
            let value = (target_y - page_size / 2.0).clamp(0.0, (upper - page_size).max(0.0));
            adj.set_value(value);
        }
    });
}

fn get_selected_node(list_view: &gtk4::ListView) -> Option<FileNode> {
    let model = list_view.model()?;
    let selection = model.downcast_ref::<gtk4::SingleSelection>()?;
    let item = selection.selected_item()?;
    let tree_row = item.downcast_ref::<gtk4::TreeListRow>()?;
    tree_row.item().and_downcast::<FileNode>()
}

fn refresh_tree(root_ref: &Rc<RefCell<Option<PathBuf>>>, list_view: &gtk4::ListView) {
    let root = root_ref.borrow().clone();
    if let Some(root_path) = root {
        let tree_model = file_tree::build_tree_list_model(&root_path);
        let selection = gtk4::SingleSelection::new(Some(tree_model));
        list_view.set_model(Some(&selection));
    }
}

/// Walk the widget tree upward from `widget` to find a `TreeExpander`, then extract
/// the `FileNode` from its `TreeListRow`.
fn node_from_widget(widget: &gtk4::Widget) -> Option<FileNode> {
    let mut current: Option<gtk4::Widget> = Some(widget.clone());
    while let Some(w) = current {
        if let Some(expander) = w.downcast_ref::<gtk4::TreeExpander>() {
            let row = expander.list_row()?;
            return row.item().and_downcast::<FileNode>();
        }
        current = w.parent();
    }
    None
}

/// Find the `FileNode` rendered at the given (x, y) coordinates in the list view.
/// Returns `None` when the click lands on empty space.
fn find_node_at_position(list_view: &gtk4::ListView, x: f64, y: f64) -> Option<FileNode> {
    let widget = list_view.pick(x, y, gtk4::PickFlags::DEFAULT)?;
    node_from_widget(&widget)
}

/// Determine the parent directory for a new file/folder based on the right-clicked node.
///
/// If a directory was right-clicked, returns that directory. If a file was right-clicked,
/// returns its parent. Falls back to the project root when clicking empty space.
fn context_directory_from_node(
    node: &Option<FileNode>,
    root_ref: &Rc<RefCell<Option<PathBuf>>>,
) -> Option<PathBuf> {
    if let Some(ref node) = *node {
        let path = PathBuf::from(node.path());
        if node.is_directory() {
            return Some(path);
        }
        return path.parent().map(|p| p.to_path_buf());
    }
    root_ref.borrow().clone()
}

/// Show a dialog to create a new file or folder.
fn show_new_entry_dialog<F: Fn(PathBuf) + 'static>(
    list_view: &gtk4::ListView,
    title: &str,
    parent_dir: &Path,
    is_folder: bool,
    on_created: F,
) {
    let title = title.to_owned();
    let window = gtk4::Window::builder()
        .title(&title)
        .default_width(300)
        .default_height(100)
        .modal(true)
        .build();

    if let Some(parent) = list_view.root().and_downcast::<gtk4::Window>() {
        window.set_transient_for(Some(&parent));
    }

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let entry = gtk4::Entry::new();
    entry.set_placeholder_text(Some(if is_folder {
        "Folder name"
    } else {
        "File name"
    }));
    vbox.append(&entry);

    let btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk4::Align::End);
    let cancel_btn = gtk4::Button::with_label("Cancel");
    let create_btn = gtk4::Button::with_label("Create");
    create_btn.add_css_class("suggested-action");
    btn_box.append(&cancel_btn);
    btn_box.append(&create_btn);
    vbox.append(&btn_box);

    window.set_child(Some(&vbox));

    cancel_btn.connect_clicked(glib::clone!(
        #[weak]
        window,
        move |_| window.close()
    ));

    let parent_dir = parent_dir.to_path_buf();
    create_btn.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        entry,
        move |_| {
            let name = entry.text().to_string();
            if name.is_empty() {
                window.close();
                return;
            }
            let new_path = parent_dir.join(&name);
            let result = if is_folder {
                std::fs::create_dir_all(&new_path)
            } else {
                // Ensure parent directories exist, then create the file
                if let Some(p) = new_path.parent() {
                    if let Err(e) = std::fs::create_dir_all(p) {
                        tracing::error!("failed to create parent dirs: {e}");
                        window.close();
                        return;
                    }
                }
                std::fs::File::create(&new_path).map(|_| ())
            };

            match result {
                Ok(()) => on_created(new_path),
                Err(e) => tracing::error!("failed to create {}: {e}", name),
            }
            window.close();
        }
    ));

    // Allow pressing Enter to confirm
    let create_for_enter = create_btn.clone();
    entry.connect_activate(move |_| {
        create_for_enter.emit_clicked();
    });

    window.present();
    entry.grab_focus();
}

fn show_rename_dialog(
    old_path: &Path,
    old_name: &str,
    root_ref: &Rc<RefCell<Option<PathBuf>>>,
    list_view: &gtk4::ListView,
) {
    let old_name = old_name.to_owned();
    let window = gtk4::Window::builder()
        .title("Rename")
        .default_width(300)
        .default_height(100)
        .modal(true)
        .build();

    if let Some(parent) = list_view.root().and_downcast::<gtk4::Window>() {
        window.set_transient_for(Some(&parent));
    }

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let entry = gtk4::Entry::new();
    entry.set_text(&old_name);
    entry.select_region(0, -1);
    vbox.append(&entry);

    let btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk4::Align::End);
    let cancel_btn = gtk4::Button::with_label("Cancel");
    let rename_btn = gtk4::Button::with_label("Rename");
    rename_btn.add_css_class("suggested-action");
    btn_box.append(&cancel_btn);
    btn_box.append(&rename_btn);
    vbox.append(&btn_box);

    window.set_child(Some(&vbox));

    cancel_btn.connect_clicked(glib::clone!(
        #[weak]
        window,
        move |_| window.close()
    ));

    let old_path_owned = old_path.to_path_buf();
    let root_for_rename = root_ref.clone();
    rename_btn.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[weak]
        entry,
        #[weak(rename_to = lv_for_rename)]
        list_view,
        move |_| {
            let new_name = entry.text().to_string();
            if new_name.is_empty() || new_name == old_name {
                window.close();
                return;
            }
            if let Some(parent) = old_path_owned.parent() {
                let new_path = parent.join(&new_name);
                if let Err(e) = std::fs::rename(&old_path_owned, &new_path) {
                    tracing::error!("failed to rename: {e}");
                } else {
                    refresh_tree(&root_for_rename, &lv_for_rename);
                }
            }
            window.close();
        }
    ));

    window.present();
}
