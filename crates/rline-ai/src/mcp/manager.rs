//! MCP server manager — coordinates multiple MCP server connections.
//!
//! Loads configuration, starts all servers, discovers tools, and provides
//! lifecycle management (shutdown).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::{info, warn};

use super::client::McpClient;
use super::config::{self, McpServerConfig};
use super::tool::McpTool;
use crate::error::AiError;
use crate::tools::Tool;

/// Manages the lifecycle of multiple MCP server connections.
pub struct McpManager {
    /// Active server clients, keyed by server name.
    clients: HashMap<String, Arc<Mutex<McpClient>>>,
    /// Server configs, keyed by server name (for trusted flag lookups).
    configs: HashMap<String, McpServerConfig>,
}

impl McpManager {
    /// Load MCP configuration and start all configured servers.
    ///
    /// - `global_config_path`: path to the global MCP config (e.g. `~/.config/rline/mcp.json`)
    /// - `workspace_root`: project directory containing `.mcp.json`
    ///
    /// Servers that fail to start are logged as warnings and skipped.
    /// Returns an empty manager if no MCP config is found.
    pub async fn from_workspace(
        global_config_path: Option<&Path>,
        workspace_root: &Path,
    ) -> Result<Self, AiError> {
        let mcp_config = match config::load_mcp_config(global_config_path, workspace_root) {
            Some(c) => c,
            None => {
                return Ok(Self {
                    clients: HashMap::new(),
                    configs: HashMap::new(),
                });
            }
        };

        let mut clients = HashMap::new();
        let configs = mcp_config.mcp_servers.clone();

        for (name, server_config) in &mcp_config.mcp_servers {
            match Self::start_server(name.clone(), server_config.clone()).await {
                Ok(client) => {
                    info!("MCP server '{name}' started successfully");
                    clients.insert(name.clone(), Arc::new(Mutex::new(client)));
                }
                Err(e) => {
                    warn!("MCP server '{name}' failed to start: {e}");
                }
            }
        }

        Ok(Self { clients, configs })
    }

    /// Start a single MCP server and perform the initialize handshake.
    async fn start_server(name: String, config: McpServerConfig) -> Result<McpClient, AiError> {
        let mut client = McpClient::start(name.clone(), config).await?;

        // Initialize with a 30-second timeout.
        let init =
            tokio::time::timeout(std::time::Duration::from_secs(30), client.initialize()).await;

        match init {
            Ok(Ok(())) => Ok(client),
            Ok(Err(e)) => {
                client.shutdown().await;
                Err(e)
            }
            Err(_) => {
                client.shutdown().await;
                Err(AiError::McpTransport {
                    server: name,
                    detail: "initialize handshake timed out (30s)".to_owned(),
                })
            }
        }
    }

    /// Discover tools from all connected servers.
    ///
    /// Returns a list of [`McpTool`] wrappers that implement the [`Tool`] trait.
    /// Tool names are prefixed as `mcp__{server}__{tool}` to avoid collisions.
    pub async fn discover_tools(&self) -> Vec<Box<dyn Tool>> {
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();

        for (name, client_arc) in &self.clients {
            let trusted = self.configs.get(name).map(|c| c.trusted).unwrap_or(false);

            let tool_infos = {
                let mut client = client_arc.lock().await;
                match client.list_tools().await {
                    Ok(infos) => infos,
                    Err(e) => {
                        warn!("MCP server '{name}': failed to list tools: {e}");
                        continue;
                    }
                }
            };

            for info in tool_infos {
                let description = info
                    .description
                    .unwrap_or_else(|| format!("MCP tool from server '{name}'"));

                let tool = McpTool::new(
                    name.clone(),
                    info.name,
                    description,
                    info.input_schema,
                    client_arc.clone(),
                    trusted,
                );
                tools.push(Box::new(tool));
            }
        }

        info!("discovered {} MCP tools total", tools.len());
        tools
    }

    /// Shut down all MCP server processes.
    pub async fn shutdown_all(&self) {
        for (name, client_arc) in &self.clients {
            info!("shutting down MCP server '{name}'");
            let mut client = client_arc.lock().await;
            client.shutdown().await;
        }
    }

    /// Whether any MCP servers are connected.
    pub fn has_servers(&self) -> bool {
        !self.clients.is_empty()
    }
}

/// Build a human-readable summary of MCP tools for inclusion in the system prompt.
///
/// Takes the already-discovered tool list (from [`McpManager::discover_tools`])
/// and formats a description of each tool. Returns `None` if the list is empty.
pub fn build_tool_summary(tools: &[Box<dyn Tool>]) -> Option<String> {
    if tools.is_empty() {
        return None;
    }

    let lines: Vec<String> = tools
        .iter()
        .map(|t| {
            let desc = t.definition().function.description;
            format!("- `{}`: {}", t.name(), desc)
        })
        .collect();

    Some(lines.join("\n"))
}

impl std::fmt::Debug for McpManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpManager")
            .field("server_count", &self.clients.len())
            .field("servers", &self.clients.keys().collect::<Vec<_>>())
            .finish()
    }
}
