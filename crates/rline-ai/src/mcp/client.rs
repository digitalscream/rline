//! MCP client — manages a single MCP server process over stdio.
//!
//! Communicates using newline-delimited JSON-RPC 2.0 messages on stdin/stdout.
//! A background reader task dispatches responses to waiting callers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::{debug, trace, warn};

use super::config::McpServerConfig;
use super::types::{
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, McpToolCallResult, McpToolInfo,
};
use crate::error::AiError;

/// Client for a single MCP server process.
pub struct McpClient {
    /// Human-readable server name (from config key).
    server_name: String,
    /// The server configuration (for restarts).
    config: McpServerConfig,
    /// The child process.
    child: Child,
    /// Buffered writer to the child's stdin.
    stdin: BufWriter<ChildStdin>,
    /// Pending request senders, keyed by JSON-RPC request id.
    pending: Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,
    /// Monotonically increasing request id.
    next_id: Arc<AtomicU64>,
    /// Handle to the background stdout reader task.
    reader_handle: Option<JoinHandle<()>>,
}

impl McpClient {
    /// Spawn an MCP server process and set up stdio communication.
    pub async fn start(name: String, config: McpServerConfig) -> Result<Self, AiError> {
        debug!(
            "starting MCP server '{name}': {} {:?}",
            config.command, config.args
        );

        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if let Some(env) = &config.env {
            for (k, v) in env {
                cmd.env(k, v);
            }
        }

        let mut child = cmd.spawn().map_err(|e| AiError::McpTransport {
            server: name.clone(),
            detail: format!("failed to spawn process '{}': {e}", config.command),
        })?;

        let child_stdin = child.stdin.take().ok_or_else(|| AiError::McpTransport {
            server: name.clone(),
            detail: "failed to capture child stdin".to_owned(),
        })?;

        let child_stdout = child.stdout.take().ok_or_else(|| AiError::McpTransport {
            server: name.clone(),
            detail: "failed to capture child stdout".to_owned(),
        })?;

        let pending: Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(std::sync::Mutex::new(HashMap::new()));

        let reader_handle = Self::spawn_reader(name.clone(), child_stdout, pending.clone());

        Ok(Self {
            server_name: name,
            config,
            child,
            stdin: BufWriter::new(child_stdin),
            pending,
            next_id: Arc::new(AtomicU64::new(1)),
            reader_handle: Some(reader_handle),
        })
    }

