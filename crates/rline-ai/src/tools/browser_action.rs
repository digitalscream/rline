//! Tool: drive a headless Chromium for web interaction.
//!
//! One session persists across tool calls within an agent task — the model
//! must begin a session with `launch` and end it with `close`. Matches the
//! action surface of the Cline `browser_action` tool.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;
use tokio::runtime::Handle;

use crate::browser::BrowserSession;
use crate::chat::types::ToolDefinition;
use crate::error::AiError;
use crate::tools::{Tool, ToolCategory, ToolResult};

const ACTION_DESCRIPTION: &str = "Interact with a headless Chromium browser. Use this to \
open a web page, click or type into it, scroll, and capture the result. A session must be \
started with action=launch and closed with action=close before the task ends. Only one \
browser action may run per tool-call turn. When `agent_multimodal` is enabled the screenshot \
is attached inline; otherwise the screenshot is saved under `.agent-cache/screenshots/` and \
its path is included in the text response alongside the current URL and captured console \
output. Supported actions: launch (requires url), click (requires coordinate \"x,y\" in the \
current viewport), type (requires text — inserted into the focused element), scroll_down, \
scroll_up, close. IMPORTANT: if the tool result contains a `Note:` line indicating the page \
did not scroll or has reached the top/bottom, DO NOT issue another scroll in the same \
direction — the model has seen all available content and must move on (close the session, \
or take a different approach).";

const SCROLL_PIXELS: i32 = 600;

/// Shared state for the browser_action tool.
#[derive(Clone)]
pub struct BrowserActionTool {
    session: Arc<Mutex<Option<BrowserSession>>>,
    runtime: Handle,
    viewport: (u32, u32),
    multimodal: Arc<std::sync::atomic::AtomicBool>,
}

impl BrowserActionTool {
    /// Create a new browser_action tool bound to the given tokio runtime handle.
    pub fn new(runtime: Handle, viewport: (u32, u32), multimodal: bool) -> Self {
        Self {
            session: Arc::new(Mutex::new(None)),
            runtime,
            viewport,
            multimodal: Arc::new(std::sync::atomic::AtomicBool::new(multimodal)),
        }
    }

    /// Whether the model is configured as multimodal.
    pub fn is_multimodal(&self) -> bool {
        self.multimodal.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Update the multimodal flag (called when settings change).
    pub fn set_multimodal(&self, multimodal: bool) {
        self.multimodal
            .store(multimodal, std::sync::atomic::Ordering::Relaxed);
    }

    /// Synchronously close any open session. Safe to call during drop.
    pub fn shutdown(&self) {
        let maybe = match self.session.lock() {
            Ok(mut g) => g.take(),
            Err(_) => return,
        };
        if let Some(session) = maybe {
            let _ = self.runtime.block_on(async move { session.close().await });
        }
    }
}

impl Drop for BrowserActionTool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Launch,
    Click,
    Type,
    ScrollDown,
    ScrollUp,
    Close,
}

#[derive(Debug, Deserialize)]
struct Args {
    action: Action,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    coordinate: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

fn parse_coordinate(s: &str) -> Option<(f64, f64)> {
    let (xs, ys) = s.split_once(',')?;
    let x: f64 = xs.trim().parse().ok()?;
    let y: f64 = ys.trim().parse().ok()?;
    Some((x, y))
}

fn screenshot_path(workspace_root: &Path) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    workspace_root
        .join(".agent-cache")
        .join("screenshots")
        .join(format!("{ts}.png"))
}

impl Tool for BrowserActionTool {
    fn name(&self) -> &str {
        "browser_action"
    }

    fn definition(&self) -> ToolDefinition {
        let (w, h) = self.viewport;
        let description = format!(
            "{ACTION_DESCRIPTION} Current viewport: {w}x{h}. Coordinates must fall within \
             (0,0)-({w},{h})."
        );
        ToolDefinition::new(
            "browser_action",
            description,
            super::definitions::schema! {
                required: ["action"],
                properties: {
                    "action" => serde_json::json!({
                        "type": "string",
                        "enum": ["launch", "click", "type", "scroll_down", "scroll_up", "close"],
                        "description": "The browser action to perform."
                    }),
                    "url" => serde_json::json!({
                        "type": "string",
                        "description": "URL to open. Required for action=launch."
                    }),
                    "coordinate" => serde_json::json!({
                        "type": "string",
                        "description": "Viewport coordinate as \"x,y\". Required for action=click."
                    }),
                    "text" => serde_json::json!({
                        "type": "string",
                        "description": "Text to insert into the focused element. Required for action=type."
                    })
                }
            },
        )
    }

