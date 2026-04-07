//! RlineWindow — main application window with three-pane layout.

use std::cell::RefCell;
use std::path::PathBuf;

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;

use crate::editor::SplitContainer;
use crate::file_browser::FileBrowserPanel;
use crate::git::GitPanel;
use crate::menu;
use crate::search::ProjectSearchPanel;
use crate::status_bar::StatusBar;
use crate::terminal::TerminalPane;

// ── Implementation ──────────────────────────────────────────────

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct RlineWindow {
        pub split_container: RefCell<Option<SplitContainer>>,
        pub file_browser: RefCell<Option<FileBrowserPanel>>,
        pub search_panel: RefCell<Option<ProjectSearchPanel>>,
        pub git_panel: RefCell<Option<GitPanel>>,
        pub terminal_pane: RefCell<Option<TerminalPane>>,
        pub status_bar: RefCell<Option<StatusBar>>,
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
            window.setup_key_controller();
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
            .property("default-width", 1400)
            .property("default-height", 900)
            .build()
    }

    /// Build the three-column layout with nested Paned widgets.
    fn setup_layout(&self) {
        let imp = self.imp();
        self.set_title(Some("rline"));

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
        let split_container = SplitContainer::new();
        let terminal_pane = TerminalPane::new();

        let middle_paned = gtk4::Paned::new(gtk4::Orientation::Vertical);
        middle_paned.set_start_child(Some(split_container.widget()));
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

        // ── Status bar at the very bottom ──
        let status_bar = StatusBar::new();

        let root_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        root_box.append(&outer_paned);
        root_box.append(status_bar.widget());
        outer_paned.set_vexpand(true);

        self.set_child(Some(&root_box));

        // ── Apply initial theme ──
        let settings = rline_config::EditorSettings::load().unwrap_or_default();
        crate::theming::apply_app_theme(&settings.theme);

        // ── Wire cross-component callbacks ──
        self.wire_file_browser(
            &file_browser,
            &split_container,
            &terminal_pane,
            &search_panel,
            &git_panel,
            &status_bar,
        );
        self.wire_git_panel(&git_panel, &split_container);

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
        imp.split_container.replace(Some(split_container));
        imp.file_browser.replace(Some(file_browser.clone()));
        imp.search_panel.replace(Some(search_panel));
        imp.git_panel.replace(Some(git_panel));
        imp.terminal_pane.replace(Some(terminal_pane));
        imp.status_bar.replace(Some(status_bar));
        imp.left_stack.replace(Some(stack));

        // ── Restore last project on startup ──
        if settings.open_last_project {
            if let Some(ref last_path) = settings.last_project_path {
                let path = std::path::PathBuf::from(last_path);
                if path.is_dir() {
                    tracing::info!("restoring last project: {}", path.display());
                    file_browser.set_root(&path);
                    self.set_title(Some(&format!("rline - {}", path.display())));
                    if let Some(ref sb) = *imp.status_bar.borrow() {
                        sb.set_project_root(&path);
                    }
                }
            }
        }

        // Spawn the initial terminal *after* restoring the project root so it
        // opens in the project directory rather than $HOME.
        if let Some(ref tp) = *imp.terminal_pane.borrow() {
            tp.add_terminal(None);
        }
    }

    /// Wire the file browser's open callback to the editor pane, and project root
    /// changes to terminal, search, git, and status bar.
    fn wire_file_browser(
        &self,
        file_browser: &FileBrowserPanel,
        split_container: &SplitContainer,
        terminal_pane: &TerminalPane,
        search_panel: &ProjectSearchPanel,
        git_panel: &GitPanel,
        status_bar: &StatusBar,
    ) {
        // Single-click opens file in editor
        let sc = split_container.clone();
        file_browser.set_on_open_file(move |path| {
            if let Err(e) = sc.open_file(path) {
                tracing::error!("failed to open file: {e}");
            }
        });

        // Reveal the active file in the browser when switching editor tabs,
        // and update the status bar with the new buffer for blame tracking.
        let fb = file_browser.clone();
        let sb = status_bar.clone();
        let sc_for_status = split_container.clone();
        split_container.set_on_active_file_changed(move |path| {
            if let Some(ref p) = path {
                fb.reveal_file(p);
            }
            // Defer status bar update: during switch-page, current_page() still
            // returns the OLD page, so current_editor_tab() would yield the wrong
            // buffer. By the time the idle callback runs the switch is complete.
            let sb_clone = sb.clone();
            let sc_clone = sc_for_status.clone();
            let path_clone = path.clone();
            glib::idle_add_local_once(move || {
                let tab = sc_clone.current_editor_tab();
                let buffer = tab.as_ref().map(|t| t.buffer().clone());
                sb_clone.set_active_editor(buffer, path_clone);
            });
        });

        // Search result opens file at line
        let sc_search = split_container.clone();
        search_panel.set_on_open_file_at_line(move |path, line| {
            if let Err(e) = sc_search.open_file_at_line(path, line) {
                tracing::error!("failed to open file at line: {e}");
            }
        });

        // Project root changes update terminal + search + git + status bar + persist last project
        let tp = terminal_pane.clone();
        let sp = search_panel.clone();
        let gp = git_panel.clone();
        let sb_root = status_bar.clone();
        file_browser.set_on_project_root_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |root| {
                tracing::info!("project root changed to: {}", root.display());
                window.imp().project_root.replace(Some(root.to_path_buf()));
                window.set_title(Some(&format!("rline - {}", root.display())));
                tp.set_default_directory(root);
                sp.set_project_root(root);
                gp.set_project_root(root);
                sb_root.set_project_root(root);

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
    fn wire_git_panel(&self, git_panel: &GitPanel, split_container: &SplitContainer) {
        let sc = split_container.clone();
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

            let sc_clone = sc.clone();
            let path_for_ui = path.clone();
            glib::idle_add_local(move || match receiver.try_recv() {
                Ok(Ok(diff)) => {
                    if let Err(e) = sc_clone.open_diff(&path_for_ui, &diff) {
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

        // win.save-file (Ctrl+S)
        let action_save = gio::ActionEntry::builder("save-file")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    let sc = window.imp().split_container.borrow().clone();
                    if let Some(ref sc) = sc {
                        sc.save_current_tab();
                    }
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
                    let sc = window.imp().split_container.borrow().clone();
                    if let Some(ref sc) = sc {
                        sc.show_find_bar(false);
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
                    let sc = window.imp().split_container.borrow().clone();
                    if let Some(ref sc) = sc {
                        sc.show_find_bar(true);
                    }
                }
            ))
            .build();

        // win.split-editor (Ctrl+\)
        let action_split = gio::ActionEntry::builder("split-editor")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    tracing::info!("split-editor action triggered");
                    let sc = window.imp().split_container.borrow().clone();
                    if let Some(ref sc) = sc {
                        sc.split_vertical();
                    }
                }
            ))
            .build();

        self.add_action_entries([
            action_open,
            action_save,
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
            action_split,
        ]);
    }

    /// Install a window-level key controller for shortcuts that may be
    /// swallowed by child widgets (sourceview, VTE) before the accelerator
    /// system sees them.
    fn setup_key_controller(&self) {
        let key_ctl = gtk4::EventControllerKey::new();
        key_ctl.set_propagation_phase(gtk4::PropagationPhase::Capture);
        key_ctl.connect_key_pressed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[upgrade_or]
            gtk4::glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                if key == gtk4::gdk::Key::backslash
                    && modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK)
                {
                    let sc = window.imp().split_container.borrow().clone();
                    if let Some(ref sc) = sc {
                        sc.split_vertical();
                    }
                    return gtk4::glib::Propagation::Stop;
                }
                gtk4::glib::Propagation::Proceed
            }
        ));
        self.add_controller(key_ctl);
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
                            if let Some(ref sc) = *imp.split_container.borrow() {
                                if let Err(e) = sc.open_file(&path) {
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
        if let Some(ref sc) = *imp.split_container.borrow() {
            sc.close_current_tab();
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
        let sc = imp.split_container.borrow().clone();
        dialog.set_on_file_selected(move |path| {
            if let Some(ref sc) = sc {
                if let Err(e) = sc.open_file(path) {
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
        let split_container = imp.split_container.borrow().clone();
        let terminal_pane = imp.terminal_pane.borrow().clone();
        let dialog = crate::editor::SettingsDialog::new(self.upcast_ref(), move |settings| {
            // Always update app-wide chrome, even if no editor tabs are open
            crate::theming::apply_app_theme(&settings.theme);
            if let Some(ref sc) = split_container {
                sc.apply_settings(&settings);
            }
            if let Some(ref terminal) = terminal_pane {
                terminal.apply_settings(&settings);
            }
        });
        dialog.present();
    }
}
