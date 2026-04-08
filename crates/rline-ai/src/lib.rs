//! rline-ai — AI completion client for code editors.
//!
//! Provides an HTTP client for OpenAI-compatible `/v1/completions` endpoints
//! with FIM (Fill-in-the-Middle) support via the `suffix` request field.
//! Async operations run on a dedicated tokio runtime to avoid blocking GTK.

pub mod client;
pub mod error;
pub mod runtime;
pub mod types;

pub use client::CompletionClient;
pub use error::AiError;
pub use runtime::ai_runtime;
pub use types::{CompletionRequest, CompletionResponse};
