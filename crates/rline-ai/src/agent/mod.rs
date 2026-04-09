//! Agent loop for multi-turn agentic tool-use conversations.
//!
//! The agent loop sends messages to the chat API, parses streaming responses,
//! dispatches tool calls, and feeds results back for the next turn.

pub mod context;
pub mod event;
pub mod r#loop;

pub use context::ConversationContext;
pub use event::AgentEvent;
pub use r#loop::{AgentLoop, AgentMode};
