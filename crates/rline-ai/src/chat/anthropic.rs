//! Anthropic Messages API client — streaming chat completions with tool use.
//!
//! Implements [`AgentChatClient`] by translating the canonical
//! [`ChatRequest`] shape into Anthropic's `/v1/messages` request format and
//! mapping its SSE event taxonomy back onto [`StreamEvent`]s, so the rest of
//! the agent loop can remain provider-agnostic.

use futures_core::Stream;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

use crate::chat::client::{AgentChatClient, StreamEvent};
use crate::chat::types::{
    ChatMessage, ChatRequest, ContentPart, FunctionCall, MessageContent, Role, ToolCall,
};
use crate::error::AiError;

/// Fixed endpoint for the Anthropic Messages API.
const ANTHROPIC_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";

/// Anthropic API version header value.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Client for the Anthropic Messages API.
#[derive(Debug, Clone)]
pub struct AnthropicClient {
    http: reqwest::Client,
    api_key: String,
    model: String,
}

impl AnthropicClient {
    /// Create a new Anthropic client.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }
}

impl AgentChatClient for AnthropicClient {
    fn send_streaming(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> tokio::sync::mpsc::Receiver<StreamEvent> {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let http = self.http.clone();
        let api_key = self.api_key.clone();
        let model = self.model.clone();

        crate::runtime::ai_runtime().spawn(async move {
            if let Err(e) = stream_response(http, api_key, model, request, cancel, tx.clone()).await
            {
                let _ = tx.send(StreamEvent::Done { text: None }).await;
                error!("anthropic stream error: {e}");
            }
        });

        rx
    }
}

/// Translate a canonical [`ChatRequest`] into an Anthropic Messages request.
fn build_request_body(request: &ChatRequest, model: &str) -> AnthropicRequest {
    let (system, messages) = translate_messages(&request.messages);

    let tools = request.tools.as_ref().map(|defs| {
        defs.iter()
            .map(|d| AnthropicTool {
                name: d.function.name.clone(),
                description: d.function.description.clone(),
                input_schema: d.function.parameters.clone(),
            })
            .collect()
    });

    // Anthropic requires max_tokens; default to 4096 to mirror the rest of the
    // codebase if the request didn't specify one.
    let max_tokens = request.max_tokens.unwrap_or(4096);

    AnthropicRequest {
        model: model.to_owned(),
        max_tokens,
        system,
        messages,
        tools,
        stream: true,
        temperature: request.temperature,
    }
}

/// Split a canonical message list into (system_prompt, converted messages).
fn translate_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<AnthropicMessage>) {
    let mut system: Option<String> = None;
    let mut out: Vec<AnthropicMessage> = Vec::with_capacity(messages.len());

    for msg in messages {
        match msg.role {
            Role::System => {
                // Concatenate multiple system messages (rare) with blank line
                // separators. Anthropic only allows one `system` field.
                let text = msg
                    .content
                    .as_ref()
                    .map(MessageContent::as_text)
                    .unwrap_or_default();
                system = Some(match system.take() {
                    Some(prev) => format!("{prev}\n\n{text}"),
                    None => text,
                });
            }
            Role::User => {
                let blocks = message_content_to_blocks(msg.content.as_ref());
                append_user_blocks(&mut out, blocks);
            }
            Role::Assistant => {
                let mut blocks: Vec<AnthropicContentBlock> = Vec::new();
                if let Some(content) = &msg.content {
                    let text = content.as_text();
                    if !text.is_empty() {
                        blocks.push(AnthropicContentBlock::Text { text });
                    }
                }
                if let Some(calls) = &msg.tool_calls {
                    for call in calls {
                        let input: serde_json::Value =
                            serde_json::from_str(&call.function.arguments)
                                .unwrap_or_else(|_| serde_json::json!({}));
                        blocks.push(AnthropicContentBlock::ToolUse {
                            id: call.id.clone(),
                            name: call.function.name.clone(),
                            input,
                        });
                    }
                }
                // Anthropic disallows empty assistant messages. If we have no
                // blocks at all (shouldn't happen, but be safe), skip it.
                if blocks.is_empty() {
                    continue;
                }
                out.push(AnthropicMessage {
                    role: AnthropicRole::Assistant,
                    content: blocks,
                });
            }
            Role::Tool => {
                let tool_use_id = msg.tool_call_id.clone().unwrap_or_default();
                let content = msg
                    .content
                    .as_ref()
                    .map(tool_result_content)
                    .unwrap_or_default();
                let block = AnthropicContentBlock::ToolResult {
                    tool_use_id,
                    content,
                };
                append_user_blocks(&mut out, vec![block]);
            }
        }
    }

    (system, out)
}

