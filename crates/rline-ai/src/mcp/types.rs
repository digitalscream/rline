//! JSON-RPC 2.0 and MCP protocol types.
//!
//! These types model the wire format for communication with MCP servers
//! over stdio. All messages use the JSON-RPC 2.0 envelope.

use serde::{Deserialize, Serialize};

/// A JSON-RPC 2.0 request (expects a response).
#[derive(Debug, Serialize)]
pub struct JsonRpcRequest {
    /// Always "2.0".
    pub jsonrpc: &'static str,
    /// Unique request identifier.
    pub id: u64,
    /// The method to invoke.
    pub method: String,
    /// Optional parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC request.
    pub fn new(id: u64, method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.into(),
            params,
        }
    }
}

/// A JSON-RPC 2.0 notification (no response expected).
#[derive(Debug, Serialize)]
pub struct JsonRpcNotification {
    /// Always "2.0".
    pub jsonrpc: &'static str,
    /// The method to invoke.
    pub method: String,
    /// Optional parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcNotification {
    /// Create a new JSON-RPC notification.
    pub fn new(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            method: method.into(),
            params,
        }
    }
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    /// The request identifier this response corresponds to.
    pub id: Option<u64>,
    /// The result on success.
    pub result: Option<serde_json::Value>,
    /// The error on failure.
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
    /// Numeric error code.
    pub code: i64,
    /// Human-readable error message.
    pub message: String,
    /// Optional additional data.
    pub data: Option<serde_json::Value>,
}

/// Server capabilities returned from the `initialize` response.
#[derive(Debug, Deserialize)]
pub struct InitializeResult {
    /// Protocol version the server supports.
    #[serde(default)]
    pub protocol_version: Option<String>,
    /// Server capabilities.
    #[serde(default)]
    pub capabilities: serde_json::Value,
    /// Server information.
    #[serde(default, rename = "serverInfo")]
    pub server_info: Option<ServerInfo>,
}

/// Server identity information.
#[derive(Debug, Deserialize)]
pub struct ServerInfo {
    /// Server name.
    pub name: String,
    /// Server version.
    #[serde(default)]
    pub version: Option<String>,
}

/// A tool definition returned from `tools/list`.
#[derive(Debug, Deserialize)]
pub struct McpToolInfo {
    /// Unique tool name.
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// JSON Schema for the tool's input parameters.
    #[serde(default, rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// The result of a `tools/call` invocation.
#[derive(Debug, Deserialize)]
pub struct McpToolCallResult {
    /// Content items returned by the tool.
    #[serde(default)]
    pub content: Vec<McpContent>,
    /// Whether the tool reported an error.
    #[serde(default, rename = "isError")]
    pub is_error: bool,
}

/// A single content item in a tool call result.
#[derive(Debug, Deserialize)]
pub struct McpContent {
    /// Content type (e.g. "text", "image").
    #[serde(default, rename = "type")]
    pub content_type: String,
    /// Text content (present when `content_type` is "text").
    #[serde(default)]
    pub text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_rpc_request_serializes() {
        let req = JsonRpcRequest::new(1, "tools/list", None);
        let json = serde_json::to_string(&req).expect("should serialize");
        assert!(
            json.contains("\"jsonrpc\":\"2.0\""),
            "should have jsonrpc field"
        );
        assert!(json.contains("\"id\":1"), "should have id field");
        assert!(
            json.contains("\"method\":\"tools/list\""),
            "should have method field"
        );
        assert!(!json.contains("params"), "should omit null params");
    }

    #[test]
    fn test_json_rpc_notification_serializes() {
        let notif = JsonRpcNotification::new("initialized", None);
        let json = serde_json::to_string(&notif).expect("should serialize");
        assert!(!json.contains("\"id\""), "notification should not have id");
    }

    #[test]
    fn test_json_rpc_response_deserializes() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).expect("should deserialize");
        assert_eq!(resp.id, Some(1), "should parse id");
        assert!(resp.result.is_some(), "should have result");
        assert!(resp.error.is_none(), "should not have error");
    }

    #[test]
    fn test_json_rpc_error_response_deserializes() {
        let json =
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"Method not found"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).expect("should deserialize");
        let err = resp.error.expect("should have error");
        assert_eq!(err.code, -32601, "should parse error code");
        assert_eq!(
            err.message, "Method not found",
            "should parse error message"
        );
    }

    #[test]
    fn test_mcp_tool_info_deserializes() {
        let json = r#"{"name":"read_file","description":"Read a file","inputSchema":{"type":"object","properties":{"path":{"type":"string"}}}}"#;
        let info: McpToolInfo = serde_json::from_str(json).expect("should deserialize");
        assert_eq!(info.name, "read_file");
        assert_eq!(info.description.as_deref(), Some("Read a file"));
    }

    #[test]
    fn test_mcp_tool_call_result_deserializes() {
        let json = r#"{"content":[{"type":"text","text":"hello world"}],"isError":false}"#;
        let result: McpToolCallResult = serde_json::from_str(json).expect("should deserialize");
        assert!(!result.is_error, "should not be an error");
        assert_eq!(result.content.len(), 1, "should have one content item");
        assert_eq!(
            result.content[0].text.as_deref(),
            Some("hello world"),
            "should parse text content"
        );
    }

    #[test]
    fn test_mcp_tool_call_result_error() {
        let json = r#"{"content":[{"type":"text","text":"something went wrong"}],"isError":true}"#;
        let result: McpToolCallResult = serde_json::from_str(json).expect("should deserialize");
        assert!(result.is_error, "should be an error");
    }

    #[test]
    fn test_server_notification_no_id() {
        let json = r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).expect("should deserialize");
        assert_eq!(resp.id, None, "notification has no id");
    }
}
