//! HTTP client for OpenAI-compatible chat completions with streaming support.

use futures_core::Stream;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

use crate::chat::stream::{parse_sse_line, ToolCallAccumulator};
use crate::chat::types::{ChatRequest, ChatStreamChunk};
use crate::error::AiError;

/// Parsed events from a streaming chat completion response.
#[derive(Debug)]
pub enum StreamEvent {
    /// An incremental text content delta.
    TextDelta(String),
    /// A complete set of tool calls parsed from the stream.
    ToolCalls(Vec<crate::chat::types::ToolCall>),
    /// The stream has finished. Contains the accumulated assistant text (if any).
    Done {
        /// The full text accumulated across all text deltas.
        text: Option<String>,
    },
}

/// Client for the OpenAI-compatible `/v1/chat/completions` endpoint.
#[derive(Debug, Clone)]
pub struct ChatClient {
    http: reqwest::Client,
    endpoint_url: String,
    api_key: String,
    model: String,
}

impl ChatClient {
    /// Create a new chat client.
    ///
    /// `endpoint_url` can be either the full URL (e.g.
    /// `http://localhost:8080/v1/chat/completions`) or just the base URL
    /// (e.g. `http://localhost:8080/v1`). If the URL does not already end
    /// with `/chat/completions`, that path is appended automatically.
    pub fn new(
        endpoint_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        let url = normalize_endpoint_url(endpoint_url.into());
        Self {
            http: reqwest::Client::new(),
            endpoint_url: url,
            api_key: api_key.into(),
            model: model.into(),
        }
    }

    /// Send a streaming chat completion request.
    ///
    /// Returns a tokio async channel receiver that yields [`StreamEvent`]
    /// values as they are parsed from the SSE stream. The sending task runs
    /// on the AI tokio runtime and respects the provided cancellation token.
    ///
    /// The receiver is async-compatible so the caller can `.recv().await`
    /// without blocking the runtime thread.
    pub fn send_streaming(
        &self,
        mut request: ChatRequest,
        cancel: CancellationToken,
    ) -> tokio::sync::mpsc::Receiver<StreamEvent> {
        request.model.clone_from(&self.model);
        request.stream = true;

        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let http = self.http.clone();
        let url = self.endpoint_url.clone();
        let api_key = self.api_key.clone();

        crate::runtime::ai_runtime().spawn(async move {
            if let Err(e) = stream_response(http, url, api_key, request, cancel, tx.clone()).await {
                let _ = tx.send(StreamEvent::Done { text: None }).await;
                error!("chat stream error: {e}");
            }
        });

        rx
    }
}

/// Internal: drive the HTTP request and parse SSE events.
async fn stream_response(
    http: reqwest::Client,
    url: String,
    api_key: String,
    request: ChatRequest,
    cancel: CancellationToken,
    tx: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), AiError> {
    debug!("sending chat request to {url}");

    let mut builder = http.post(&url);
    if !api_key.is_empty() {
        builder = builder.bearer_auth(&api_key);
    }

    let response = tokio::select! {
        res = builder.json(&request).send() => res?,
        () = cancel.cancelled() => return Err(AiError::Cancelled),
    };

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(AiError::Api {
            status: status.as_u16(),
            body,
        });
    }

    // Read the streaming body in chunks.
    let mut accumulated_text = String::new();
    let mut tool_acc = ToolCallAccumulator::default();
    let mut carry = String::new();

    // reqwest's bytes_stream requires the `stream` feature.
    let mut stream = response.bytes_stream();

    loop {
        if cancel.is_cancelled() {
            return Err(AiError::Cancelled);
        }

        let chunk = {
            use tokio::time::{timeout, Duration};
            // Poll with a short timeout so we can check cancellation regularly.
            match timeout(Duration::from_millis(100), next_bytes(&mut stream)).await {
                Ok(Some(Ok(bytes))) => Some(bytes),
                Ok(Some(Err(e))) => return Err(AiError::Http(e)),
                Ok(None) => None,   // stream ended
                Err(_) => continue, // timeout, loop to check cancel
            }
        };

        let Some(bytes) = chunk else {
            // Stream ended.
            break;
        };

        let text = match std::str::from_utf8(&bytes) {
            Ok(t) => t,
            Err(e) => {
                warn!("non-UTF-8 SSE chunk: {e}");
                continue;
            }
        };

        carry.push_str(text);

        // Process all complete lines in the buffer.
        while let Some(newline_pos) = carry.find('\n') {
            let line = carry[..newline_pos].to_owned();
            carry.drain(..=newline_pos);

            let line = line.trim();
            if line.is_empty() || !line.starts_with("data: ") {
                continue;
            }

            match parse_sse_line(line) {
                Ok(Some(chunk)) => {
                    process_chunk(&chunk, &mut accumulated_text, &mut tool_acc, &tx).await;
                }
                Ok(None) => {
                    // [DONE] sentinel — break inner loop, outer will end too.
                }
                Err(e) => {
                    warn!("failed to parse SSE line: {e}");
                }
            }
        }
    }

    // Emit any accumulated tool calls.
    if tool_acc.has_entries() {
        let calls = tool_acc.finish();
        let _ = tx.send(StreamEvent::ToolCalls(calls)).await;
    }

    let text = if accumulated_text.is_empty() {
        None
    } else {
        Some(accumulated_text)
    };
    let _ = tx.send(StreamEvent::Done { text }).await;

    Ok(())
}

