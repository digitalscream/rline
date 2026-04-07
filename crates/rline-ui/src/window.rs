//! RlineWindow — main application window with three-pane layout.

use std::cell::RefCell;
use std::path::PathBuf;

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;

use crate::editor::EditorPane;
use crate::file_browser::FileBrowserPanel;
use crate::git::GitPanel;
use crate::menu;
use crate::search::ProjectSearchPanel;
use crate::terminal::TerminalPane;

// ── Implementation ──────────────────────────────────────────────

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct RlineWindow {
        pub editor_pane: RefCell<Option<EditorPane>>,
        pub file_browser: RefCell<Option<FileBrowserPanel>>,
        pub search_panel: RefCell<Option<ProjectSearchPanel>>,
        pub git_panel: RefCell<Option<GitPanel>>,
        pub terminal_pane: RefCell<Option<TerminalPane>>,
        pub left_stack: RefCell<Option<gtk4::Stack>>,
        pub project_root: RefCell<Option<PathBuf>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RlineWindow {
        const NAME: &'static str = "RlineWindow";
        type Type = super::RlineWindow;
        type ParentType = gtk4::ApplicationWindow;
    }

    impl ObjectImpl for RlineWindow {
        fn constructed(&self) {
            self.parent_constructed();
            let window = self.obj();
            window.setup_layout();
            window.setup_actions();
        }
    }

    impl WidgetImpl for RlineWindow {}
    impl WindowImpl for RlineWindow {}
    impl ApplicationWindowImpl for RlineWindow {}
}

// ── Public type ─────────────────────────────────────────────────

glib::wrapper! {
    /// The main rline editor window containing three resizable panes.
    pub struct RlineWindow(ObjectSubclass<imp::RlineWindow>)
        @extends gtk4::ApplicationWindow, gtk4::Window, gtk4::Widget,
        @implements gio::ActionGroup, gio::ActionMap,
                    gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget,
                    gtk4::Native, gtk4::Root, gtk4::ShortcutManager;
}

impl RlineWindow {
    /// Create a new rline window attached to the given application.
    pub fn new(app: &gtk4::Application) -> Self {
        glib::Object::builder()
            .property("application", app)
            .property("title", "rline")
            .property("default-width", 1400)
            .property("default-height", 900)
            .build()
    }

