//! ProblemsPanel — left-pane sidebar tab listing project lint diagnostics.
//!
//! Mirrors the structure of [`crate::search::ProjectSearchPanel`]: results
//! grouped by file with expand/collapse, single-click to open the file at
//! the offending line.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;

use rline_lint::Diagnostic;

use crate::lint::lint_worker::{self, LintEvent, LintJobHandle};

mod problem_row_object {
    use std::cell::RefCell;

    use glib::prelude::*;
    use glib::subclass::prelude::*;
    use glib::Properties;

    mod imp {
        use super::*;

        #[derive(Debug, Default, Properties)]
        #[properties(wrapper_type = super::ProblemRowObject)]
        pub struct ProblemRowObject {
            #[property(get, set)]
            file_path: RefCell<String>,
            #[property(get, set)]
            display_text: RefCell<String>,
            #[property(get, set)]
            line_number: RefCell<u32>,
            #[property(get, set, name = "is-header")]
            is_header: RefCell<bool>,
        }

        #[glib::object_subclass]
        impl ObjectSubclass for ProblemRowObject {
            const NAME: &'static str = "RlineProblemRow";
            type Type = super::ProblemRowObject;
            type ParentType = glib::Object;
        }

        #[glib::derived_properties]
        impl ObjectImpl for ProblemRowObject {}
    }

    glib::wrapper! {
        pub struct ProblemRowObject(ObjectSubclass<imp::ProblemRowObject>);
    }

    impl ProblemRowObject {
        pub fn new_header(path: &std::path::Path, count: usize) -> Self {
            let filename = path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            let suffix = if count == 1 { "" } else { "s" };
            let display = format!("{filename} ({count} problem{suffix})");
            glib::Object::builder()
                .property("file-path", path.display().to_string())
                .property("display-text", &display)
                .property("line-number", u32::MAX)
                .property("is-header", true)
                .build()
        }

        pub fn new_diagnostic(diag: &rline_lint::Diagnostic) -> Self {
            let severity = match diag.severity {
                rline_lint::Severity::Error => "error",
                rline_lint::Severity::Warning => "warn",
                rline_lint::Severity::Info => "info",
                rline_lint::Severity::Hint => "hint",
            };
            let code = diag.code.as_deref().unwrap_or("");
            let line = diag.range.start.line + 1;
            let display = if code.is_empty() {
                format!("  {line}: [{severity}] {}", diag.message)
            } else {
                format!("  {line}: [{severity} {code}] {}", diag.message)
            };
            glib::Object::builder()
                .property("file-path", diag.path.display().to_string())
                .property("display-text", &display)
                .property("line-number", diag.range.start.line)
                .property("is-header", false)
                .build()
        }
    }
}

use problem_row_object::ProblemRowObject;

/// Sidebar panel showing lint diagnostics across the project.
#[derive(Clone)]
pub struct ProblemsPanel {
    container: gtk4::Box,
    status_label: gtk4::Label,
    run_button: gtk4::Button,
    cancel_button: gtk4::Button,
    results_store: gio::ListStore,
    project_root: Rc<RefCell<Option<PathBuf>>>,
    /// Cached diagnostics keyed by file path.
    cached: Rc<RefCell<HashMap<String, Vec<Diagnostic>>>>,
    /// Expansion state per file.
    expanded: Rc<RefCell<HashMap<String, bool>>>,
    /// In-flight lint job handle (cancellable).
    current_job: Rc<RefCell<Option<LintJobHandle>>>,
    #[allow(clippy::type_complexity)]
    on_open_file_at_line: Rc<RefCell<Option<Box<dyn Fn(&Path, rline_core::LineIndex)>>>>,
}

impl std::fmt::Debug for ProblemsPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProblemsPanel").finish_non_exhaustive()
    }
}

impl Default for ProblemsPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ProblemsPanel {
    /// Create a new panel.
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        container.set_margin_top(4);
        container.set_margin_start(4);
        container.set_margin_end(4);

        let toolbar = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        let run_button = gtk4::Button::with_label("Run lint");
        let cancel_button = gtk4::Button::with_label("Cancel");
        cancel_button.set_sensitive(false);
        toolbar.append(&run_button);
        toolbar.append(&cancel_button);
        container.append(&toolbar);

        let status_label = gtk4::Label::new(Some("Idle"));
        status_label.set_halign(gtk4::Align::Start);
        status_label.add_css_class("dim-label");
        container.append(&status_label);

