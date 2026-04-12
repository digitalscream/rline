//! MCP server configuration loading.
//!
//! Loads MCP server definitions from two locations and merges them:
//! 1. Global: `~/.config/rline/mcp.json`
//! 2. Project: `{workspace_root}/.mcp.json`
//!
//! Project-level servers override global servers with the same name.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::warn;

/// Configuration for a single MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// The command to execute (e.g. "npx", "python").
    pub command: String,

    /// Command-line arguments.
    #[serde(default)]
    pub args: Vec<String>,

    /// Optional environment variables for the server process.
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,

    /// Whether this server's tools are trusted (auto-approved).
    ///
    /// When `false` (the default), every tool call requires explicit user
    /// approval. When `true`, tool calls are auto-approved like built-in tools.
    #[serde(default)]
    pub trusted: bool,
}

/// Top-level MCP configuration file format.
///
/// Compatible with the Claude Desktop `claude_desktop_config.json` format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// Map of server name → server configuration.
    #[serde(rename = "mcpServers", default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

/// Load and merge MCP server configurations from global and project locations.
///
/// - `global_config_path`: path to the global MCP config (e.g. `~/.config/rline/mcp.json`)
/// - `workspace_root`: project directory containing the project-level `.mcp.json`
///
/// Project-level servers override global servers with the same name.
/// Returns `None` if no servers are configured from either source.
pub fn load_mcp_config(
    global_config_path: Option<&Path>,
    workspace_root: &Path,
) -> Option<McpConfig> {
    let mut servers = HashMap::new();

    // 1. Global config: ~/.config/rline/mcp.json
    if let Some(global_path) = global_config_path {
        if global_path.exists() {
            match load_config_file(global_path) {
                Ok(config) => {
                    servers.extend(config.mcp_servers);
                }
                Err(e) => {
                    warn!(
                        "failed to parse global MCP config at {}: {e}",
                        global_path.display()
                    );
                }
            }
        }
    }

    // 2. Project config: {workspace_root}/.mcp.json (overrides global)
    let project_path = workspace_root.join(".mcp.json");
    if project_path.exists() {
        match load_config_file(&project_path) {
            Ok(config) => {
                servers.extend(config.mcp_servers);
            }
            Err(e) => {
                warn!(
                    "failed to parse project MCP config at {}: {e}",
                    project_path.display()
                );
            }
        }
    }

    if servers.is_empty() {
        None
    } else {
        Some(McpConfig {
            mcp_servers: servers,
        })
    }
}

/// Load a single MCP config file.
fn load_config_file(path: &Path) -> Result<McpConfig, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_mcp_config() {
        let json = r#"{
            "mcpServers": {
                "context7": {
                    "command": "npx",
                    "args": ["-y", "@upstash/context7-mcp@latest"]
                },
                "filesystem": {
                    "command": "npx",
                    "args": ["@modelcontextprotocol/server-filesystem", "/tmp"],
                    "trusted": true
                }
            }
        }"#;

        let config: McpConfig = serde_json::from_str(json).expect("should parse MCP config");
        assert_eq!(config.mcp_servers.len(), 2, "should have two servers");

        let ctx = &config.mcp_servers["context7"];
        assert_eq!(ctx.command, "npx");
        assert!(!ctx.trusted, "context7 should not be trusted by default");

        let fs = &config.mcp_servers["filesystem"];
        assert!(fs.trusted, "filesystem should be trusted");
    }

    #[test]
    fn test_deserialize_with_env() {
        let json = r#"{
            "mcpServers": {
                "postgres": {
                    "command": "npx",
                    "args": ["@modelcontextprotocol/server-postgres"],
                    "env": {
                        "PGPASSWORD": "secret",
                        "PGHOST": "localhost"
                    }
                }
            }
        }"#;

        let config: McpConfig = serde_json::from_str(json).expect("should parse");
        let pg = &config.mcp_servers["postgres"];
        let env = pg.env.as_ref().expect("should have env");
        assert_eq!(env.get("PGPASSWORD").map(String::as_str), Some("secret"));
    }

    #[test]
    fn test_load_missing_workspace() {
        let result = load_mcp_config(None, Path::new("/nonexistent/path"));
        assert!(result.is_none(), "should return None for missing workspace");
    }

    #[test]
    fn test_trusted_defaults_to_false() {
        let json = r#"{"command": "npx", "args": []}"#;
        let config: McpServerConfig = serde_json::from_str(json).expect("should parse");
        assert!(!config.trusted, "trusted should default to false");
    }
}
