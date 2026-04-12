//! A single headless Chromium session driven via the DevTools Protocol.
//!
//! Each `BrowserSession` owns one Chromium process and one page. Clicks and
//! typing use the top-level `chromiumoxide` mouse / keyboard helpers so the
//! model can target arbitrary `(x, y)` coordinates, matching the Cline
//! extension's action surface.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chromiumoxide::browser::{Browser, BrowserConfig, HeadlessMode};
use chromiumoxide::cdp::browser_protocol::input::InsertTextParams;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::cdp::js_protocol::runtime::{
    ConsoleApiCalledType, EventConsoleApiCalled, EventExceptionThrown,
};
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::layout::Point;
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide::Page;
use futures::StreamExt;
use tokio::task::JoinHandle;

use crate::error::AiError;

/// Result of a [`BrowserSession::scroll`] call.
#[derive(Debug, Clone, Copy)]
pub struct ScrollOutcome {
    /// `window.scrollY` before the scroll.
    pub before_y: f64,
    /// `window.scrollY` after the scroll.
    pub after_y: f64,
    /// The maximum scrollable Y position (`scrollHeight - innerHeight`).
    pub max_y: f64,
}

impl ScrollOutcome {
    /// Whether the scroll actually moved the page.
    pub fn moved(&self) -> bool {
        (self.after_y - self.before_y).abs() > 0.5
    }

    /// Whether the page is scrolled to the very top.
    pub fn at_top(&self) -> bool {
        self.after_y <= 0.5
    }

    /// Whether the page is scrolled to the bottom (within 1px).
    pub fn at_bottom(&self) -> bool {
        self.max_y > 0.0 && (self.max_y - self.after_y) <= 1.0
    }
}

#[derive(serde::Deserialize)]
struct ScrollOutcomeRaw {
    before: f64,
    after: f64,
    max: f64,
}

/// A live browser session with one page.
pub struct BrowserSession {
    browser: Browser,
    page: Page,
    handler_task: JoinHandle<()>,
    console_logs: Arc<Mutex<Vec<String>>>,
    _log_task: JoinHandle<()>,
    _error_task: JoinHandle<()>,
    viewport: (u32, u32),
}

impl BrowserSession {
    /// Launch a new headless Chromium at the given URL and viewport.
    pub async fn launch(url: &str, viewport: (u32, u32)) -> Result<Self, AiError> {
        let (width, height) = viewport;
        let config = BrowserConfig::builder()
            .headless_mode(HeadlessMode::New)
            .window_size(width, height)
            .viewport(Viewport {
                width,
                height,
                device_scale_factor: None,
                emulating_mobile: false,
                is_landscape: width > height,
                has_touch: false,
            })
            .arg("--no-sandbox")
            .arg("--disable-dev-shm-usage")
            .build()
            .map_err(AiError::Browser)?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| AiError::Browser(format!("failed to launch Chromium: {e}")))?;