    fn execute(&self, arguments: &str, workspace_root: &Path) -> Result<ToolResult, AiError> {
        let args: Args = match serde_json::from_str(arguments) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::err(format!("invalid arguments: {e}"))),
        };

        let viewport = self.viewport;
        let session_slot = Arc::clone(&self.session);
        let runtime = self.runtime.clone();

        let outcome: Result<BrowserOutcome, String> = runtime.block_on(async move {
            match args.action {
                Action::Launch => {
                    let url = args
                        .url
                        .ok_or_else(|| "action=launch requires a `url`".to_owned())?;

                    // Close any prior session first.
                    let prior = take_session(&session_slot)?;
                    if let Some(old) = prior {
                        let _ = old.close().await;
                    }

                    let session = BrowserSession::launch(&url, viewport)
                        .await
                        .map_err(|e| e.to_string())?;
                    let outcome = capture_outcome(&session).await?;
                    store_session(&session_slot, session)?;
                    Ok(outcome)
                }
                Action::Click => {
                    let coord = args
                        .coordinate
                        .ok_or_else(|| "action=click requires a `coordinate`".to_owned())?;
                    let (x, y) = parse_coordinate(&coord)
                        .ok_or_else(|| format!("coordinate must be \"x,y\", got {coord:?}"))?;
                    let session = take_required(&session_slot)?;
                    let result = async {
                        session.click(x, y).await.map_err(|e| e.to_string())?;
                        capture_outcome(&session).await
                    }
                    .await;
                    store_session(&session_slot, session)?;
                    result
                }
                Action::Type => {
                    let text = args
                        .text
                        .ok_or_else(|| "action=type requires `text`".to_owned())?;
                    let session = take_required(&session_slot)?;
                    let result = async {
                        session.type_text(&text).await.map_err(|e| e.to_string())?;
                        capture_outcome(&session).await
                    }
                    .await;
                    store_session(&session_slot, session)?;
                    result
                }
                Action::ScrollDown => scroll(&session_slot, SCROLL_PIXELS).await,
                Action::ScrollUp => scroll(&session_slot, -SCROLL_PIXELS).await,
                Action::Close => {
                    let prior = take_session(&session_slot)?;
                    if let Some(session) = prior {
                        session.close().await.map_err(|e| e.to_string())?;
                    }
                    Ok(BrowserOutcome {
                        screenshot: None,
                        current_url: String::new(),
                        logs: Vec::new(),
                        closed: true,
                        note: None,
                    })
                }
            }
        });

        match outcome {
            Ok(mut out) => {
                let logs_text = if out.logs.is_empty() {
                    "(none)".to_owned()
                } else {
                    out.logs.join("\n")
                };
                if out.closed {
                    return Ok(ToolResult::ok("Browser session closed.".to_owned()));
                }
                let note_line = out
                    .note
                    .as_deref()
                    .map(|n| format!("Note: {n}\n"))
                    .unwrap_or_default();

                let Some(png) = out.screenshot.take() else {
                    return Ok(ToolResult::ok(format!(
                        "URL: {}\n{note_line}Console:\n{logs_text}",
                        out.current_url
                    )));
                };

                if self.is_multimodal() {
                    let text = format!(
                        "URL: {}\n{note_line}Console:\n{logs_text}\n(Screenshot attached.)",
                        out.current_url
                    );
                    Ok(ToolResult::ok_with_image(text, png))
                } else {
                    let path = screenshot_path(workspace_root);
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    match std::fs::write(&path, &png) {
                        Ok(()) => Ok(ToolResult::ok(format!(
                            "URL: {}\n{note_line}Screenshot: {}\nConsole:\n{logs_text}",
                            out.current_url,
                            path.display()
                        ))),
                        Err(e) => Ok(ToolResult::ok(format!(
                            "URL: {}\n{note_line}Screenshot could not be saved ({e}); size {} bytes.\nConsole:\n{logs_text}",
                            out.current_url,
                            png.len()
                        ))),
                    }
                }
            }
            Err(msg) => Ok(ToolResult::err(msg)),
        }
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Browser
    }
}

struct BrowserOutcome {
    screenshot: Option<Vec<u8>>,
    current_url: String,
    logs: Vec<String>,
    closed: bool,
    /// Optional boundary / state note to surface to the model (e.g. "at
    /// bottom of page"). Appears as a `Note:` line in the tool result.
    note: Option<String>,
}

