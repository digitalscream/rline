//! Background workers for formatting and linting.
//!
//! Mirrors the pattern used by `git_worker` and `search_worker`:
//! `std::thread::spawn` for the blocking external-process call, an
//! `mpsc::channel` for the result, and `glib::idle_add_local` to deliver it
//! back to the GTK main thread.
//!
//! Sharing the agent's tokio runtime would either starve AI streaming or
//! starve lint, so format/lint always run on dedicated OS threads.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;

use rline_lint::{Diagnostic, Formatter, LintError, LintRegistry};

/// Result of a single format request.
pub type FormatResult = Result<String, LintError>;

/// Spawn a format job. The result lands in the supplied callback on the GTK
/// main thread. Returns immediately.
///
/// `request_id` is opaque to the worker — callers use it to ignore stale
/// results when a newer format request has superseded this one.
pub fn spawn_format<F>(
    formatter: Arc<dyn Formatter>,
    source: String,
    path: PathBuf,
    request_id: u64,
    on_result: F,
) where
    F: FnOnce(u64, FormatResult) + 'static,
{
    let (sender, receiver) = mpsc::channel::<FormatResult>();

    std::thread::spawn(move || {
        let result = formatter.format(&source, &path);
        // The receiver may have been dropped if the user closed the tab; that
        // is fine — drop the result.
        let _ = sender.send(result);
    });

    let mut callback = Some(on_result);
    glib::idle_add_local(move || match receiver.try_recv() {
        Ok(result) => {
            if let Some(cb) = callback.take() {
                cb(request_id, result);
            }
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(mpsc::TryRecvError::Disconnected) => {
            if let Some(cb) = callback.take() {
                cb(
                    request_id,
                    Err(LintError::ParseError {
                        tool: "format".to_owned(),
                        message: "worker disconnected".to_owned(),
                    }),
                );
            }
            glib::ControlFlow::Break
        }
    });
}

/// Cancellable handle for a running project lint job.
#[derive(Debug, Clone)]
pub struct LintJobHandle {
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl LintJobHandle {
    /// Request cancellation. The worker checks this flag between linters
    /// (it cannot interrupt an in-flight subprocess; that's a v1 limitation
    /// that matches the existing search/git workers).
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Returns true if cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Events emitted by [`spawn_project_lint`] as it streams results.
#[derive(Debug)]
pub enum LintEvent {
    /// A linter has produced diagnostics. Successive `Diagnostics` events
    /// extend the current set; the receiver should `extend` rather than
    /// replace.
    Diagnostics(Vec<Diagnostic>),
    /// A linter failed. The error is informational; the worker continues
    /// with remaining linters.
    Error(LintError),
    /// All linters have finished.
    Finished,
}

/// Spawn a project-wide lint job.
///
/// The job iterates over every linter registered in `registry`, invoking each
/// in turn against `root`. Diagnostics stream back via `on_event` on the GTK
/// main thread.
pub fn spawn_project_lint<F>(registry: LintRegistry, root: PathBuf, on_event: F) -> LintJobHandle
where
    F: Fn(LintEvent) + 'static,
{
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handle = LintJobHandle {
        cancelled: cancelled.clone(),
    };

    let (sender, receiver) = mpsc::channel::<LintEvent>();
    let cancelled_for_thread = cancelled.clone();

    std::thread::spawn(move || {
        for (_lang, linter) in registry.linters() {
            if cancelled_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            let event = match linter.lint_project(&root) {
                Ok(diags) => LintEvent::Diagnostics(diags),
                Err(e) => {
                    tracing::warn!("linter '{}' failed: {e}", linter.name());
                    LintEvent::Error(e)
                }
            };
            if sender.send(event).is_err() {
                return; // receiver dropped
            }
        }
        let _ = sender.send(LintEvent::Finished);
    });

    glib::idle_add_local(move || loop {
        match receiver.try_recv() {
            Ok(event) => {
                let is_finished = matches!(event, LintEvent::Finished);
                on_event(event);
                if is_finished {
                    return glib::ControlFlow::Break;
                }
            }
            Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
        }
    });

    handle
}

/// Build a [`LintRegistry`] from current settings.
pub fn registry_from_settings(settings: &rline_config::EditorSettings) -> LintRegistry {
    LintRegistry::from_settings(&settings.lint)
}

/// Resolve the language id (`"rust"`, `"python"`, `"javascript"`, `"ruby"`,
/// or `None`) for a path, used by format-on-save's per-language toggle.
pub fn language_id_for_path(path: &Path) -> Option<&'static str> {
    let ext = path.extension().and_then(|e| e.to_str())?;
    let lang = rline_syntax::language_for_extension(ext)?;
    Some(match lang {
        rline_syntax::SupportedLanguage::Rust => "rust",
        rline_syntax::SupportedLanguage::Python => "python",
        rline_syntax::SupportedLanguage::JavaScript => "javascript",
        rline_syntax::SupportedLanguage::Ruby => "ruby",
        _ => return None,
    })
}