    /// Spawn a background task that reads JSON-RPC responses from stdout.
    fn spawn_reader(
        server_name: String,
        stdout: ChildStdout,
        pending: Arc<std::sync::Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let line = line.trim().to_owned();
                        if line.is_empty() {
                            continue;
                        }

                        trace!("MCP '{server_name}' stdout: {line}");

                        match serde_json::from_str::<JsonRpcResponse>(&line) {
                            Ok(resp) => {
                                if let Some(id) = resp.id {
                                    // Dispatch to the waiting caller.
                                    let sender = pending
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .remove(&id);
                                    if let Some(tx) = sender {
                                        let _ = tx.send(resp);
                                    } else {
                                        warn!("MCP '{server_name}': response for unknown id {id}");
                                    }
                                } else {
                                    // Server notification — log and discard.
                                    debug!("MCP '{server_name}': received notification: {line}");
                                }
                            }
                            Err(e) => {
                                debug!("MCP '{server_name}': ignoring unparseable line: {e}");
                            }
                        }
                    }
                    Ok(None) => {
                        // EOF — server process exited.
                        debug!("MCP '{server_name}': stdout closed (process exited)");
                        // Drop all pending senders so callers get errors.
                        pending.lock().unwrap_or_else(|e| e.into_inner()).clear();
                        break;
                    }
                    Err(e) => {
                        warn!("MCP '{server_name}': read error: {e}");
                        pending.lock().unwrap_or_else(|e| e.into_inner()).clear();
                        break;
                    }
                }
            }
        })
    }

    /// Send a JSON-RPC request and wait for the response.
    async fn send_request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, AiError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = JsonRpcRequest::new(id, method, params);

        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);

        // Write the request as a newline-delimited JSON message.
        let json = serde_json::to_string(&request).map_err(|e| AiError::McpTransport {
            server: self.server_name.clone(),
            detail: format!("failed to serialize request: {e}"),
        })?;

        self.stdin
            .write_all(json.as_bytes())
            .await
            .map_err(|e| AiError::McpTransport {
                server: self.server_name.clone(),
                detail: format!("failed to write to stdin: {e}"),
            })?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|e| AiError::McpTransport {
                server: self.server_name.clone(),
                detail: format!("failed to write newline: {e}"),
            })?;
        self.stdin
            .flush()
            .await
            .map_err(|e| AiError::McpTransport {
                server: self.server_name.clone(),
                detail: format!("failed to flush stdin: {e}"),
            })?;

        // Wait for the response.
        let resp = rx.await.map_err(|_| AiError::McpTransport {
            server: self.server_name.clone(),
            detail: "response channel closed (server may have exited)".to_owned(),
        })?;

        if let Some(err) = resp.error {
            return Err(AiError::McpServerError {
                server: self.server_name.clone(),
                message: format!("[{}] {}", err.code, err.message),
            });
        }

        resp.result.ok_or_else(|| AiError::McpServerError {
            server: self.server_name.clone(),
            message: "response has neither result nor error".to_owned(),
        })
    }

    /// Send a JSON-RPC notification (no response expected).
    async fn send_notification(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), AiError> {
        let notification = JsonRpcNotification::new(method, params);
        let json = serde_json::to_string(&notification).map_err(|e| AiError::McpTransport {
            server: self.server_name.clone(),
            detail: format!("failed to serialize notification: {e}"),
        })?;

        self.stdin
            .write_all(json.as_bytes())
            .await
            .map_err(|e| AiError::McpTransport {
                server: self.server_name.clone(),
                detail: format!("failed to write notification: {e}"),
            })?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|e| AiError::McpTransport {
                server: self.server_name.clone(),
                detail: format!("failed to write newline: {e}"),
            })?;
        self.stdin
            .flush()
            .await
            .map_err(|e| AiError::McpTransport {
                server: self.server_name.clone(),
                detail: format!("failed to flush notification: {e}"),
            })?;

        Ok(())
    }

    /// Perform the MCP initialize handshake.
    ///
    /// Sends an `initialize` request followed by an `initialized` notification.
    pub async fn initialize(&mut self) -> Result<(), AiError> {
        debug!("MCP '{}': sending initialize", self.server_name);

        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "rline",
                "version": env!("CARGO_PKG_VERSION")
            }
        });

        let result = self.send_request("initialize", Some(params)).await?;
        debug!("MCP '{}': initialize result: {result}", self.server_name);

        // Send the initialized notification.
        self.send_notification("initialized", None).await?;
        debug!("MCP '{}': initialization complete", self.server_name);

        Ok(())
    }

    /// Discover available tools from this server.
    pub async fn list_tools(&mut self) -> Result<Vec<McpToolInfo>, AiError> {
        debug!("MCP '{}': listing tools", self.server_name);

        let result = self.send_request("tools/list", None).await?;

        // The result should have a "tools" array.
        let tools_value = result
            .get("tools")
            .cloned()
            .unwrap_or(serde_json::Value::Array(Vec::new()));

        let tools: Vec<McpToolInfo> =
            serde_json::from_value(tools_value).map_err(|e| AiError::McpServerError {
                server: self.server_name.clone(),
                message: format!("failed to parse tools/list response: {e}"),
            })?;

        debug!(
            "MCP '{}': discovered {} tools",
            self.server_name,
            tools.len()
        );
        Ok(tools)
    }

    /// Execute a tool call on this server.
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallResult, AiError> {
        debug!("MCP '{}': calling tool '{name}'", self.server_name);

        let params = serde_json::json!({
            "name": name,
            "arguments": arguments
        });

        let result = self.send_request("tools/call", Some(params)).await?;

        let call_result: McpToolCallResult =
            serde_json::from_value(result).map_err(|e| AiError::McpServerError {
                server: self.server_name.clone(),
                message: format!("failed to parse tools/call response: {e}"),
            })?;

        Ok(call_result)
    }

    /// Shut down the server process.
    pub async fn shutdown(&mut self) {
        debug!("MCP '{}': shutting down", self.server_name);

        // Abort the reader task.
        if let Some(handle) = self.reader_handle.take() {
            handle.abort();
        }

        // Kill the child process.
        let _ = self.child.kill().await;
    }

    /// The server name.
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// Whether this server's tools are trusted.
    pub fn trusted(&self) -> bool {
        self.config.trusted
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        // Best-effort kill on drop (sync).
        if let Some(handle) = self.reader_handle.take() {
            handle.abort();
        }
        let _ = self.child.start_kill();
    }
}

impl std::fmt::Debug for McpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClient")
            .field("server_name", &self.server_name)
            .field("command", &self.config.command)
            .finish()
    }
}