/// Append content blocks to a user message, merging with the previous message
/// if it was already a user message (Anthropic disallows consecutive same-role
/// messages, and batching tool_results is idiomatic).
fn append_user_blocks(out: &mut Vec<AnthropicMessage>, blocks: Vec<AnthropicContentBlock>) {
    if blocks.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut() {
        if matches!(last.role, AnthropicRole::User) {
            last.content.extend(blocks);
            return;
        }
    }
    out.push(AnthropicMessage {
        role: AnthropicRole::User,
        content: blocks,
    });
}

/// Convert a canonical `MessageContent` into Anthropic content blocks for
/// user messages (text + optional inline images).
fn message_content_to_blocks(content: Option<&MessageContent>) -> Vec<AnthropicContentBlock> {
    let Some(content) = content else {
        return Vec::new();
    };
    match content {
        MessageContent::Text(s) => {
            if s.is_empty() {
                Vec::new()
            } else {
                vec![AnthropicContentBlock::Text { text: s.clone() }]
            }
        }
        MessageContent::Parts(parts) => parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } if !text.is_empty() => {
                    Some(AnthropicContentBlock::Text { text: text.clone() })
                }
                ContentPart::Text { .. } => None,
                ContentPart::ImageUrl { image_url } => {
                    parse_data_url(&image_url.url).map(|(media_type, data)| {
                        AnthropicContentBlock::Image {
                            source: AnthropicImageSource {
                                source_type: "base64".to_owned(),
                                media_type,
                                data,
                            },
                        }
                    })
                }
            })
            .collect(),
    }
}

/// Convert a tool result's content into Anthropic `tool_result` content.
///
/// Text-only results become a bare string; multimodal results (text + image)
/// become an array of content blocks, matching Anthropic's schema.
fn tool_result_content(content: &MessageContent) -> ToolResultContent {
    match content {
        MessageContent::Text(s) => ToolResultContent::Text(s.clone()),
        MessageContent::Parts(_) => {
            let blocks = message_content_to_blocks(Some(content));
            // Tool results cannot contain tool_use/tool_result nested, so the
            // conversion above (which only emits Text/Image) is exactly right.
            ToolResultContent::Blocks(blocks)
        }
    }
}

/// Extract `(media_type, base64_data)` from a `data:<mime>;base64,<data>` URL.
fn parse_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (header, data) = rest.split_once(',')?;
    let (media_type, encoding) = header.split_once(';')?;
    if encoding != "base64" {
        return None;
    }
    Some((media_type.to_owned(), data.to_owned()))
}

