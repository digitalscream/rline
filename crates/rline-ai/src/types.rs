//! Request and response types for OpenAI-compatible completion endpoints.

use serde::{Deserialize, Serialize};

/// A request to the OpenAI-compatible `/v1/completions` endpoint.
///
/// Uses the `suffix` field to signal FIM (Fill-in-the-Middle) mode
/// to servers that support it.
#[derive(Debug, Clone, Serialize)]
pub struct CompletionRequest {
    /// The model identifier (e.g. `"codellama"`, `"deepseek-coder"`).
    pub model: String,
    /// Text before the cursor (FIM prefix).
    pub prompt: String,
    /// Text after the cursor (FIM suffix).
    pub suffix: String,
    /// Maximum number of tokens to generate.
    pub max_tokens: u32,
    /// Sampling temperature (0.0 = deterministic).
    pub temperature: f64,
    /// Whether to stream the response. Always `false` for now.
    pub stream: bool,
}

/// A response from the OpenAI-compatible `/v1/completions` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct CompletionResponse {
    /// The list of completion choices.
    pub choices: Vec<CompletionChoice>,
}

/// A single completion choice from the API response.
#[derive(Debug, Clone, Deserialize)]
pub struct CompletionChoice {
    /// The generated completion text.
    pub text: String,
    /// The reason the model stopped generating (e.g. `"stop"`, `"length"`).
    pub finish_reason: Option<String>,
}