        let results_store = gio::ListStore::new::<ProblemRowObject>();
        let selection = gtk4::SingleSelection::new(Some(results_store.clone()));
        let factory = gtk4::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            if let Some(list_item) = item.downcast_ref::<gtk4::ListItem>() {
                let label = gtk4::Label::new(None);
                label.set_halign(gtk4::Align::Start);
                label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                list_item.set_child(Some(&label));
            }
        });
        factory.connect_bind(|_, item| {
            if let Some(list_item) = item.downcast_ref::<gtk4::ListItem>() {
                if let Some(obj) = list_item.item().and_downcast::<ProblemRowObject>() {
                    if let Some(label) = list_item.child().and_downcast::<gtk4::Label>() {
                        label.set_text(&obj.display_text());
                        if obj.is_header() {
                            label.add_css_class("heading");
                        } else {
                            label.remove_css_class("heading");
                        }
                    }
                }
            }
        });

        let list_view = gtk4::ListView::new(Some(selection), Some(factory));
        list_view.set_vexpand(true);

        let scrolled = gtk4::ScrolledWindow::builder()
            .child(&list_view)
            .vexpand(true)
            .build();
        container.append(&scrolled);

        let panel = Self {
            container,
            status_label: status_label.clone(),
            run_button: run_button.clone(),
            cancel_button: cancel_button.clone(),
            results_store: results_store.clone(),
            project_root: Rc::new(RefCell::new(None)),
            cached: Rc::new(RefCell::new(HashMap::new())),
            expanded: Rc::new(RefCell::new(HashMap::new())),
            current_job: Rc::new(RefCell::new(None)),
            on_open_file_at_line: Rc::new(RefCell::new(None)),
        };

        // Wire run / cancel
        let panel_for_run = panel.clone();
        run_button.connect_clicked(move |_| panel_for_run.run_lint());
        let panel_for_cancel = panel.clone();
        cancel_button.connect_clicked(move |_| panel_for_cancel.cancel_lint());

        // Wire single-click on result
        let panel_for_click = panel.clone();
        let store_for_click = results_store.clone();
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
                if let Some(obj) = item.downcast_ref::<ProblemRowObject>() {
                    if obj.is_header() {
                        panel_for_click.toggle_file(&obj.file_path());
                    } else {
                        let path = PathBuf::from(obj.file_path());
                        let line = rline_core::LineIndex(obj.line_number() as usize);
                        if let Some(ref cb) = *panel_for_click.on_open_file_at_line.borrow() {
                            cb(&path, line);
                        }
                    }
                }
            }
        });
        list_view.add_controller(click_gesture);

        panel
    }

    /// Set the project root.
    pub fn set_project_root(&self, root: &Path) {
        self.project_root.replace(Some(root.to_path_buf()));
    }

    /// Set the open-at-line callback.
    pub fn set_on_open_file_at_line<F: Fn(&Path, rline_core::LineIndex) + 'static>(&self, f: F) {
        self.on_open_file_at_line.replace(Some(Box::new(f)));
    }

    /// The container widget.
    pub fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    /// Trigger a project lint.
    pub fn run_lint(&self) {
        let root = match self.project_root.borrow().clone() {
            Some(r) => r,
            None => {
                self.status_label.set_text("No project root");
                return;
            }
        };

        // Cancel any existing job.
        self.cancel_lint();

        self.cached.borrow_mut().clear();
        self.expanded.borrow_mut().clear();
        self.results_store.remove_all();
        self.status_label.set_text("Linting…");
        self.run_button.set_sensitive(false);
        self.cancel_button.set_sensitive(true);

        let settings = rline_config::EditorSettings::load().unwrap_or_default();
        let registry = lint_worker::registry_from_settings(&settings);

        let panel = self.clone();
        let handle = lint_worker::spawn_project_lint(registry, root, move |event| match event {
            LintEvent::Diagnostics(diags) => {
                panel.merge_diagnostics(diags);
            }
            LintEvent::Error(e) => {
                tracing::warn!("project lint error: {e}");
            }
            LintEvent::Finished => {
                let total: usize = panel.cached.borrow().values().map(|v| v.len()).sum();
                let suffix = if total == 1 { "" } else { "s" };
                panel
                    .status_label
                    .set_text(&format!("{total} problem{suffix}"));
                panel.run_button.set_sensitive(true);
                panel.cancel_button.set_sensitive(false);
                panel.current_job.borrow_mut().take();
            }
        });
        self.current_job.borrow_mut().replace(handle);
    }

    /// Cancel the in-flight lint job, if any.
    pub fn cancel_lint(&self) {
        if let Some(handle) = self.current_job.borrow_mut().take() {
            handle.cancel();
            self.status_label.set_text("Cancelled");
            self.run_button.set_sensitive(true);
            self.cancel_button.set_sensitive(false);
        }
    }

    fn merge_diagnostics(&self, diagnostics: Vec<Diagnostic>) {
        let mut cached = self.cached.borrow_mut();
        for diag in diagnostics {
            let key = diag.path.display().to_string();
            cached.entry(key).or_default().push(diag);
        }
        // Auto-expand files with up to 5 entries (matching the default search threshold).
        let mut expanded = self.expanded.borrow_mut();
        for (path, diags) in cached.iter() {
            if diags.len() <= 5 {
                expanded.entry(path.clone()).or_insert(true);
            }
        }
        drop(expanded);
        rebuild_store(&self.results_store, &cached, &self.expanded.borrow());
    }

    fn toggle_file(&self, file_path: &str) {
        let mut expanded = self.expanded.borrow_mut();
        let is_expanded = expanded.get(file_path).copied().unwrap_or(false);
        expanded.insert(file_path.to_owned(), !is_expanded);
        drop(expanded);
        rebuild_store(
            &self.results_store,
            &self.cached.borrow(),
            &self.expanded.borrow(),
        );
    }
}

fn rebuild_store(
    store: &gio::ListStore,
    cached: &HashMap<String, Vec<Diagnostic>>,
    expanded: &HashMap<String, bool>,
) {
    store.remove_all();

    let mut paths: Vec<&String> = cached.keys().collect();
    paths.sort();

    for file_path in paths {
        let diags = &cached[file_path];
        let path = PathBuf::from(file_path);
        let is_expanded = expanded.get(file_path).copied().unwrap_or(false);
        let arrow = if is_expanded { "▼" } else { "▶" };
        let header = ProblemRowObject::new_header(&path, diags.len());
        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        let count = diags.len();
        let suffix = if count == 1 { "" } else { "s" };
        header.set_display_text(format!("{arrow} {filename} ({count} problem{suffix})"));
        store.append(&header);

        if is_expanded {
            // Sort by line so they appear in source order.
            let mut sorted: Vec<&Diagnostic> = diags.iter().collect();
            sorted.sort_by_key(|d| (d.range.start.line, d.range.start.column));
            for d in sorted {
                store.append(&ProblemRowObject::new_diagnostic(d));
            }
        }
    }
}
