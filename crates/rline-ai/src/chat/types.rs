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

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// The role of the message author.
    pub role: Role,
    /// The text content of the message (may be absent for tool-call-only messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
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
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message with text content.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message that contains tool calls.
    pub fn assistant_tool_calls(content: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    /// Create a tool result message.
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(content.into()),
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
        assert_eq!(msg.content.as_deref(), Some("You are helpful."));
        assert!(msg.tool_calls.is_none());
    }

    #[test]
    fn test_chat_message_user() {
        let msg = ChatMessage::user("Hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content.as_deref(), Some("Hello"));
    }

    #[test]
    fn test_chat_message_tool_result() {
        let msg = ChatMessage::tool_result("call_123", "file contents here");
        assert_eq!(msg.role, Role::Tool);
        assert_eq!(msg.tool_call_id.as_deref(), Some("call_123"));
        assert_eq!(msg.content.as_deref(), Some("file contents here"));
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