    /// Build the three-column layout with nested Paned widgets.
    fn setup_layout(&self) {
        let imp = self.imp();

        // ── Header bar with custom window controls ──
        let header = gtk4::HeaderBar::new();
        header.set_show_title_buttons(false);

        let menu_button = gtk4::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu::build_app_menu())
            .build();
        // Custom window control buttons for precise size control
        let close_btn = gtk4::Button::from_icon_name("window-close-symbolic");
        close_btn.add_css_class("flat");
        close_btn.add_css_class("windowcontrol");
        close_btn.set_valign(gtk4::Align::Center);
        close_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.close()
        ));

        let max_btn = gtk4::Button::from_icon_name("window-maximize-symbolic");
        max_btn.add_css_class("flat");
        max_btn.add_css_class("windowcontrol");
        max_btn.set_valign(gtk4::Align::Center);
        max_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.set_maximized(!win.is_maximized())
        ));

        let min_btn = gtk4::Button::from_icon_name("window-minimize-symbolic");
        min_btn.add_css_class("flat");
        min_btn.add_css_class("windowcontrol");
        min_btn.set_valign(gtk4::Align::Center);
        min_btn.connect_clicked(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.minimize()
        ));

        let controls_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        controls_box.set_valign(gtk4::Align::Center);
        controls_box.append(&min_btn);
        controls_box.append(&max_btn);
        controls_box.append(&close_btn);
        header.pack_end(&controls_box);
        header.pack_end(&menu_button);

        self.set_titlebar(Some(&header));

        // ── Left pane: Stack with three tabs ──
        let stack = gtk4::Stack::new();
        stack.set_transition_type(gtk4::StackTransitionType::SlideUpDown);

        let file_browser = FileBrowserPanel::new();
        let search_panel = ProjectSearchPanel::new();
        let git_panel = GitPanel::new();

        stack.add_titled(file_browser.widget(), Some("files"), "Files");
        stack.add_titled(git_panel.widget(), Some("git"), "Git");
        stack.add_titled(search_panel.widget(), Some("search"), "Search");

        let switcher = gtk4::StackSwitcher::new();
        switcher.set_stack(Some(&stack));

        let left_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        left_box.append(&switcher);
        left_box.append(&stack);
        stack.set_vexpand(true);

        // ── Middle pane: editor (top) + terminal (bottom) ──
        let editor_pane = EditorPane::new();
        let terminal_pane = TerminalPane::new();

        let middle_paned = gtk4::Paned::new(gtk4::Orientation::Vertical);
        middle_paned.set_start_child(Some(editor_pane.widget()));
        middle_paned.set_end_child(Some(terminal_pane.widget()));
        middle_paned.set_resize_start_child(true);
        middle_paned.set_resize_end_child(true);
        middle_paned.set_shrink_start_child(false);
        middle_paned.set_shrink_end_child(false);
        middle_paned.set_position(550);

        // ── Right pane: AI placeholder ──
        let right_label = gtk4::Label::new(Some("AI Agent (coming soon)"));
        right_label.set_vexpand(true);
        right_label.set_hexpand(false);
        let right_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        right_box.append(&right_label);
        right_box.set_width_request(250);

        // ── Assemble: left | middle | right ──
        let inner_paned = gtk4::Paned::new(gtk4::Orientation::Horizontal);
        inner_paned.set_start_child(Some(&middle_paned));
        inner_paned.set_end_child(Some(&right_box));
        inner_paned.set_resize_start_child(true);
        inner_paned.set_resize_end_child(false);
        inner_paned.set_shrink_start_child(false);
        inner_paned.set_shrink_end_child(false);

        let outer_paned = gtk4::Paned::new(gtk4::Orientation::Horizontal);
        outer_paned.set_start_child(Some(&left_box));
        outer_paned.set_end_child(Some(&inner_paned));
        outer_paned.set_resize_start_child(false);
        outer_paned.set_resize_end_child(true);
        outer_paned.set_shrink_start_child(false);
        outer_paned.set_shrink_end_child(false);
        outer_paned.set_position(250);

        self.set_child(Some(&outer_paned));

        // ── Apply initial theme ──
        let settings = rline_config::EditorSettings::load().unwrap_or_default();
        crate::theming::apply_app_theme(&settings.theme);

        // ── Wire cross-component callbacks ──
        self.wire_file_browser(
            &file_browser,
            &editor_pane,
            &terminal_pane,
            &search_panel,
            &git_panel,
        );
        self.wire_git_panel(&git_panel, &editor_pane);

        // Wire action buttons (needs the window reference for confirmation dialogs).
        git_panel.wire_action_buttons(self.upcast_ref::<gtk4::ApplicationWindow>());

        // Auto-refresh git panel when the tab becomes visible.
        let gp_for_stack = git_panel.clone();
        stack.connect_notify_local(Some("visible-child-name"), move |stack, _| {
            if let Some(name) = stack.visible_child_name() {
                if name == "git" {
                    gp_for_stack.refresh();
                }
            }
        });

        // ── Store references ──
        imp.editor_pane.replace(Some(editor_pane));
        imp.file_browser.replace(Some(file_browser.clone()));
        imp.search_panel.replace(Some(search_panel));
        imp.git_panel.replace(Some(git_panel));
        imp.terminal_pane.replace(Some(terminal_pane));
        imp.left_stack.replace(Some(stack));

        // ── Restore last project on startup ──
        if settings.open_last_project {
            if let Some(ref last_path) = settings.last_project_path {
                let path = std::path::PathBuf::from(last_path);
                if path.is_dir() {
                    tracing::info!("restoring last project: {}", path.display());
                    file_browser.set_root(&path);
                }
            }
        }
    }

    /// Wire the file browser's open callback to the editor pane, and project root
    /// changes to terminal, search, and git.
    fn wire_file_browser(
        &self,
        file_browser: &FileBrowserPanel,
        editor_pane: &EditorPane,
        terminal_pane: &TerminalPane,
        search_panel: &ProjectSearchPanel,
        git_panel: &GitPanel,
    ) {
        // Single-click opens file in editor
        let ep = editor_pane.clone();
        file_browser.set_on_open_file(move |path| {
            if let Err(e) = ep.open_file(path) {
                tracing::error!("failed to open file: {e}");
            }
        });

        // Search result opens file at line
        let ep_search = editor_pane.clone();
        search_panel.set_on_open_file_at_line(move |path, line| {
            if let Err(e) = ep_search.open_file_at_line(path, line) {
                tracing::error!("failed to open file at line: {e}");
            }
        });

        // Project root changes update terminal + search + git + persist last project
        let tp = terminal_pane.clone();
        let sp = search_panel.clone();
        let gp = git_panel.clone();
        file_browser.set_on_project_root_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |root| {
                tracing::info!("project root changed to: {}", root.display());
                window.imp().project_root.replace(Some(root.to_path_buf()));
                tp.set_default_directory(root);
                sp.set_project_root(root);
                gp.set_project_root(root);

                // Persist last project path for next startup
                if let Ok(mut settings) = rline_config::EditorSettings::load() {
                    settings.last_project_path = Some(root.display().to_string());
                    if let Err(e) = settings.save() {
                        tracing::warn!("failed to save last project path: {e}");
                    }
                }
            }
        ));
    }

    /// Wire the git panel's diff callback to the editor pane.
    fn wire_git_panel(&self, git_panel: &GitPanel, editor_pane: &EditorPane) {
        let ep = editor_pane.clone();
        let gp = git_panel.clone();

        git_panel.set_on_open_diff(move |path, is_staged| {
            let root = match gp.project_root() {
                Some(r) => r,
                None => return,
            };
            let path = path.to_path_buf();
            let (sender, receiver) =
                std::sync::mpsc::channel::<Result<crate::git::git_worker::FileDiff, String>>();

            let path_for_thread = path.clone();
            std::thread::spawn(move || {
                let result =
                    crate::git::git_worker::get_file_diff(&root, &path_for_thread, is_staged)
                        .map_err(|e| e.to_string());
                let _ = sender.send(result);
            });

            let ep_clone = ep.clone();
            let path_for_ui = path.clone();
            glib::idle_add_local(move || match receiver.try_recv() {
                Ok(Ok(diff)) => {
                    if let Err(e) = ep_clone.open_diff(&path_for_ui, &diff) {
                        tracing::error!("failed to open diff: {e}");
                    }
                    glib::ControlFlow::Break
                }
                Ok(Err(e)) => {
                    tracing::error!("git diff failed: {e}");
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            });
        });
    }

    /// Set up window-level actions for keyboard shortcuts.
    fn setup_actions(&self) {
        // win.open-file (Ctrl+O)
        let action_open = gio::ActionEntry::builder("open-file")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    window.action_open_file();
                }
            ))
            .build();

        // win.close-tab (Ctrl+W)
        let action_close = gio::ActionEntry::builder("close-tab")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    window.action_close_tab();
                }
            ))
            .build();

        // win.quick-open (Ctrl+P)
        let action_quick_open = gio::ActionEntry::builder("quick-open")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    window.action_quick_open();
                }
            ))
            .build();

        // win.project-search (Ctrl+Shift+F)
        let action_search = gio::ActionEntry::builder("project-search")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    window.action_project_search();
                }
            ))
            .build();

        // win.show-settings
        let action_settings = gio::ActionEntry::builder("show-settings")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    window.action_show_settings();
                }
            ))
            .build();

        // win.quit-app (Ctrl+Q)
        let action_quit = gio::ActionEntry::builder("quit-app")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    if let Some(app) = window.application() {
                        app.quit();
                    }
                }
            ))
            .build();

        // win.show-git (Ctrl+Shift+G)
        let action_show_git = gio::ActionEntry::builder("show-git")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    let stack = window.imp().left_stack.borrow().clone();
                    if let Some(ref stack) = stack {
                        stack.set_visible_child_name("git");
                    }
                }
            ))
            .build();

        // win.show-files (Ctrl+Shift+E)
        let action_show_files = gio::ActionEntry::builder("show-files")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    let stack = window.imp().left_stack.borrow().clone();
                    if let Some(ref stack) = stack {
                        stack.set_visible_child_name("files");
                    }
                }
            ))
            .build();

        // win.focus-terminal (Ctrl+Shift+W)
        let action_focus_terminal = gio::ActionEntry::builder("focus-terminal")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    window.action_focus_terminal();
                }
            ))
            .build();

        // win.find (Ctrl+F)
        let action_find = gio::ActionEntry::builder("find")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    let editor = window.imp().editor_pane.borrow().clone();
                    if let Some(ref editor) = editor {
                        editor.show_find_bar(false);
                    }
                }
            ))
            .build();

        // win.find-replace (Ctrl+H)
        let action_find_replace = gio::ActionEntry::builder("find-replace")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    let editor = window.imp().editor_pane.borrow().clone();
                    if let Some(ref editor) = editor {
                        editor.show_find_bar(true);
                    }
                }
            ))
            .build();

        self.add_action_entries([
            action_open,
            action_close,
            action_quick_open,
            action_search,
            action_settings,
            action_quit,
            action_show_git,
            action_show_files,
            action_focus_terminal,
            action_find,
            action_find_replace,
        ]);
    }

    fn action_open_file(&self) {
        let dialog = gtk4::FileDialog::builder()
            .title("Open File")
            .modal(true)
            .build();

        dialog.open(
            Some(self),
            gio::Cancellable::NONE,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = file.path() {
                            let imp = window.imp();
                            if let Some(ref editor) = *imp.editor_pane.borrow() {
                                if let Err(e) = editor.open_file(&path) {
                                    tracing::error!("failed to open file: {e}");
                                }
                            }
                        }
                    }
                }
            ),
        );
    }

    fn action_close_tab(&self) {
        let imp = self.imp();
        if let Some(ref editor) = *imp.editor_pane.borrow() {
            editor.close_current_tab();
        }
    }

    fn action_quick_open(&self) {
        let imp = self.imp();
        let project_root = imp.project_root.borrow().clone();
        tracing::debug!("quick-open: project_root = {:?}", project_root);
        let root = match project_root {
            Some(r) => r,
            None => {
                tracing::info!("quick-open: no project root set, ignoring Ctrl+P");
                return;
            }
        };

        let dialog = crate::search::QuickOpenDialog::new(self.upcast_ref(), &root);
        let ep = imp.editor_pane.borrow().clone();
        dialog.set_on_file_selected(move |path| {
            if let Some(ref editor) = ep {
                if let Err(e) = editor.open_file(path) {
                    tracing::error!("failed to open file: {e}");
                }
            }
        });
        dialog.present();
    }

    fn action_focus_terminal(&self) {
        let imp = self.imp();
        if let Some(ref terminal) = *imp.terminal_pane.borrow() {
            terminal.focus_current();
        }
    }

    fn action_project_search(&self) {
        let imp = self.imp();
        if let Some(ref stack) = *imp.left_stack.borrow() {
            stack.set_visible_child_name("search");
        }
        if let Some(ref search) = *imp.search_panel.borrow() {
            search.focus_entry();
        }
    }

    fn action_show_settings(&self) {
        let imp = self.imp();
        let editor_pane = imp.editor_pane.borrow().clone();
        let terminal_pane = imp.terminal_pane.borrow().clone();
        let dialog = crate::editor::SettingsDialog::new(self.upcast_ref(), move |settings| {
            // Always update app-wide chrome, even if no editor tabs are open
            crate::theming::apply_app_theme(&settings.theme);
            if let Some(ref editor) = editor_pane {
                editor.apply_settings(&settings);
            }
            if let Some(ref terminal) = terminal_pane {
                terminal.apply_settings(&settings);
            }
        });
        dialog.present();
    }
}
