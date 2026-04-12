//! rline-ai — AI completion and agentic tool-use client for code editors.
//!
//! Provides HTTP clients for OpenAI-compatible endpoints:
//! - [`CompletionClient`] for `/v1/completions` (FIM inline completion)
//! - [`chat::ChatClient`] for `/v1/chat/completions` (agentic tool use)
//!
//! The [`agent`] module contains the core agent loop that drives
//! multi-turn tool-use conversations. The [`tools`] module defines
//! the tool trait and built-in tool implementations.
//!
//! Async operations run on a dedicated tokio runtime to avoid blocking GTK.

pub mod agent;
pub mod browser;
pub mod chat;
pub mod client;
pub mod error;
pub mod mcp;
pub mod runtime;
pub mod skills;
pub mod tools;
pub mod types;

pub use client::CompletionClient;
pub use error::AiError;
pub use runtime::ai_runtime;
pub use types::{CompletionRequest, CompletionResponse};
