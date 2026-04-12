//! RlineWindow — main application window with three-pane layout.

use std::cell::RefCell;
use std::path::PathBuf;

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;

use crate::agent::AgentPanel;
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
        pub agent_panel: RefCell<Option<AgentPanel>>,
        pub project_root: RefCell<Option<PathBuf>>,
        /// File monitors watching `.git/index` and `.git/HEAD` for change count updates.
        pub git_monitors: RefCell<Vec<gio::FileMonitor>>,
        /// Periodic timer for polling working-tree changes.
        pub git_timer: RefCell<Option<glib::SourceId>>,
        /// Debounce source for file-monitor–triggered refreshes.
        pub git_count_pending: RefCell<Option<glib::SourceId>>,
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
    impl WindowImpl for RlineWindow {
        fn close_request(&self) -> glib::Propagation {
            let window = self.obj();
            window.save_session();
            self.parent_close_request()
        }
    }
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

        // ── Right pane: AI agent ──
        let agent_panel = AgentPanel::new();
        let right_box = agent_panel.widget().clone();
        right_box.set_width_request(350);

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

        // ── Apply initial theme and font rendering ──
        let settings = rline_config::EditorSettings::load().unwrap_or_default();
        crate::theming::apply_font_rendering(&settings.hint_style);
        crate::theming::apply_app_theme(&settings.theme);

        // ── Wire cross-component callbacks ──
        self.wire_file_browser(
            &file_browser,
            &split_container,
            &terminal_pane,
            &search_panel,
            &git_panel,
            &status_bar,
            &agent_panel,
        );
        self.wire_git_panel(&git_panel, &split_container);
        self.wire_agent_panel(&agent_panel, &split_container, &terminal_pane);

        // Wire action buttons (needs the window reference for confirmation dialogs).
        git_panel.wire_action_buttons(self.upcast_ref::<gtk4::ApplicationWindow>());

        // Update the Git tab title and file browser git status after each refresh.
        let stack_for_count = stack.clone();
        let fb_for_git = file_browser.clone();
        let gp_for_git = git_panel.clone();
        git_panel.set_on_status_refreshed(move |count| {
            if let Some(child) = stack_for_count.child_by_name("git") {
                let page = stack_for_count.page(&child);
                if count > 0 {
                    page.set_title(&format!("Git [{count}]"));
                } else {
                    page.set_title("Git");
                }
            }
            // Update file browser tree with git status colors
            let status_map = gp_for_git.file_status_map();
            fb_for_git.update_git_status(status_map);
        });

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
        imp.agent_panel.replace(Some(agent_panel));

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
                    if let Some(ref ap) = *imp.agent_panel.borrow() {
                        ap.set_project_root(&path);
                    }
                    imp.project_root.replace(Some(path.clone()));
                    self.start_git_change_watcher(&path);
                }
            }
        }

        // Spawn the initial terminal *after* restoring the project root so it
        // opens in the project directory rather than $HOME.
        if let Some(ref tp) = *imp.terminal_pane.borrow() {
            tp.add_terminal(None);
        }

        // Restore previously open editor tabs from the last session.
        if settings.open_last_project {
            self.restore_session();
        }
    }

    /// Wire the file browser's open callback to the editor pane, and project root
    /// changes to terminal, search, git, and status bar.
    #[allow(clippy::too_many_arguments)]
    fn wire_file_browser(
        &self,
        file_browser: &FileBrowserPanel,
        split_container: &SplitContainer,
        terminal_pane: &TerminalPane,
        search_panel: &ProjectSearchPanel,
        git_panel: &GitPanel,
        status_bar: &StatusBar,
        agent_panel: &AgentPanel,
    ) {
        // Single-click opens file in editor
        let sc = split_container.clone();
        file_browser.set_on_open_file(move |path| {
            if let Err(e) = sc.open_file(path) {
                tracing::error!("failed to open file: {e}");
            }
        });

        // Reveal the active file in the browser when switching editor tabs,
        // update the status bar with the new buffer for blame tracking,
        // and refresh the open buffers list.
        let fb = file_browser.clone();
        let fb_for_buffers = file_browser.clone();
        let sb = status_bar.clone();
        let sc_for_status = split_container.clone();
        let sc_for_buffers = split_container.clone();
        split_container.set_on_active_file_changed(move |path| {
            if let Some(ref p) = path {
                fb.reveal_file(p);
            }
            // Defer status bar and buffer list update: during switch-page,
            // current_page() still returns the OLD page. By the time the
            // idle callback runs the switch is complete.
            let sb_clone = sb.clone();
            let sc_clone = sc_for_status.clone();
            let sc_buf_clone = sc_for_buffers.clone();
            let fb_buf_clone = fb_for_buffers.clone();
            let path_clone = path.clone();
            glib::idle_add_local_once(move || {
                let tab = sc_clone.current_editor_tab();
                let buffer = tab.as_ref().map(|t| t.buffer().clone());
                sb_clone.set_active_editor(buffer, path_clone);
                // Refresh open buffers list
                fb_buf_clone.update_open_buffers(&sc_buf_clone.open_buffers());
            });
        });

        // Search result opens file at line
        let sc_search = split_container.clone();
        search_panel.set_on_open_file_at_line(move |path, line| {
            if let Err(e) = sc_search.open_file_at_line(path, line) {
                tracing::error!("failed to open file at line: {e}");
            }
        });

        // Project root changes update terminal + search + git + status bar + agent + persist
        let tp = terminal_pane.clone();
        let sp = search_panel.clone();
        let gp = git_panel.clone();
        let sb_root = status_bar.clone();
        let ap = agent_panel.clone();
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
                ap.set_project_root(root);
                window.start_git_change_watcher(root);

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

    /// Wire the agent panel's diff callback to the editor pane.
    fn wire_agent_panel(
        &self,
        agent_panel: &AgentPanel,
        split_container: &SplitContainer,
        terminal_pane: &crate::terminal::TerminalPane,
    ) {
        let sc = split_container.clone();
        let project_root = self.imp().project_root.clone();

        agent_panel.set_on_open_diff(move |path| {
            let root = match project_root.borrow().clone() {
                Some(r) => r,
                None => return,
            };
            let path = path.to_path_buf();
            let (sender, receiver) =
                std::sync::mpsc::channel::<Result<crate::git::git_worker::FileDiff, String>>();

            let path_for_thread = path.clone();
            std::thread::spawn(move || {
                let result = crate::git::git_worker::get_file_diff(&root, &path_for_thread, false)
                    .map_err(|e| e.to_string());
                let _ = sender.send(result);
            });

            let sc_clone = sc.clone();
            glib::idle_add_local(move || match receiver.try_recv() {
                Ok(Ok(diff)) => {
                    if let Err(e) = sc_clone.open_diff(&path, &diff) {
                        tracing::error!("failed to open agent diff: {e}");
                    }
                    glib::ControlFlow::Break
                }
                Ok(Err(e)) => {
                    tracing::error!("agent diff failed: {e}");
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            });
        });

        // Wire terminal command execution.
        let tp = terminal_pane.clone();
        agent_panel.set_on_terminal_command(move |command, working_dir, timeout_secs, respond| {
            tp.execute_agent_command(command, working_dir, timeout_secs, respond);
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

        // win.show-shortcuts
        let action_shortcuts = gio::ActionEntry::builder("show-shortcuts")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    window.action_show_shortcuts();
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

        // win.trigger-completion (Ctrl+Space)
        let action_trigger_completion = gio::ActionEntry::builder("trigger-completion")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    let sc = window.imp().split_container.borrow().clone();
                    if let Some(ref sc) = sc {
                        sc.trigger_completion();
                    }
                }
            ))
            .build();

        // win.focus-agent (Ctrl+Shift+A)
        let action_focus_agent = gio::ActionEntry::builder("focus-agent")
            .activate(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _, _| {
                    let ap = window.imp().agent_panel.borrow().clone();
                    if let Some(ref ap) = ap {
                        ap.focus_input();
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
            action_shortcuts,
            action_quit,
            action_show_git,
            action_show_files,
            action_focus_terminal,
            action_find,
            action_find_replace,
            action_split,
            action_trigger_completion,
            action_focus_agent,
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

    /// Start watching the git repository for changes and keep the Git tab
    /// content and title badge up to date.
    fn start_git_change_watcher(&self, root: &std::path::Path) {
        let imp = self.imp();

        // Drop old monitors and timer.
        imp.git_monitors.borrow_mut().clear();
        if let Some(timer_id) = imp.git_timer.borrow_mut().take() {
            timer_id.remove();
        }
        if let Some(pending) = imp.git_count_pending.borrow_mut().take() {
            pending.remove();
        }

        // If the project has no git repository, disable the Git tab and bail.
        let git_dir = root.join(".git");
        if let Some(ref stack) = *imp.left_stack.borrow() {
            if let Some(child) = stack.child_by_name("git") {
                let page = stack.page(&child);
                if git_dir.exists() {
                    page.set_title("Git");
                    child.set_sensitive(true);
                } else {
                    page.set_title("Git");
                    child.set_sensitive(false);
                    return;
                }
            }
        }

        // Initial refresh.
        self.refresh_git_panel();

        // Watch .git/index for stage/unstage/commit changes.
        let git_index = root.join(".git").join("index");
        if git_index.exists() {
            let file = gio::File::for_path(&git_index);
            if let Ok(monitor) =
                file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
            {
                monitor.connect_changed(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move |_, _, _, event| {
                        if matches!(
                            event,
                            gio::FileMonitorEvent::Changed | gio::FileMonitorEvent::Created
                        ) {
                            window.schedule_git_refresh();
                        }
                    }
                ));
                imp.git_monitors.borrow_mut().push(monitor);
            }
        }

        // Watch .git/HEAD for branch switches (which change the set of changes).
        let git_head = root.join(".git").join("HEAD");
        if git_head.exists() {
            let file = gio::File::for_path(&git_head);
            if let Ok(monitor) =
                file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
            {
                monitor.connect_changed(glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move |_, _, _, event| {
                        if matches!(
                            event,
                            gio::FileMonitorEvent::Changed | gio::FileMonitorEvent::Created
                        ) {
                            window.schedule_git_refresh();
                        }
                    }
                ));
                imp.git_monitors.borrow_mut().push(monitor);
            }
        }

        // Periodic timer for working-tree changes (every 2 seconds).
        // Also refreshes the open buffers list to keep modified indicators current.
        let timer_id = glib::timeout_add_local(
            std::time::Duration::from_secs(2),
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    window.refresh_git_panel();
                    window.refresh_open_buffers();
                    glib::ControlFlow::Continue
                }
            ),
        );
        imp.git_timer.borrow_mut().replace(timer_id);
    }

    /// Schedule a debounced git panel refresh (300ms) for file-monitor events.
    fn schedule_git_refresh(&self) {
        let imp = self.imp();
        if let Some(pending) = imp.git_count_pending.borrow_mut().take() {
            pending.remove();
        }
        let source_id = glib::timeout_add_local_once(
            std::time::Duration::from_millis(300),
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || {
                    window.imp().git_count_pending.borrow_mut().take();
                    window.refresh_git_panel();
                }
            ),
        );
        imp.git_count_pending.borrow_mut().replace(source_id);
    }

    /// Trigger a git panel refresh. The panel's `on_status_refreshed` callback
    /// (wired in `setup_layout`) updates the tab title with the change count.
    fn refresh_git_panel(&self) {
        if let Some(ref gp) = *self.imp().git_panel.borrow() {
            gp.refresh();
        }
    }

    /// Refresh the open buffers list in the file browser panel.
    fn refresh_open_buffers(&self) {
        let imp = self.imp();
        let buffers = imp
            .split_container
            .borrow()
            .as_ref()
            .map(|sc| sc.open_buffers())
            .unwrap_or_default();
        if let Some(ref fb) = *imp.file_browser.borrow() {
            fb.update_open_buffers(&buffers);
        }
    }

    /// Persist the current session state (open files and split layout) to disk.
    fn save_session(&self) {
        let imp = self.imp();
        if let Some(ref sc) = *imp.split_container.borrow() {
            let state = sc.session_state();
            if let Err(e) = state.save() {
                tracing::warn!("failed to save session state: {e}");
            }
        }
    }

    /// Restore previously open files from the saved session state.
    fn restore_session(&self) {
        let imp = self.imp();
        let session = rline_config::SessionState::load();

        // Only restore if there are files to open.
        if session.left.files.is_empty() {
            return;
        }

        if let Some(ref sc) = *imp.split_container.borrow() {
            sc.restore_session(&session);
        }
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

    fn action_show_shortcuts(&self) {
        let settings = rline_config::EditorSettings::load().unwrap_or_default();
        let window_ref = self.clone();
        let dialog = crate::shortcuts_dialog::build_shortcuts_dialog(
            self.upcast_ref::<gtk4::Window>(),
            &settings,
            move |bindings| {
                // Persist the updated keybindings.
                let mut settings = rline_config::EditorSettings::load().unwrap_or_default();
                settings.keybindings = bindings.clone();
                if let Err(e) = settings.save() {
                    tracing::error!("failed to save keybindings: {e}");
                }
                // Re-register accelerators on the running application.
                if let Some(app) = window_ref.application() {
                    crate::shortcuts::register_accels(&app, bindings);
                }
            },
        );
        dialog.present();
    }

    fn action_show_settings(&self) {
        let imp = self.imp();
        let split_container = imp.split_container.borrow().clone();
        let terminal_pane = imp.terminal_pane.borrow().clone();
        let sc_for_open = imp.split_container.borrow().clone();
        let dialog = crate::editor::SettingsDialog::new(
            self.upcast_ref(),
            move |settings| {
                // Always update app-wide chrome, even if no editor tabs are open
                crate::theming::apply_font_rendering(&settings.hint_style);
                crate::theming::apply_app_theme(&settings.theme);
                if let Some(ref sc) = split_container {
                    sc.apply_settings(&settings);
                }
                if let Some(ref terminal) = terminal_pane {
                    terminal.apply_settings(&settings);
                }
            },
            move |path| {
                if let Some(ref sc) = sc_for_open {
                    if let Err(e) = sc.open_file(&path) {
                        tracing::error!("failed to open system prompt file: {e}");
                    }
                }
            },
        );
        dialog.present();
    }
}