fn take_session(
    slot: &Arc<Mutex<Option<BrowserSession>>>,
) -> Result<Option<BrowserSession>, String> {
    Ok(slot
        .lock()
        .map_err(|_| "session lock poisoned".to_owned())?
        .take())
}

fn take_required(slot: &Arc<Mutex<Option<BrowserSession>>>) -> Result<BrowserSession, String> {
    take_session(slot)?
        .ok_or_else(|| "no active browser session — call action=launch first".to_owned())
}

fn store_session(
    slot: &Arc<Mutex<Option<BrowserSession>>>,
    session: BrowserSession,
) -> Result<(), String> {
    slot.lock()
        .map_err(|_| "session lock poisoned".to_owned())?
        .replace(session);
    Ok(())
}

async fn capture_outcome(session: &BrowserSession) -> Result<BrowserOutcome, String> {
    let screenshot = session.screenshot().await.map_err(|e| e.to_string())?;
    Ok(BrowserOutcome {
        screenshot: Some(screenshot),
        current_url: session.current_url().await,
        logs: session.drain_logs(),
        closed: false,
        note: None,
    })
}

async fn scroll(
    session_slot: &Arc<Mutex<Option<BrowserSession>>>,
    dy: i32,
) -> Result<BrowserOutcome, String> {
    let session = take_required(session_slot)?;
    let result = async {
        let scroll = session.scroll(dy).await.map_err(|e| e.to_string())?;
        let mut outcome = capture_outcome(&session).await?;
        outcome.note = scroll_boundary_note(dy, scroll);
        Ok(outcome)
    }
    .await;
    store_session(session_slot, session)?;
    result
}

fn scroll_boundary_note(dy: i32, scroll: crate::browser::ScrollOutcome) -> Option<String> {
    if !scroll.moved() {
        if dy > 0 {
            return Some(
                "Page did not scroll — already at the bottom of the document. Stop scrolling \
                 and use a different strategy (close, or use the visible content)."
                    .to_owned(),
            );
        }
        if dy < 0 {
            return Some("Page did not scroll — already at the top of the document.".to_owned());
        }
        return Some("Page did not scroll (no movement).".to_owned());
    }
    if dy > 0 && scroll.at_bottom() {
        return Some("Reached the bottom of the page.".to_owned());
    }
    if dy < 0 && scroll.at_top() {
        return Some("Reached the top of the page.".to_owned());
    }
    None
}

/// Encode PNG bytes to base64 (used when building multimodal chat messages).
pub fn encode_png_base64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> BrowserActionTool {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("build rt");
        BrowserActionTool::new(rt.handle().clone(), (900, 600), false)
    }

    #[test]
    fn test_parse_coordinate_ok() {
        assert_eq!(parse_coordinate("450,300"), Some((450.0, 300.0)));
        assert_eq!(parse_coordinate("  12 , 34 "), Some((12.0, 34.0)));
    }

    #[test]
    fn test_parse_coordinate_bad() {
        assert!(parse_coordinate("no-comma").is_none());
        assert!(parse_coordinate("abc,def").is_none());
    }

    #[test]
    fn test_missing_url_for_launch() {
        let t = tool();
        let args = serde_json::json!({ "action": "launch" }).to_string();
        let dir = tempfile::tempdir().expect("temp dir");
        let res = t.execute(&args, dir.path()).expect("tool call");
        assert!(!res.success);
        assert!(res.output.contains("url"));
    }

    #[test]
    fn test_missing_coordinate_for_click() {
        let t = tool();
        let args = serde_json::json!({ "action": "click" }).to_string();
        let dir = tempfile::tempdir().expect("temp dir");
        let res = t.execute(&args, dir.path()).expect("tool call");
        assert!(!res.success);
        assert!(res.output.contains("coordinate"));
    }

    #[test]
    fn test_close_with_no_session_is_ok() {
        let t = tool();
        let args = serde_json::json!({ "action": "close" }).to_string();
        let dir = tempfile::tempdir().expect("temp dir");
        let res = t.execute(&args, dir.path()).expect("tool call");
        assert!(res.success);
    }

    #[test]
    fn test_encode_png_base64_roundtrip() {
        let bytes = b"\x89PNG\r\n\x1a\n";
        let s = encode_png_base64(bytes);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(s.as_bytes())
            .expect("decode");
        assert_eq!(decoded, bytes);
    }
}
