//! MCP tool wrapper — adapts an MCP-discovered tool to the [`Tool`] trait.
//!
//! Each `McpTool` holds an `Arc` reference to its parent [`McpClient`](super::client::McpClient)
//! and delegates `execute()` calls to `tools/call` over the JSON-RPC connection.

use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::client::McpClient;
use crate::chat::types::{FunctionDefinition, ToolDefinition};
use crate::error::AiError;
use crate::runtime::ai_runtime;
use crate::tools::{Tool, ToolCategory, ToolResult};

/// A tool discovered from an MCP server, wrapped to implement [`Tool`].
pub struct McpTool {
    /// The MCP server name this tool belongs to.
    server_name: String,
    /// The original tool name from the server.
    tool_name: String,
    /// The qualified name: `mcp__{server}__{tool}`.
    qualified_name: String,
    /// Human-readable description.
    description: String,
    /// JSON Schema for the tool's input parameters.
    input_schema: serde_json::Value,
    /// Shared reference to the MCP client for this server.
    client: Arc<Mutex<McpClient>>,
    /// Whether this server is trusted (tools auto-approved).
    trusted: bool,
}

impl McpTool {
    /// Create a new MCP tool wrapper.
    pub fn new(
        server_name: String,
        tool_name: String,
        description: String,
        input_schema: serde_json::Value,
        client: Arc<Mutex<McpClient>>,
        trusted: bool,
    ) -> Self {
        let qualified_name = format!("mcp__{server_name}__{tool_name}");
        // Ensure the schema is a valid JSON object — some model servers
        // reject tool definitions with null parameters.
        let input_schema = if input_schema.is_null() || !input_schema.is_object() {
            serde_json::json!({"type": "object", "properties": {}})
        } else {
            input_schema
        };
        Self {
            server_name,
            tool_name,
            qualified_name,
            description,
            input_schema,
            client,
            trusted,
        }
    }
}

impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.qualified_name
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".to_owned(),
            function: FunctionDefinition {
                name: self.qualified_name.clone(),
                description: self.description.clone(),
                parameters: self.input_schema.clone(),
            },
        }
    }

    fn execute(&self, arguments: &str, _workspace_root: &Path) -> Result<ToolResult, AiError> {
        // Parse the arguments JSON string into a Value.
        let args: serde_json::Value =
            serde_json::from_str(arguments).map_err(|e| AiError::ToolExecution {
                tool: self.qualified_name.clone(),
                detail: format!("invalid JSON arguments: {e}"),
            })?;

        // Call the MCP server via the shared client.
        // This runs on the AI runtime — safe because Tool::execute is called
        // from spawn_blocking in the agent loop.
        let client = self.client.clone();
        let tool_name = self.tool_name.clone();
        let server_name = self.server_name.clone();

        let result = ai_runtime().block_on(async move {
            let mut guard = client.lock().await;
            guard.call_tool(&tool_name, args).await
        });

        match result {
            Ok(call_result) => {
                // Concatenate all text content items.
                let output: String = call_result
                    .content
                    .iter()
                    .filter_map(|c| c.text.as_deref())
                    .collect::<Vec<_>>()
                    .join("\n");

                if call_result.is_error {
                    Ok(ToolResult::err(output))
                } else {
                    Ok(ToolResult::ok(output))
                }
            }
            Err(e) => Ok(ToolResult::err(format!(
                "MCP server '{server_name}' error: {e}"
            ))),
        }
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn category(&self) -> ToolCategory {
        if self.trusted {
            ToolCategory::McpTrusted
        } else {
            ToolCategory::McpUntrusted
        }
    }
}

impl std::fmt::Debug for McpTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpTool")
            .field("qualified_name", &self.qualified_name)
            .field("server_name", &self.server_name)
            .field("trusted", &self.trusted)
            .finish()
    }
}
