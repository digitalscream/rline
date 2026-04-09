//! Tool: execute a shell command.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

use crate::chat::types::ToolDefinition;
use crate::error::AiError;
use crate::tools::{Tool, ToolCategory, ToolResult};

/// Execute a shell command in the workspace directory.
pub struct ExecuteCommandTool {
    /// Maximum execution time in seconds.
    timeout_secs: u64,
}

impl ExecuteCommandTool {
    /// Create a new command tool with a custom timeout.
    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }
}

impl Default for ExecuteCommandTool {
    fn default() -> Self {
        Self { timeout_secs: 30 }
    }
}

#[derive(Debug, Deserialize)]
struct Args {
    command: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

impl Tool for ExecuteCommandTool {
    fn name(&self) -> &str {
        "execute_command"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "execute_command",
            "Execute a shell command in the workspace directory. The command runs in a bash shell. \
             Returns stdout and stderr. Use this for build commands, tests, git operations, etc.",
            super::definitions::schema! {
                required: ["command"],
                properties: {
                    "command" => serde_json::json!({ "type": "string", "description": "The shell command to execute" }),
                    "timeout_secs" => serde_json::json!({ "type": "integer", "description": "Timeout in seconds (default: 30)" })
                }
            },
        )
    }

    fn execute(&self, arguments: &str, workspace_root: &Path) -> Result<ToolResult, AiError> {
        let args: Args = serde_json::from_str(arguments)?;
        let timeout = Duration::from_secs(args.timeout_secs.unwrap_or(self.timeout_secs));

        let child = Command::new("bash")
            .arg("-c")
            .arg(&args.command)
            .current_dir(workspace_root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();

        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult::err(format!("Failed to spawn command: {e}")));
            }
        };

        // Wait with timeout.
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let output = child.wait_with_output().map_err(AiError::Io)?;
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);

                    let mut result = String::new();
                    if !stdout.is_empty() {
                        result.push_str(&stdout);
                    }
                    if !stderr.is_empty() {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str("STDERR:\n");
                        result.push_str(&stderr);
                    }
                    if result.is_empty() {
                        result = "(no output)".to_owned();
                    }

                    // Truncate very long output.
                    if result.len() > 50_000 {
                        result.truncate(50_000);
                        result.push_str("\n\n(output truncated)");
                    }

                    let exit_code = status.code().unwrap_or(-1);
                    result.push_str(&format!("\n\nExit code: {exit_code}"));

                    return Ok(if status.success() {
                        ToolResult::ok(result)
                    } else {
                        ToolResult::err(result)
                    });
                }
                Ok(None) => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        return Ok(ToolResult::err(format!(
                            "Command timed out after {} seconds",
                            timeout.as_secs()
                        )));
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    return Ok(ToolResult::err(format!("Failed to wait for command: {e}")));
                }
            }
        }
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ExecuteCommand
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_command_echo() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        let tool = ExecuteCommandTool::default();
        let args = serde_json::json!({ "command": "echo hello" }).to_string();

        let result = tool.execute(&args, dir.path()).expect("should execute");
        assert!(result.success);
        assert!(result.output.contains("hello"));
    }

    #[test]
    fn test_execute_command_failure() {
        let dir = tempfile::tempdir().expect("temp dir in test");
        let tool = ExecuteCommandTool::default();
        let args = serde_json::json!({ "command": "exit 1" }).to_string();

        let result = tool.execute(&args, dir.path()).expect("should execute");
        assert!(!result.success);
        assert!(result.output.contains("Exit code: 1"));
    }
}
