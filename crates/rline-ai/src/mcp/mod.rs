//! MCP (Model Context Protocol) client for connecting to external tool servers.
//!
//! Spawns MCP server processes, performs the JSON-RPC 2.0 handshake over stdio,
//! discovers tools via `tools/list`, and executes tool calls via `tools/call`.
//! Tools from MCP servers are integrated into the agent's [`ToolRegistry`](crate::tools::ToolRegistry)
//! alongside built-in tools.

pub mod client;
pub mod config;
pub mod manager;
pub mod tool;
pub mod types;