/// Process a single parsed SSE chunk.
async fn process_chunk(
    chunk: &ChatStreamChunk,
    accumulated_text: &mut String,
    tool_acc: &mut ToolCallAccumulator,
    tx: &tokio::sync::mpsc::Sender<StreamEvent>,
) {
    for choice in &chunk.choices {
        // Handle text content delta.
        if let Some(content) = &choice.delta.content {
            if !content.is_empty() {
                accumulated_text.push_str(content);
                let _ = tx.send(StreamEvent::TextDelta(content.clone())).await;
            }
        }

        // Handle tool call deltas.
        if let Some(tool_calls) = &choice.delta.tool_calls {
            for tc_delta in tool_calls {
                tool_acc.feed(tc_delta);
            }
        }

        // If finish_reason is "tool_calls", flush the accumulator now.
        if choice.finish_reason.as_deref() == Some("tool_calls") && tool_acc.has_entries() {
            let mut fresh = ToolCallAccumulator::default();
            std::mem::swap(tool_acc, &mut fresh);
            let calls = fresh.finish();
            let _ = tx.send(StreamEvent::ToolCalls(calls)).await;
        }
    }
}

/// Helper to get the next bytes from a reqwest bytes stream.
async fn next_bytes(
    stream: &mut (impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin),
) -> Option<Result<bytes::Bytes, reqwest::Error>> {
    use std::pin::Pin;

    std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
}

/// Normalize an endpoint URL so it always ends with `/chat/completions`.
///
/// Users commonly provide just the base URL (e.g. `http://host/v1/` or
/// `http://host/v1`). This function detects that and appends the
/// required path.
fn normalize_endpoint_url(mut url: String) -> String {
    // Strip trailing slashes for consistent matching.
    while url.ends_with('/') {
        url.pop();
    }

    if !url.ends_with("/chat/completions") {
        url.push_str("/chat/completions");
    }

    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_client_creation() {
        let client = ChatClient::new(
            "http://localhost:8080/v1/chat/completions",
            "test-key",
            "test-model",
        );
        assert_eq!(
            client.endpoint_url,
            "http://localhost:8080/v1/chat/completions"
        );
        assert_eq!(client.model, "test-model");
    }

    #[test]
    fn test_normalize_url_already_correct() {
        assert_eq!(
            normalize_endpoint_url("http://host/v1/chat/completions".to_owned()),
            "http://host/v1/chat/completions"
        );
    }

    #[test]
    fn test_normalize_url_base_only() {
        assert_eq!(
            normalize_endpoint_url("http://host/v1".to_owned()),
            "http://host/v1/chat/completions"
        );
    }

    #[test]
    fn test_normalize_url_trailing_slash() {
        assert_eq!(
            normalize_endpoint_url("http://host/v1/".to_owned()),
            "http://host/v1/chat/completions"
        );
    }

    #[test]
    fn test_normalize_url_multiple_trailing_slashes() {
        assert_eq!(
            normalize_endpoint_url("http://host/v1//".to_owned()),
            "http://host/v1/chat/completions"
        );
    }
}
