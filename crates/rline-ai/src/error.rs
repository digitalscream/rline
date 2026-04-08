//! AI-specific error types.

/// Errors that can occur during AI completion requests.
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
}