/// Drive the HTTP request and parse the SSE stream.
async fn stream_response(
    http: reqwest::Client,
    api_key: String,
    model: String,
    request: ChatRequest,
    cancel: CancellationToken,
    tx: tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), AiError> {
    let body = build_request_body(&request, &model);
    debug!("sending anthropic request to {ANTHROPIC_ENDPOINT}");

    let mut builder = http
        .post(ANTHROPIC_ENDPOINT)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json");
    if !api_key.is_empty() {
        builder = builder.header("x-api-key", &api_key);
    }

    let response = tokio::select! {
        res = builder.json(&body).send() => res?,
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

    let mut parser = AnthropicStreamParser::default();
    let mut carry = String::new();
    let mut stream = response.bytes_stream();

    loop {
        if cancel.is_cancelled() {
            return Err(AiError::Cancelled);
        }

        let chunk = {
            use tokio::time::{timeout, Duration};
            match timeout(Duration::from_millis(100), next_bytes(&mut stream)).await {
                Ok(Some(Ok(bytes))) => Some(bytes),
                Ok(Some(Err(e))) => return Err(AiError::Http(e)),
                Ok(None) => None,
                Err(_) => continue,
            }
        };

        let Some(bytes) = chunk else {
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

        while let Some(newline_pos) = carry.find('\n') {
            let line = carry[..newline_pos].to_owned();
            carry.drain(..=newline_pos);

            let line = line.trim();
            // Anthropic lines: either `event: <name>` or `data: {...}` or empty.
            // We parse JSON from `data:` lines and use the `type` field as the
            // discriminant, so the `event:` lines are informational only.
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            let payload = payload.trim();
            if payload.is_empty() {
                continue;
            }

            match serde_json::from_str::<AnthropicStreamEvent>(payload) {
                Ok(event) => {
                    parser.handle(event, &tx).await;
                    if parser.done {
                        break;
                    }
                }
                Err(e) => {
                    warn!("failed to parse anthropic SSE event: {e} — payload: {payload}");
                }
            }
        }

        if parser.done {
            break;
        }
    }

    // Flush any remaining tool calls / accumulated text.
    parser.finish(&tx).await;

    Ok(())
}

/// Helper to poll the next bytes chunk from a reqwest stream.
async fn next_bytes(
    stream: &mut (impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin),
) -> Option<Result<bytes::Bytes, reqwest::Error>> {
    use std::pin::Pin;
    std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
}

// ── Anthropic stream parser ────────────────────────────────────────────

/// Stateful parser that maps Anthropic stream events onto [`StreamEvent`]s.
#[derive(Debug, Default)]
struct AnthropicStreamParser {
    /// In-progress content blocks indexed by their `index` field.
    blocks: Vec<BlockState>,
    /// Full text accumulated across all `text_delta` events.
    accumulated_text: String,
    /// Set when `message_stop` arrives or the stream ends.
    done: bool,
    /// Set to true once we've emitted the final `Done`.
    done_emitted: bool,
}

/// In-progress state of a single content block.
#[derive(Debug)]
enum BlockState {
    /// A text block (we don't need to buffer text — we emit deltas live).
    Text,
    /// A tool_use block being accumulated from `input_json_delta` fragments.
    ToolUse {
        id: String,
        name: String,
        input_json: String,
    },
    /// Placeholder for indices we haven't seen a start event for yet.
    Unknown,
}

impl AnthropicStreamParser {
    async fn handle(
        &mut self,
        event: AnthropicStreamEvent,
        tx: &tokio::sync::mpsc::Sender<StreamEvent>,
    ) {
        match event {
            AnthropicStreamEvent::MessageStart { .. } | AnthropicStreamEvent::Ping => {}
            AnthropicStreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                self.ensure_index(index);
                self.blocks[index] = match content_block {
                    AnthropicStreamContentBlock::Text { .. } => BlockState::Text,
                    AnthropicStreamContentBlock::ToolUse { id, name, .. } => BlockState::ToolUse {
                        id,
                        name,
                        input_json: String::new(),
                    },
                };
            }
            AnthropicStreamEvent::ContentBlockDelta { index, delta } => {
                self.ensure_index(index);
                match (&mut self.blocks[index], delta) {
                    (_, AnthropicStreamDelta::TextDelta { text }) => {
                        if !text.is_empty() {
                            self.accumulated_text.push_str(&text);
                            let _ = tx.send(StreamEvent::TextDelta(text)).await;
                        }
                    }
                    (
                        BlockState::ToolUse { input_json, .. },
                        AnthropicStreamDelta::InputJsonDelta { partial_json },
                    ) => {
                        input_json.push_str(&partial_json);
                    }
                    // Ignore unexpected combinations (e.g. input_json_delta on
                    // a text block) rather than erroring — forward-compat with
                    // new delta types.
                    _ => {}
                }
            }
            AnthropicStreamEvent::ContentBlockStop { .. } => {
                // Emission of tool calls is deferred to message_delta /
                // message_stop so all tool_use blocks in the turn go out
                // together in a single StreamEvent::ToolCalls.
            }
            AnthropicStreamEvent::MessageDelta { delta, .. } => {
                if delta.stop_reason.as_deref() == Some("tool_use") {
                    self.emit_tool_calls(tx).await;
                }
            }
            AnthropicStreamEvent::MessageStop => {
                self.done = true;
            }
            AnthropicStreamEvent::Error { error } => {
                warn!(
                    "anthropic stream error event: {}: {}",
                    error.error_type, error.message
                );
                self.done = true;
            }
        }
    }

    fn ensure_index(&mut self, index: usize) {
        while self.blocks.len() <= index {
            self.blocks.push(BlockState::Unknown);
        }
    }

    async fn emit_tool_calls(&mut self, tx: &tokio::sync::mpsc::Sender<StreamEvent>) {
        let mut calls: Vec<ToolCall> = Vec::new();
        for block in std::mem::take(&mut self.blocks) {
            if let BlockState::ToolUse {
                id,
                name,
                input_json,
            } = block
            {
                let arguments = if input_json.is_empty() {
                    "{}".to_owned()
                } else {
                    input_json
                };
                calls.push(ToolCall {
                    id,
                    call_type: "function".to_owned(),
                    function: FunctionCall { name, arguments },
                });
            }
        }
        if !calls.is_empty() {
            let _ = tx.send(StreamEvent::ToolCalls(calls)).await;
        }
    }

    async fn finish(&mut self, tx: &tokio::sync::mpsc::Sender<StreamEvent>) {
        if self.done_emitted {
            return;
        }
        // If we never saw stop_reason == "tool_use" but still have tool_use
        // blocks buffered (shouldn't happen, but be defensive), flush them.
        let has_tool_use = self
            .blocks
            .iter()
            .any(|b| matches!(b, BlockState::ToolUse { .. }));
        if has_tool_use {
            self.emit_tool_calls(tx).await;
        }

        let text = if self.accumulated_text.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.accumulated_text))
        };
        let _ = tx.send(StreamEvent::Done { text }).await;
        self.done_emitted = true;
    }
}

