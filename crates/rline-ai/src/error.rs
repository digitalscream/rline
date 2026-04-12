//! AI-specific error types.

/// Errors that can occur during AI completion and agent operations.
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    /// An HTTP/network error from the underlying client.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// The API returned a non-success status code.
    #[error("API error (status {status}): {body}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Response body text.
        body: String,
    },

    /// The request was cancelled via a cancellation token.
    #[error("request cancelled")]
    Cancelled,

    /// The API response contained no completion choices.
    #[error("no completion choices in response")]
    NoChoices,

    /// Failed to deserialize the API response.
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    /// Failed to parse an SSE stream event.
    #[error("SSE stream parse error: {detail}")]
    StreamParse {
        /// Description of the parse failure.
        detail: String,
    },

    /// A tool execution failed.
    #[error("tool '{tool}' failed: {detail}")]
    ToolExecution {
        /// The name of the tool that failed.
        tool: String,
        /// Description of the failure.
        detail: String,
    },

    /// The requested tool was not found in the registry.
    #[error("tool not found: {0}")]
    ToolNotFound(String),

    /// An I/O error during tool execution.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A regex compilation error.
    #[error("regex error: {0}")]
    Regex(#[from] regex::Error),

    /// An MCP server returned a JSON-RPC error.
    #[error("MCP server '{server}' error: {message}")]
    McpServerError {
        /// The name of the MCP server.
        server: String,
        /// The error message from the server.
        message: String,
    },

    /// Failed to communicate with an MCP server process.
    #[error("MCP transport error for '{server}': {detail}")]
    McpTransport {
        /// The name of the MCP server.
        server: String,
        /// Description of the transport failure.
        detail: String,
    },

    /// MCP server configuration is invalid.
    #[error("MCP config error: {0}")]
    McpConfig(String),

    /// A browser automation error.
    #[error("browser error: {0}")]
    Browser(String),
}
