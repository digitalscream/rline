//! Types for the OpenAI-compatible chat completions API.

use serde::{Deserialize, Serialize};

/// The role of a message participant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System prompt message.
    System,
    /// User-provided message.
    User,
    /// AI assistant response.
    Assistant,
    /// Tool execution result.
    Tool,
}

/// The content of a chat message.
///
/// Serializes as either a bare JSON string (text-only) or an array of
/// content parts (multimodal, matching OpenAI's content-parts format).
/// The `#[serde(untagged)]` representation preserves wire compatibility
/// with any OpenAI-compatible endpoint that only accepts string content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Plain-text content — serializes as a JSON string.
    Text(String),
    /// An array of content parts, which may include text and images.
    Parts(Vec<ContentPart>),
}

/// A single content part in a multimodal message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    /// A text segment.
    Text {
        /// The text content.
        text: String,
    },
    /// An image, supplied as a URL or data URL.
    ImageUrl {
        /// The image URL payload.
        image_url: ImageUrl,
    },
}

/// An image reference for a content part.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    /// Either an `http(s)://` URL or a `data:image/...;base64,...` URL.
    pub url: String,
}

impl MessageContent {
    /// Create a plain-text content.
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text(s.into())
    }

    /// Create a multimodal content from text + a base64-encoded PNG.
    pub fn text_with_png(text: impl Into<String>, png_base64: String) -> Self {
        Self::Parts(vec![
            ContentPart::Text { text: text.into() },
            ContentPart::ImageUrl {
                image_url: ImageUrl {
                    url: format!("data:image/png;base64,{png_base64}"),
                },
            },
        ])
    }

    /// Extract the concatenated text across all parts.
    ///
    /// Returns an empty string if the content is entirely non-text.
    pub fn as_text(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Parts(parts) => parts
                .iter()
                .filter_map(|p| match p {
                    ContentPart::Text { text } => Some(text.as_str()),
                    ContentPart::ImageUrl { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// Approximate character count for token-budget estimation.
    pub fn char_len(&self) -> usize {
        match self {
            Self::Text(s) => s.len(),
            Self::Parts(parts) => parts
                .iter()
                .map(|p| match p {
                    ContentPart::Text { text } => text.len(),
                    // Data URLs can be huge; approximate at a fixed cost to
                    // avoid wildly inflating token estimates. The actual
                    // vision token count is model-specific.
                    ContentPart::ImageUrl { .. } => 1024,
                })
                .sum(),
        }
    }
}

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// The role of the message author.
    pub role: Role,
    /// The content of the message (may be absent for tool-call-only messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<MessageContent>,
    /// Tool calls requested by the assistant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// The ID of the tool call this message is a response to (for `Tool` role).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    /// Create a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: Some(MessageContent::text(content)),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(MessageContent::text(content)),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message with text content.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: Some(MessageContent::text(content)),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message that contains tool calls.
    pub fn assistant_tool_calls(content: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.map(MessageContent::text),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    /// Create a plain-text tool result message.
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(MessageContent::text(content)),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    /// Create a multimodal tool result message with an inline PNG image.
    pub fn tool_result_with_image(
        tool_call_id: impl Into<String>,
        text: impl Into<String>,
        png_base64: String,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: Some(MessageContent::text_with_png(text, png_base64)),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

/// A tool call requested by the assistant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique identifier for this tool call.
    pub id: String,
    /// Always "function" for OpenAI-compatible APIs.
    #[serde(rename = "type")]
    pub call_type: String,
    /// The function to call.
    pub function: FunctionCall,
}

/// A function call within a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    /// The name of the function to call.
    pub name: String,
    /// The arguments as a JSON string.
    pub arguments: String,
}

/// Definition of a tool available to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Always "function" for OpenAI-compatible APIs.
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition.
    pub function: FunctionDefinition,
}

/// The function part of a tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    /// The name of the function.
    pub name: String,
    /// A description of what the function does.
    pub description: String,
    /// JSON Schema describing the function parameters.
    pub parameters: serde_json::Value,
}

impl ToolDefinition {
    /// Create a new tool definition.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            tool_type: "function".to_owned(),
            function: FunctionDefinition {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

/// A chat completion request.
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    /// Model identifier.
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<ChatMessage>,
    /// Available tools (omitted if empty).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    /// Whether to stream the response.
    pub stream: bool,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
}

/// A streaming chunk from the chat completions API.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatStreamChunk {
    /// The choices in this chunk.
    pub choices: Vec<StreamChoice>,
}

/// A single choice in a streaming chunk.
#[derive(Debug, Clone, Deserialize)]
pub struct StreamChoice {
    /// The delta content for this choice.
    pub delta: ChatStreamDelta,
    /// The reason the model stopped generating (present in the final chunk).
    pub finish_reason: Option<String>,
}

/// Incremental content in a streaming response.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChatStreamDelta {
    /// The role (typically only in the first chunk).
    #[serde(default)]
    pub role: Option<Role>,
    /// Incremental text content.
    #[serde(default)]
    pub content: Option<String>,
    /// Incremental tool call data.
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

/// Incremental tool call data in a streaming response.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolCallDelta {
    /// Index of the tool call being built.
    pub index: usize,
    /// Tool call ID (present in the first delta for this index).
    #[serde(default)]
    pub id: Option<String>,
    /// Function data delta.
    #[serde(default)]
    pub function: Option<FunctionCallDelta>,
}

