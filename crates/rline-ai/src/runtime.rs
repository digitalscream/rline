//! Shared tokio runtime for AI operations.
//!
//! GTK4 runs on its own main loop, so AI async work runs on a separate
//! dedicated tokio runtime. This avoids requiring the entire application
//! to be wrapped in a tokio runtime.

use std::sync::OnceLock;

use tokio::runtime::Runtime;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Returns a reference to the shared AI tokio runtime.
///
/// The runtime is lazily initialised on first call with a single
/// worker thread. All async AI operations (HTTP requests, cancellation)
/// should be spawned on this runtime.
pub fn ai_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("rline-ai")
            .build()
            .expect("failed to create AI tokio runtime")
    })
}