        // Keep polling the handler for the whole lifetime of the session.
        // Logging non-fatal events helps diagnose startup/teardown races —
        // breaking on the first error kills the connection and makes every
        // subsequent command fail with "receiver is gone".
        let handler_task = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(e) = event {
                    tracing::warn!("chromiumoxide handler event error: {e}");
                }
            }
        });

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| AiError::Browser(format!("failed to open page: {e}")))?;

        let console_logs = Arc::new(Mutex::new(Vec::<String>::new()));

        let log_task = {
            let logs = Arc::clone(&console_logs);
            let mut stream = page
                .event_listener::<EventConsoleApiCalled>()
                .await
                .map_err(|e| AiError::Browser(format!("console listener failed: {e}")))?;
            tokio::spawn(async move {
                while let Some(event) = stream.next().await {
                    let prefix = match event.r#type {
                        ConsoleApiCalledType::Log => "log",
                        ConsoleApiCalledType::Debug => "debug",
                        ConsoleApiCalledType::Info => "info",
                        ConsoleApiCalledType::Error => "error",
                        ConsoleApiCalledType::Warning => "warn",
                        _ => "console",
                    };
                    let text = event
                        .args
                        .iter()
                        .filter_map(|a| a.value.as_ref().map(|v| v.to_string()))
                        .collect::<Vec<_>>()
                        .join(" ");
                    if let Ok(mut guard) = logs.lock() {
                        guard.push(format!("[{prefix}] {text}"));
                    }
                }
            })
        };

        let error_task = {
            let logs = Arc::clone(&console_logs);
            let mut stream = page
                .event_listener::<EventExceptionThrown>()
                .await
                .map_err(|e| AiError::Browser(format!("exception listener failed: {e}")))?;
            tokio::spawn(async move {
                while let Some(event) = stream.next().await {
                    let msg = &event.exception_details.text;
                    if let Ok(mut guard) = logs.lock() {
                        guard.push(format!("[pageerror] {msg}"));
                    }
                }
            })
        };

        // Enable runtime so console events actually fire.
        let _ = page.enable_runtime().await;
        let _ = page.enable_log().await;

        // Navigate to the requested URL.
        page.goto(url)
            .await
            .map_err(|e| AiError::Browser(format!("navigation failed: {e}")))?;
        let _ = page.wait_for_navigation().await;

        // Small settle delay to let initial console logs accumulate.
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok(Self {
            browser,
            page,
            handler_task,
            console_logs,
            _log_task: log_task,
            _error_task: error_task,
            viewport,
        })
    }

    /// Navigate an already-open session to a new URL.
    pub async fn navigate(&self, url: &str) -> Result<(), AiError> {
        self.page
            .goto(url)
            .await
            .map_err(|e| AiError::Browser(format!("navigation failed: {e}")))?;
        let _ = self.page.wait_for_navigation().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(())
    }

    /// Click at viewport coordinates `(x, y)`.
    pub async fn click(&self, x: f64, y: f64) -> Result<(), AiError> {
        self.page
            .click(Point::new(x, y))
            .await
            .map_err(|e| AiError::Browser(format!("click failed: {e}")))?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(())
    }

    /// Insert text into the currently focused element.
    ///
    /// Uses the CDP `Input.insertText` method rather than per-character
    /// key events so emoji, IME input, and Unicode all work.
    pub async fn type_text(&self, text: &str) -> Result<(), AiError> {
        self.page
            .execute(InsertTextParams::new(text))
            .await
            .map_err(|e| AiError::Browser(format!("type failed: {e}")))?;
        Ok(())
    }

    /// Scroll the page vertically by `dy` pixels (positive = down).
    ///
    /// Reports the scroll-position before and after so the caller can tell
    /// the model when a scroll action had no effect (the page has hit the
    /// top or bottom).
    pub async fn scroll(&self, dy: i32) -> Result<ScrollOutcome, AiError> {
        let js = format!(
            "(() => {{ \
                const before = Math.round(window.scrollY); \
                window.scrollBy(0, {dy}); \
                const after = Math.round(window.scrollY); \
                const max = Math.max(0, Math.round((document.documentElement.scrollHeight || 0) - window.innerHeight)); \
                return {{ before, after, max }}; \
            }})()"
        );
        let result = self
            .page
            .evaluate(js)
            .await
            .map_err(|e| AiError::Browser(format!("scroll failed: {e}")))?;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let value: ScrollOutcomeRaw = result
            .into_value()
            .map_err(|e| AiError::Browser(format!("scroll returned unexpected value: {e}")))?;
        Ok(ScrollOutcome {
            before_y: value.before,
            after_y: value.after,
            max_y: value.max,
        })
    }

    /// Capture a PNG screenshot of the current viewport.
    pub async fn screenshot(&self) -> Result<Vec<u8>, AiError> {
        let params = ScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Png)
            .build();
        self.page
            .screenshot(params)
            .await
            .map_err(|e| AiError::Browser(format!("screenshot failed: {e}")))
    }

    /// Return the page's current URL, or an empty string if unknown.
    pub async fn current_url(&self) -> String {
        self.page.url().await.ok().flatten().unwrap_or_default()
    }

    /// Drain and return the accumulated console log lines.
    pub fn drain_logs(&self) -> Vec<String> {
        self.console_logs
            .lock()
            .map(|mut g| std::mem::take(&mut *g))
            .unwrap_or_default()
    }

    /// Viewport (width, height) the browser was launched with.
    pub fn viewport(&self) -> (u32, u32) {
        self.viewport
    }

    /// Close the browser process and wait for the handler task to exit.
    pub async fn close(mut self) -> Result<(), AiError> {
        let close_result = self.browser.close().await;
        let _ = self.browser.wait().await;
        self.handler_task.abort();
        let _ = self.handler_task.await;
        close_result
            .map(|_| ())
            .map_err(|e| AiError::Browser(format!("close failed: {e}")))
    }
}