/// Incremental function call data in a streaming response.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FunctionCallDelta {
    /// Function name (present in the first delta for this tool call).
    #[serde(default)]
    pub name: Option<String>,
    /// Partial JSON arguments fragment.
    #[serde(default)]
    pub arguments: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_message_system() {
        let msg = ChatMessage::system("You are helpful.");
        assert_eq!(msg.role, Role::System);
        assert_eq!(
            msg.content.as_ref().map(MessageContent::as_text),
            Some("You are helpful.".to_owned())
        );
        assert!(msg.tool_calls.is_none());
    }

    #[test]
    fn test_chat_message_user() {
        let msg = ChatMessage::user("Hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(
            msg.content.as_ref().map(MessageContent::as_text),
            Some("Hello".to_owned())
        );
    }

    #[test]
    fn test_chat_message_tool_result() {
        let msg = ChatMessage::tool_result("call_123", "file contents here");
        assert_eq!(msg.role, Role::Tool);
        assert_eq!(msg.tool_call_id.as_deref(), Some("call_123"));
        assert_eq!(
            msg.content.as_ref().map(MessageContent::as_text),
            Some("file contents here".to_owned())
        );
    }

    #[test]
    fn test_text_message_serializes_as_bare_string() {
        let msg = ChatMessage::user("hi");
        let json = serde_json::to_value(&msg).expect("serialize");
        assert_eq!(json["content"], serde_json::json!("hi"));
    }

    #[test]
    fn test_multimodal_tool_result_serialization() {
        let msg = ChatMessage::tool_result_with_image(
            "call_1",
            "Screenshot attached.",
            "iVBORw0KGgo=".to_owned(),
        );
        let json = serde_json::to_value(&msg).expect("serialize");
        assert_eq!(json["role"], "tool");
        let parts = json["content"].as_array().expect("parts array");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[0]["text"], "Screenshot attached.");
        assert_eq!(parts[1]["type"], "image_url");
        assert_eq!(
            parts[1]["image_url"]["url"],
            "data:image/png;base64,iVBORw0KGgo="
        );
    }

    #[test]
    fn test_tool_definition_serialization() {
        let def = ToolDefinition::new(
            "read_file",
            "Read a file",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        );
        let json = serde_json::to_value(&def).expect("serialization should succeed in test");
        assert_eq!(json["type"], "function");
        assert_eq!(json["function"]["name"], "read_file");
    }

    #[test]
    fn test_chat_request_serialization() {
        let req = ChatRequest {
            model: "test-model".to_owned(),
            messages: vec![ChatMessage::user("hi")],
            tools: None,
            stream: true,
            max_tokens: Some(100),
            temperature: Some(0.0),
        };
        let json = serde_json::to_value(&req).expect("serialization should succeed in test");
        assert_eq!(json["model"], "test-model");
        assert_eq!(json["stream"], true);
        assert!(json.get("tools").is_none());
    }

    #[test]
    fn test_stream_chunk_deserialization() {
        let data = r#"{
            "choices": [{
                "delta": { "content": "Hello" },
                "finish_reason": null
            }]
        }"#;
        let chunk: ChatStreamChunk =
            serde_json::from_str(data).expect("deserialization should succeed in test");
        assert_eq!(chunk.choices.len(), 1);
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hello"));
        assert!(chunk.choices[0].finish_reason.is_none());
    }

    #[test]
    fn test_stream_tool_call_delta_deserialization() {
        let data = r#"{
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_abc",
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\":"
                        }
                    }]
                },
                "finish_reason": null
            }]
        }"#;
        let chunk: ChatStreamChunk =
            serde_json::from_str(data).expect("deserialization should succeed in test");
        let tc = &chunk.choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.index, 0);
        assert_eq!(tc.id.as_deref(), Some("call_abc"));
        let func = tc.function.as_ref().unwrap();
        assert_eq!(func.name.as_deref(), Some("read_file"));
    }

    #[test]
    fn test_role_serialization() {
        let json = serde_json::to_string(&Role::Assistant).expect("should serialize in test");
        assert_eq!(json, "\"assistant\"");
        let role: Role = serde_json::from_str("\"tool\"").expect("should deserialize in test");
        assert_eq!(role, Role::Tool);
    }
}
