//! Chat completions API clients with streaming support.
//!
//! Two concrete clients are provided — [`ChatClient`] for OpenAI-compatible
//! `/v1/chat/completions` endpoints, and [`AnthropicClient`] for the
//! Anthropic Messages API. Both implement the [`AgentChatClient`] trait so
//! the agent loop can drive either one interchangeably.

pub mod anthropic;
pub mod client;
pub mod stream;
pub mod types;

pub use anthropic::AnthropicClient;
pub use client::{AgentChatClient, ChatClient, StreamEvent};
pub use types::{
    ChatMessage, ChatRequest, ChatStreamChunk, ChatStreamDelta, FunctionCall, FunctionDefinition,
    Role, ToolCall, ToolCallDelta, ToolDefinition,
};
