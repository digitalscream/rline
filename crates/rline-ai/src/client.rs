//! HTTP client for OpenAI-compatible completion endpoints.

use tokio_util::sync::CancellationToken;

use crate::error::AiError;
use crate::types::{CompletionRequest, CompletionResponse};

/// Client for making FIM completion requests to an OpenAI-compatible API.
///
/// Communicates with the `/v1/completions` endpoint using the `suffix`
/// field to signal Fill-in-the-Middle mode.
#[derive(Debug, Clone)]
pub struct CompletionClient {
    http: reqwest::Client,
    endpoint_url: String,
    api_key: Option<String>,
    model: String,
}

impl CompletionClient {
    /// Create a new completion client.
    ///
    /// # Arguments
    /// * `endpoint_url` — Full URL to the completions endpoint
    ///   (e.g. `"http://localhost:8080/v1/completions"`).
    /// * `api_key` — Optional bearer token. Pass `None` or empty string to skip auth.
    /// * `model` — Model identifier sent in requests.
    pub fn new(endpoint_url: &str, api_key: Option<&str>, model: &str) -> Self {
        let api_key = api_key
            .map(|k| k.trim())
            .filter(|k| !k.is_empty())
            .map(String::from);

        Self {
            http: reqwest::Client::new(),
            endpoint_url: endpoint_url.to_owned(),
            api_key,
            model: model.to_owned(),
        }
    }

    /// Request a FIM completion.
    ///
    /// Sends a POST to the configured endpoint with the given prefix/suffix
    /// context. Returns the first completion choice text on success.
    ///
    /// The request is cancellable via the provided `CancellationToken`.
    pub async fn complete(
        &self,
        prefix: &str,
        suffix: &str,
        max_tokens: u32,
        temperature: f64,
        cancel: CancellationToken,
    ) -> Result<String, AiError> {
        let request = CompletionRequest {
            model: self.model.clone(),
            prompt: prefix.to_owned(),
            suffix: suffix.to_owned(),
            max_tokens,
            temperature,
            stream: false,
        };

        let mut req_builder = self.http.post(&self.endpoint_url).json(&request);

        if let Some(ref key) = self.api_key {
            req_builder = req_builder.bearer_auth(key);
        }

        // Race the HTTP request against cancellation.
        let response = tokio::select! {
            result = req_builder.send() => result?,
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

        let body_bytes = tokio::select! {
            result = response.bytes() => result?,
            () = cancel.cancelled() => return Err(AiError::Cancelled),
        };

        let parsed: CompletionResponse = serde_json::from_slice(&body_bytes)?;

        let text = parsed
            .choices
            .into_iter()
            .next()
            .ok_or(AiError::NoChoices)?
            .text;

        Ok(text)
    }
}