// ── Wire types — Anthropic request body ────────────────────────────────

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: AnthropicRole,
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum AnthropicRole {
    User,
    Assistant,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
    Text {
        text: String,
    },
    Image {
        source: AnthropicImageSource,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: ToolResultContent,
    },
}

#[derive(Debug, Serialize)]
struct AnthropicImageSource {
    #[serde(rename = "type")]
    source_type: String,
    media_type: String,
    data: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ToolResultContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

impl Default for ToolResultContent {
    fn default() -> Self {
        Self::Text(String::new())
    }
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

// ── Wire types — Anthropic SSE stream events ───────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicStreamEvent {
    MessageStart {
        #[allow(dead_code)]
        #[serde(default)]
        message: serde_json::Value,
    },
    ContentBlockStart {
        index: usize,
        content_block: AnthropicStreamContentBlock,
    },
    ContentBlockDelta {
        index: usize,
        delta: AnthropicStreamDelta,
    },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop {
        #[allow(dead_code)]
        index: usize,
    },
    MessageDelta {
        delta: AnthropicMessageDelta,
        #[allow(dead_code)]
        #[serde(default)]
        usage: serde_json::Value,
    },
    MessageStop,
    Ping,
    Error {
        error: AnthropicError,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicStreamContentBlock {
    Text {
        #[allow(dead_code)]
        #[serde(default)]
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        #[allow(dead_code)]
        #[serde(default)]
        input: serde_json::Value,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicStreamDelta {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageDelta {
    #[serde(default)]
    stop_reason: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    stop_sequence: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicError {
    #[serde(rename = "type", default)]
    error_type: String,
    #[serde(default)]
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::types::{ChatMessage, ChatRequest, ToolDefinition};

    fn make_request(messages: Vec<ChatMessage>) -> ChatRequest {
        ChatRequest {
            model: String::new(),
            messages,
            tools: None,
            stream: true,
            max_tokens: Some(1024),
            temperature: Some(0.0),
        }
    }

    #[test]
    fn test_system_message_is_extracted_to_top_level() {
        let req = make_request(vec![
            ChatMessage::system("You are rline's agent."),
            ChatMessage::user("Hello"),
        ]);
        let body = build_request_body(&req, "claude-sonnet-4-6");
        assert_eq!(body.system.as_deref(), Some("You are rline's agent."));
        assert_eq!(
            body.messages.len(),
            1,
            "system message must not appear in messages"
        );
        assert!(matches!(body.messages[0].role, AnthropicRole::User));
    }

    #[test]
    fn test_tool_definitions_translate_to_input_schema() {
        let mut req = make_request(vec![ChatMessage::user("hi")]);
        req.tools = Some(vec![ToolDefinition::new(
            "read_file",
            "Read a file from disk",
            serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        )]);
        let body = build_request_body(&req, "claude-sonnet-4-6");
        let tools = body.tools.expect("tools should translate");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(tools[0].description, "Read a file from disk");
        assert_eq!(tools[0].input_schema["required"][0], "path");
    }

    #[test]
    fn test_assistant_tool_calls_become_tool_use_blocks() {
        let tool_call = ToolCall {
            id: "toolu_123".to_owned(),
            call_type: "function".to_owned(),
            function: FunctionCall {
                name: "read_file".to_owned(),
                arguments: r#"{"path":"src/main.rs"}"#.to_owned(),
            },
        };
        let req = make_request(vec![
            ChatMessage::user("read main"),
            ChatMessage::assistant_tool_calls(Some("Sure.".to_owned()), vec![tool_call]),
            ChatMessage::tool_result("toolu_123", "fn main() {}"),
        ]);
        let body = build_request_body(&req, "claude-sonnet-4-6");

        // messages: [user, assistant(text + tool_use), user(tool_result)]
        assert_eq!(body.messages.len(), 3);

        let json = serde_json::to_value(&body).expect("serialize");
        let assistant = &json["messages"][1];
        assert_eq!(assistant["role"], "assistant");
        let content = assistant["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Sure.");
        assert_eq!(content[1]["type"], "tool_use");
        assert_eq!(content[1]["id"], "toolu_123");
        assert_eq!(content[1]["name"], "read_file");
        assert_eq!(content[1]["input"]["path"], "src/main.rs");

        let tool_result_msg = &json["messages"][2];
        assert_eq!(tool_result_msg["role"], "user");
        let blocks = tool_result_msg["content"].as_array().unwrap();
        assert_eq!(blocks[0]["type"], "tool_result");
        assert_eq!(blocks[0]["tool_use_id"], "toolu_123");
        assert_eq!(blocks[0]["content"], "fn main() {}");
    }

    #[test]
    fn test_multiple_tool_results_batch_into_one_user_message() {
        let calls = vec![
            ToolCall {
                id: "t1".to_owned(),
                call_type: "function".to_owned(),
                function: FunctionCall {
                    name: "a".to_owned(),
                    arguments: "{}".to_owned(),
                },
            },
            ToolCall {
                id: "t2".to_owned(),
                call_type: "function".to_owned(),
                function: FunctionCall {
                    name: "b".to_owned(),
                    arguments: "{}".to_owned(),
                },
            },
        ];
        let req = make_request(vec![
            ChatMessage::user("go"),
            ChatMessage::assistant_tool_calls(None, calls),
            ChatMessage::tool_result("t1", "result-one"),
            ChatMessage::tool_result("t2", "result-two"),
        ]);
        let body = build_request_body(&req, "claude-sonnet-4-6");
        // [user, assistant(tool_use x2), user(tool_result x2)]
        assert_eq!(body.messages.len(), 3);
        let json = serde_json::to_value(&body).expect("serialize");
        let last = &json["messages"][2];
        assert_eq!(last["role"], "user");
        let blocks = last["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 2, "two tool_result blocks should batch");
        assert_eq!(blocks[0]["tool_use_id"], "t1");
        assert_eq!(blocks[1]["tool_use_id"], "t2");
    }

    #[test]
    fn test_multimodal_tool_result_serializes_as_image_block() {
        // Realistic sequence: user → assistant(tool_use) → user(tool_result)
        let tool_call = ToolCall {
            id: "toolu_1".to_owned(),
            call_type: "function".to_owned(),
            function: FunctionCall {
                name: "browser_action".to_owned(),
                arguments: "{}".to_owned(),
            },
        };
        let msg = ChatMessage::tool_result_with_image(
            "toolu_1",
            "Screenshot captured.",
            "iVBORw0KGgo=".to_owned(),
        );
        let req = make_request(vec![
            ChatMessage::user("open example.com"),
            ChatMessage::assistant_tool_calls(None, vec![tool_call]),
            msg,
        ]);
        let body = build_request_body(&req, "claude-sonnet-4-6");
        let json = serde_json::to_value(&body).expect("serialize");
        let tool_result = &json["messages"][2]["content"][0];
        assert_eq!(tool_result["type"], "tool_result");
        assert_eq!(tool_result["tool_use_id"], "toolu_1");
        let blocks = tool_result["content"]
            .as_array()
            .expect("multimodal content array");
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "Screenshot captured.");
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["source"]["type"], "base64");
        assert_eq!(blocks[1]["source"]["media_type"], "image/png");
        assert_eq!(blocks[1]["source"]["data"], "iVBORw0KGgo=");
    }

    #[test]
    fn test_parse_data_url_basic() {
        let (mt, data) = parse_data_url("data:image/png;base64,ABCD").expect("parse");
        assert_eq!(mt, "image/png");
        assert_eq!(data, "ABCD");
    }

    #[test]
    fn test_parse_data_url_rejects_non_base64() {
        assert!(parse_data_url("data:image/png;utf8,hello").is_none());
    }

    #[tokio::test]
    async fn test_stream_parser_text_only() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let mut p = AnthropicStreamParser::default();

        for line in [
            r#"{"type":"message_start","message":{}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{}}"#,
            r#"{"type":"message_stop"}"#,
        ] {
            let ev: AnthropicStreamEvent = serde_json::from_str(line).expect("parse test event");
            p.handle(ev, &tx).await;
        }
        p.finish(&tx).await;
        drop(tx);

        let mut events: Vec<StreamEvent> = Vec::new();
        while let Some(e) = rx.recv().await {
            events.push(e);
        }

        // Expect: TextDelta("Hel"), TextDelta("lo"), Done(text="Hello")
        assert!(matches!(&events[0], StreamEvent::TextDelta(s) if s == "Hel"));
        assert!(matches!(&events[1], StreamEvent::TextDelta(s) if s == "lo"));
        match events.last().expect("last event") {
            StreamEvent::Done { text } => assert_eq!(text.as_deref(), Some("Hello")),
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_stream_parser_tool_use() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let mut p = AnthropicStreamParser::default();

        for line in [
            r#"{"type":"message_start","message":{}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"read_file","input":{}}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"pa"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"th\":\"x\"}"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{}}"#,
            r#"{"type":"message_stop"}"#,
        ] {
            let ev: AnthropicStreamEvent = serde_json::from_str(line).expect("parse test event");
            p.handle(ev, &tx).await;
        }
        p.finish(&tx).await;
        drop(tx);

        let mut events: Vec<StreamEvent> = Vec::new();
        while let Some(e) = rx.recv().await {
            events.push(e);
        }

        let tool_call_event = events
            .iter()
            .find(|e| matches!(e, StreamEvent::ToolCalls(_)))
            .expect("should emit tool calls");
        match tool_call_event {
            StreamEvent::ToolCalls(calls) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].id, "toolu_1");
                assert_eq!(calls[0].function.name, "read_file");
                assert_eq!(calls[0].function.arguments, r#"{"path":"x"}"#);
            }
            _ => unreachable!(),
        }
    }

    #[tokio::test]
    async fn test_stream_parser_mixed_text_and_tool_use() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let mut p = AnthropicStreamParser::default();

        for line in [
            r#"{"type":"message_start","message":{}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Okay, reading."}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_2","name":"list_files","input":{}}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{}"}}"#,
            r#"{"type":"content_block_stop","index":1}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{}}"#,
            r#"{"type":"message_stop"}"#,
        ] {
            let ev: AnthropicStreamEvent = serde_json::from_str(line).expect("parse test event");
            p.handle(ev, &tx).await;
        }
        p.finish(&tx).await;
        drop(tx);

        let mut events: Vec<StreamEvent> = Vec::new();
        while let Some(e) = rx.recv().await {
            events.push(e);
        }

        assert!(matches!(&events[0], StreamEvent::TextDelta(s) if s == "Okay, reading."));
        let tool_event = events
            .iter()
            .find(|e| matches!(e, StreamEvent::ToolCalls(_)))
            .expect("tool calls");
        match tool_event {
            StreamEvent::ToolCalls(calls) => {
                assert_eq!(calls[0].function.name, "list_files");
                assert_eq!(calls[0].function.arguments, "{}");
            }
            _ => unreachable!(),
        }
        match events.last().expect("last") {
            StreamEvent::Done { text } => assert_eq!(text.as_deref(), Some("Okay, reading.")),
            other => panic!("expected Done, got {other:?}"),
        }
    }
}
