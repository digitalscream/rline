//! AI agent panel — chat UI for agentic tool-use conversations.
//!
//! Provides [`AgentPanel`], the right-pane widget that lets users interact
//! with an AI coding agent in Plan or Act mode.

pub mod agent_panel;
mod markdown;
mod message_widget;
pub mod permission;

pub use agent_panel::AgentPanel;
