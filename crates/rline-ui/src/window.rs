//! RlineWindow — main application window with three-pane layout.

use std::cell::RefCell;
use std::path::PathBuf;

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;

use crate::editor::EditorPane;
use crate::file_browser::FileBrowserPanel;
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

        // ── Header bar with hamburger menu ──
        let header = gtk4::HeaderBar::new();
        let menu_button = gtk4::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu::build_app_menu())
            .build();
        header.pack_end(&menu_button);
        self.set_titlebar(Some(&header));

        // ── Left pane: Stack with three tabs ──
        let stack = gtk4::Stack::new();
        stack.set_transition_type(gtk4::StackTransitionType::SlideUpDown);

        let file_browser = FileBrowserPanel::new();
        let search_panel = ProjectSearchPanel::new();
        let git_placeholder = gtk4::Label::new(Some("Git (coming soon)"));
        git_placeholder.set_vexpand(true);

        stack.add_titled(file_browser.widget(), Some("files"), "Files");
        stack.add_titled(&git_placeholder, Some("git"), "Git");
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
        self.wire_file_browser(&file_browser, &editor_pane, &terminal_pane, &search_panel);

        // ── Store references ──
        imp.editor_pane.replace(Some(editor_pane));
        imp.file_browser.replace(Some(file_browser.clone()));
        imp.search_panel.replace(Some(search_panel));
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
    /// changes to terminal + search.
    fn wire_file_browser(
        &self,
        file_browser: &FileBrowserPanel,
        editor_pane: &EditorPane,
        terminal_pane: &TerminalPane,
        search_panel: &ProjectSearchPanel,
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

        // Project root changes update terminal + search + persist last project
        let tp = terminal_pane.clone();
        let sp = search_panel.clone();
        file_browser.set_on_project_root_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |root| {
                tracing::info!("project root changed to: {}", root.display());
                window.imp().project_root.replace(Some(root.to_path_buf()));
                tp.set_default_directory(root);
                sp.set_project_root(root);

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

        self.add_action_entries([
            action_open,
            action_close,
            action_quick_open,
            action_search,
            action_settings,
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
        let dialog = crate::editor::SettingsDialog::new(self.upcast_ref(), move |settings| {
            if let Some(ref editor) = editor_pane {
                editor.apply_settings(&settings);
            }
        });
        dialog.present();
    }
}
