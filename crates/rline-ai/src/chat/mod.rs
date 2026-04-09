//! Chat completions API client with streaming support.
//!
//! Provides [`ChatClient`] for communicating with OpenAI-compatible
//! `/v1/chat/completions` endpoints, including tool/function calling
//! and incremental SSE streaming.

pub mod client;
pub mod stream;
pub mod types;

pub use client::{ChatClient, StreamEvent};
pub use types::{
    ChatMessage, ChatRequest, ChatStreamChunk, ChatStreamDelta, FunctionCall, FunctionDefinition,
    Role, ToolCall, ToolCallDelta, ToolDefinition,
};
